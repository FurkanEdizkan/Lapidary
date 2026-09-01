#![deny(clippy::unwrap_used)]
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

/// Connect and verify the server is PostgreSQL 18 or newer.
pub async fn connect(url: &str) -> Result<PgPool, DbError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await
        .map_err(|_| DbError::Unreachable {
            target: redact_credentials(url),
        })?;

    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(&pool)
        .await?;

    if version < 180_000 {
        return Err(DbError::UnsupportedVersion {
            found: version.to_string(),
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
    use super::redact_credentials;

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
}
