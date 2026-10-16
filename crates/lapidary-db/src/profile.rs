//! This install's profile: a display name. See migration `0060`.

use crate::DbError;
use sqlx::PgPool;

pub struct PgProfile(pub PgPool);

impl PgProfile {
    /// The display name, or `None` until somebody gives one.
    pub async fn display_name(&self) -> Result<Option<String>, DbError> {
        Ok(sqlx::query_scalar("SELECT display_name FROM profile")
            .fetch_optional(&self.0)
            .await?
            .flatten())
    }

    /// Set the display name; `None` clears it. The column's check refuses a blank or an over-long one.
    pub async fn set_display_name(&self, name: Option<&str>) -> Result<(), DbError> {
        sqlx::query("UPDATE profile SET display_name = $1, updated_at = now()")
            .bind(name.map(str::trim))
            .execute(&self.0)
            .await?;
        Ok(())
    }
}
