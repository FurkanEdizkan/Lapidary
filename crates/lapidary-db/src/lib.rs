//! Every SQL statement in Lapidary lives in this crate. Other crates go through the
//! repository traits below.

mod repo;

pub use repo::PartRepository;
pub use sqlx::PgPool;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(
        "Could not reach the database at {target}. Check that the `db` service is running and that DATABASE_URL in your .env matches it."
    )]
    Unreachable { target: String },

    #[error(
        "The database at {target} rejected the credentials. Check POSTGRES_PASSWORD and DATABASE_URL in your .env — the service is reachable, so this is not a connectivity problem."
    )]
    AuthenticationFailed { target: String },

    #[error(
        "The database `{database}` does not exist at {target}. Check DATABASE_URL in your .env, or create the database before starting Lapidary."
    )]
    DatabaseMissing { target: String, database: String },

    #[error(
        "The database is PostgreSQL {found}, but Lapidary requires 18 or newer. Generated columns must be STORED, which earlier versions do not support."
    )]
    UnsupportedVersion { found: String },

    #[error("A database query failed: {0}")]
    Query(#[from] sqlx::Error),

    #[error(
        "Could not bring the database schema up to date: {0}. If the database is at a newer schema version than this binary, check that the api and worker images are the same version."
    )]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// Strip credentials from a connection URL so it is safe to put in an error or a log.
/// `postgres://user:pw@host:5432/db` becomes `postgres://host:5432/db`.
///
/// This matters because `main` returns `anyhow::Result`, and anyhow prints the whole
/// source chain on exit. Without redaction the connection string — password included —
/// lands in `podman logs` the first time a container cannot reach its database.
/// Splits on the LAST `@` so a password that itself contains `@` is still removed.
pub(crate) fn redact_credentials(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "the configured database".to_owned();
    };
    // The authority ends at the first '/', '?' or '#'. RFC 3986 requires userinfo to
    // percent-encode all three, so in a well-formed URL the credentials are entirely
    // inside `authority`.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);

    match authority.rsplit_once('@') {
        // Well-formed: strip the userinfo, keep host and path. Drop query and fragment —
        // libpq URIs accept `?password=...`, so keeping them would leak by another route.
        Some((_credentials, host)) => {
            let path = tail.split(['?', '#']).next().unwrap_or("");
            format!("{scheme}://{host}{path}")
        }
        // No '@' in the authority, but one appears later. Either it is a harmless '@' in a
        // query string, or the userinfo contains an unencoded '/', '?' or '#' and the real
        // credentials are sitting in `tail`. We cannot tell which without a full parser, so
        // fail closed: an operator losing the hostname from one error message is a far
        // cheaper mistake than printing a password.
        None if tail.contains('@') => format!("{scheme}://<redacted>"),
        // No credentials anywhere.
        None => {
            let path = tail.split(['?', '#']).next().unwrap_or("");
            format!("{scheme}://{authority}{path}")
        }
    }
}

/// Turn a failed connection attempt into the right `DbError` variant, based on the
/// SQLSTATE PostgreSQL reports rather than by guessing from `Display` text. Kept as a
/// free function taking `&sqlx::Error` (rather than living inline in `connect()`'s
/// `.map_err`) so it can be unit-tested with a synthetic error and no live server.
///
/// `url` is the *only* thing here allowed to be a raw connection string, and it never
/// leaves this function except through `redact_credentials`. Every returned variant
/// carries `target`, not `url` — see the credential-leak tests below.
fn classify_connect_error(err: &sqlx::Error, url: &str) -> DbError {
    let target = redact_credentials(url);

    let sqlx::Error::Database(db_err) = err else {
        // Not a structured database error at all — IO, DNS, TLS, or a pool timeout.
        // The server never got far enough to reject anything, so this is a
        // connectivity problem by elimination.
        return DbError::Unreachable { target };
    };

    match db_err.code().as_deref() {
        // invalid_password / invalid_authorization_specification: the server spoke
        // the wire protocol and answered — it just did not like who we claimed to be.
        Some("28P01" | "28000") => DbError::AuthenticationFailed { target },
        // invalid_catalog_name: the server accepted the credentials, but the database
        // named in the connection string is not there.
        Some("3D000") => DbError::DatabaseMissing {
            // PostgreSQL's own message already names the database, e.g.
            // `database "widgets" does not exist` — reuse that instead of re-deriving
            // it from `url`, so this path never has to reason about redaction at all.
            database: quoted_name(db_err.message())
                .unwrap_or("the configured database")
                .to_owned(),
            target,
        },
        // Any other SQLSTATE (or none) — stay with the fallback rather than invent a
        // variant for a code we have not actually seen in practice.
        _ => DbError::Unreachable { target },
    }
}

/// Pull the first double-quoted substring out of a PostgreSQL error message, e.g.
/// `database "widgets" does not exist` -> `Some("widgets")`. The message is
/// server-supplied text, never client input, so this never touches the connection URL.
fn quoted_name(message: &str) -> Option<&str> {
    let start = message.find('"')? + 1;
    let end = start + message[start..].find('"')?;
    Some(&message[start..end])
}

/// Connect and verify the server is PostgreSQL 18 or newer.
pub async fn connect(url: &str) -> Result<PgPool, DbError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await
        .map_err(|e| classify_connect_error(&e, url))?;

    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(&pool)
        .await?;

    if version < 180_000 {
        return Err(DbError::UnsupportedVersion {
            found: (version / 10_000).to_string(),
        });
    }

    Ok(pool)
}

/// Apply every migration in `crates/lapidary-db/migrations`. `sqlx::migrate!` embeds them
/// at compile time, so an image carries its own schema and an air-gapped operator needs no
/// migration tooling on the host.
pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

/// The PostgreSQL `server_version_num` (e.g. 180002). Lives here because no SQL may
/// appear outside this crate.
pub async fn server_version_num(pool: &PgPool) -> Result<i32, DbError> {
    let num = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(pool)
        .await?;
    Ok(num)
}

#[cfg(test)]
mod tests {
    use super::{DbError, classify_connect_error, redact_credentials};

    #[test]
    fn redaction_removes_the_password() {
        let out = redact_credentials("postgres://lapidary:sup3rs3cret@db:5432/lapidary");
        assert_eq!(out, "postgres://db:5432/lapidary");
        assert!(!out.contains("sup3rs3cret"));
    }

    #[test]
    fn redaction_handles_a_password_containing_an_at_sign() {
        let out = redact_credentials("postgres://lapidary:p@ss@db:5432/lapidary");
        assert!(!out.contains("p@ss"), "must split on the last @, got {out}");
        assert_eq!(out, "postgres://db:5432/lapidary");
    }

    #[test]
    fn redaction_is_scoped_to_the_authority_not_the_query_string() {
        // The last '@' here is inside the query string. Splitting on it would report the
        // host as "bar".
        let out = redact_credentials("postgres://user:pass@host:5432/db?options=foo@bar");
        assert_eq!(out, "postgres://host:5432/db");
    }

    #[test]
    fn redaction_drops_a_password_carried_in_the_query_string() {
        // libpq URIs accept ?password=... . Keeping the query would leak it even though
        // the authority had no credentials to strip.
        let out = redact_credentials("postgres://host:5432/db?password=hunter2");
        assert!(
            !out.contains("hunter2"),
            "query-string password must not survive, got {out}"
        );
        assert_eq!(out, "postgres://host:5432/db");
    }

    #[test]
    fn redaction_keeps_a_bracketed_ipv6_host() {
        assert_eq!(
            redact_credentials("postgres://user:pw@[::1]:5432/db"),
            "postgres://[::1]:5432/db"
        );
    }

    #[test]
    fn redaction_passes_through_a_url_with_no_credentials() {
        assert_eq!(
            redact_credentials("postgres://db:5432/lapidary"),
            "postgres://db:5432/lapidary"
        );
    }

    /// An unencoded '/' in the password truncates the authority before the real '@'.
    /// A previous version of this function returned the URL completely unredacted here.
    #[test]
    fn redaction_fails_closed_on_an_unencoded_slash_in_the_password() {
        let out = redact_credentials("postgres://user:p/ssw0rd@host:5432/db");
        assert!(
            !out.contains("ssw0rd"),
            "password must not survive, got {out}"
        );
        assert_eq!(out, "postgres://<redacted>");
    }

    #[test]
    fn redaction_fails_closed_on_an_unencoded_question_mark_or_hash_in_the_password() {
        for url in [
            "postgres://user:p?ss@host:5432/db",
            "postgres://user:p#ss@host:5432/db",
        ] {
            let out = redact_credentials(url);
            assert_eq!(out, "postgres://<redacted>", "input {url}");
        }
    }

    #[test]
    fn redaction_fails_closed_on_an_unencoded_slash_in_the_username() {
        let out = redact_credentials("postgres://ab/cd:secret@host:5432/db");
        assert!(
            !out.contains("secret"),
            "password must not survive, got {out}"
        );
        assert_eq!(out, "postgres://<redacted>");
    }

    /// Fragments are dropped on the well-formed path too, not only on the fail-closed one.
    /// Without this test, changing `tail.split(['?', '#'])` to `tail.split(['?'])` passes the
    /// whole suite while leaking whatever follows a '#'.
    #[test]
    fn redaction_drops_a_fragment_on_a_well_formed_url() {
        let out = redact_credentials("postgres://user:pw@host:5432/db#secretfragment");
        assert!(
            !out.contains("secretfragment"),
            "fragment must not survive, got {out}"
        );
        assert_eq!(out, "postgres://host:5432/db");
    }

    #[test]
    fn redaction_drops_a_fragment_when_there_are_no_credentials() {
        let out = redact_credentials("postgres://host:5432/db#secretfragment");
        assert!(
            !out.contains("secretfragment"),
            "fragment must not survive, got {out}"
        );
        assert_eq!(out, "postgres://host:5432/db");
    }

    // --- classify_connect_error ------------------------------------------------
    //
    // `sqlx::Error::Database` holds a `Box<dyn sqlx::error::DatabaseError>`, and there
    // is no way to construct sqlx's own implementation of that trait without a live
    // connection to misconfigure. The trait is public and unsealed
    // (`sqlx-core-0.9.0/src/error.rs`), so a minimal test-only double stands in.

    use sqlx::error::{DatabaseError, ErrorKind};
    use std::borrow::Cow;
    use std::error::Error as StdError;
    use std::fmt;

    #[derive(Debug)]
    struct FakeDbError {
        code: Option<&'static str>,
        message: String,
    }

    impl fmt::Display for FakeDbError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.message)
        }
    }

    impl StdError for FakeDbError {}

    impl DatabaseError for FakeDbError {
        fn message(&self) -> &str {
            &self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.map(Cow::Borrowed)
        }

        fn kind(&self) -> ErrorKind {
            // None of the classifier's decisions depend on `kind()` — it dispatches on
            // SQLSTATE — so `Other` is a fine stand-in for every test case here.
            ErrorKind::Other
        }

        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }
    }

    fn database_error(code: &'static str, message: &str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(FakeDbError {
            code: Some(code),
            message: message.to_owned(),
        }))
    }

    /// A connection string carrying a real-looking password, reused by every
    /// credential-leak test below. Matches the URL in the Phase 0a incident report.
    const URL_WITH_PASSWORD: &str = "postgres://lapidary:sup3rs3cret@db:5432/lapidary";

    #[test]
    fn invalid_password_classifies_as_authentication_failed() {
        let err = database_error(
            "28P01",
            "password authentication failed for user \"lapidary\"",
        );
        let classified = classify_connect_error(&err, URL_WITH_PASSWORD);

        assert!(matches!(classified, DbError::AuthenticationFailed { .. }));
        let message = classified.to_string();
        assert!(
            message.contains("credentials") || message.contains("POSTGRES_PASSWORD"),
            "message should point at credentials, got: {message}"
        );
        assert!(
            !message.contains("service is running"),
            "authentication failures must not send the operator to check the service, got: {message}"
        );
    }

    #[test]
    fn invalid_authorization_specification_also_classifies_as_authentication_failed() {
        let err = database_error("28000", "invalid authorization specification");
        let classified = classify_connect_error(&err, URL_WITH_PASSWORD);
        assert!(matches!(classified, DbError::AuthenticationFailed { .. }));
    }

    #[test]
    fn invalid_catalog_name_classifies_as_database_missing_and_names_the_database() {
        let err = database_error("3D000", "database \"widgets\" does not exist");
        let classified = classify_connect_error(&err, URL_WITH_PASSWORD);

        match &classified {
            DbError::DatabaseMissing { database, .. } => assert_eq!(database, "widgets"),
            other => panic!("expected DatabaseMissing, got {other:?}"),
        }
        assert!(classified.to_string().contains("widgets"));
    }

    #[test]
    fn an_unrecognised_sqlstate_falls_back_to_unreachable() {
        let err = database_error("55000", "some database error we do not classify");
        let classified = classify_connect_error(&err, URL_WITH_PASSWORD);
        assert!(matches!(classified, DbError::Unreachable { .. }));
    }

    #[test]
    fn a_non_database_error_falls_back_to_unreachable() {
        let classified = classify_connect_error(&sqlx::Error::PoolTimedOut, URL_WITH_PASSWORD);
        assert!(matches!(classified, DbError::Unreachable { .. }));
    }

    /// Regression test for the Phase 0a credential leak: `DbError::Unreachable` once
    /// carried the raw connection URL, and because `main` returns `anyhow::Result`
    /// (which prints the whole source chain), the password reached `podman logs` the
    /// first time a container could not reach its database. Every variant
    /// `classify_connect_error` can produce must be covered here, not only the ones
    /// that existed at the time of that incident.
    #[test]
    fn no_classified_variant_ever_renders_the_password() {
        let cases: Vec<sqlx::Error> = vec![
            database_error(
                "28P01",
                "password authentication failed for user \"lapidary\"",
            ),
            database_error("28000", "invalid authorization specification"),
            database_error("3D000", "database \"lapidary\" does not exist"),
            database_error("55000", "some database error we do not classify"),
            sqlx::Error::PoolTimedOut,
        ];

        for err in cases {
            let classified = classify_connect_error(&err, URL_WITH_PASSWORD);
            let rendered = classified.to_string();
            assert!(
                !rendered.contains("sup3rs3cret"),
                "variant {classified:?} leaked the password: {rendered}"
            );
        }
    }
}
