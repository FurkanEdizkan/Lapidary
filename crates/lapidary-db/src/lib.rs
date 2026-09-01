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
        "Could not reach the database at {url}. Check that the `db` service is running and that DATABASE_URL in your .env matches it."
    )]
    Unreachable { url: String },

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

/// Connect and verify the server is PostgreSQL 18 or newer.
pub async fn connect(url: &str) -> Result<PgPool, DbError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await
        .map_err(|_| DbError::Unreachable {
            url: url.to_owned(),
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
