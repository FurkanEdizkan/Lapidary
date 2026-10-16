//! The keys saved for the sites models are imported from. See migration `0059`.

use crate::DbError;
use sqlx::PgPool;

/// One saved key, as the settings page may see it: never the key itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteKeyRow {
    pub site: String,
    /// The last four characters, so a person can tell which key is saved.
    pub hint: String,
    /// The last check's answer, `None` until the key is checked.
    pub checked_ok: Option<bool>,
    pub checked_at_us: Option<i64>,
    pub updated_at_us: i64,
}

pub struct PgSiteKeys(pub PgPool);

/// `list`'s SELECT, in its order.
type KeyRow = (String, String, Option<bool>, Option<i64>, i64);

impl PgSiteKeys {
    /// Every saved key, by site.
    pub async fn list(&self) -> Result<Vec<SiteKeyRow>, DbError> {
        let rows: Vec<KeyRow> = sqlx::query_as(
            "SELECT site, right(secret, 4), checked_ok, \
                    (extract(epoch FROM checked_at) * 1000000)::bigint, \
                    (extract(epoch FROM updated_at) * 1000000)::bigint \
             FROM site_key ORDER BY site",
        )
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(site, hint, checked_ok, checked_at_us, updated_at_us)| SiteKeyRow {
                    site,
                    hint,
                    checked_ok,
                    checked_at_us,
                    updated_at_us,
                },
            )
            .collect())
    }

    /// The key itself, for the one thing that uses it: a request to its site.
    pub async fn secret(&self, site: &str) -> Result<Option<String>, DbError> {
        Ok(
            sqlx::query_scalar("SELECT secret FROM site_key WHERE site = $1")
                .bind(site)
                .fetch_optional(&self.0)
                .await?,
        )
    }

    /// Save `secret` for `site`, replacing what was there and forgetting its last check.
    pub async fn set(&self, site: &str, secret: &str) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO site_key (site, secret) VALUES ($1, $2) \
             ON CONFLICT (site) DO UPDATE SET secret = EXCLUDED.secret, checked_ok = NULL, \
                 checked_at = NULL, updated_at = now()",
        )
        .bind(site)
        .bind(secret.trim())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Record what checking the saved key said. Nothing when no key is saved.
    pub async fn record_check(&self, site: &str, ok: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE site_key SET checked_ok = $2, checked_at = now() WHERE site = $1")
            .bind(site)
            .bind(ok)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// Forget the key for `site`. Whether there was one.
    pub async fn clear(&self, site: &str) -> Result<bool, DbError> {
        let done = sqlx::query("DELETE FROM site_key WHERE site = $1")
            .bind(site)
            .execute(&self.0)
            .await?;
        Ok(done.rows_affected() > 0)
    }
}
