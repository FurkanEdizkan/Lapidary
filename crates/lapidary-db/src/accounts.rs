//! Accounts, sessions and this install's own settings (A1). See migration `0063`.
//!
//! Nothing here hashes a password or makes a token: the API does both, and hands this crate a PHC
//! string and a token's BLAKE3 hash. A session row is found by that hash alone, so the cookie's value
//! never reaches the database.

use crate::DbError;
use lapidary_core::UserId;
use sqlx::PgPool;
use uuid::Uuid;

/// A person, as everything but sign-in sees them: never the password hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRow {
    pub id: UserId,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

type UserTuple = (Uuid, String, Option<String>, bool);

fn user((id, username, display_name, is_admin): UserTuple) -> UserRow {
    UserRow {
        id: UserId::from_uuid(id),
        username,
        display_name,
        is_admin,
    }
}

pub struct PgAccounts(pub PgPool);

impl PgAccounts {
    /// Whether anybody has an account yet. `false` is the one state in which setup may run.
    pub async fn any_user(&self) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app_user)")
            .fetch_one(&self.0)
            .await?)
    }

    /// The setup wizard's account: the admin, and the owner of every library and part that exists.
    ///
    /// `Ok(None)` when somebody already has an account — a second setup is refused, not merged. The
    /// table is locked for the check and the insert, so two browsers racing the wizard make one admin.
    pub async fn create_first_admin(
        &self,
        username: &str,
        display_name: Option<&str>,
        password_hash: &str,
    ) -> Result<Option<UserRow>, DbError> {
        let mut tx = self.0.begin().await?;
        sqlx::query("LOCK TABLE app_user IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM app_user)")
            .fetch_one(&mut *tx)
            .await?;
        if taken {
            tx.rollback().await?;
            return Ok(None);
        }
        let id = UserId::new();
        let row: UserTuple = sqlx::query_as(
            "INSERT INTO app_user (id, username, display_name, password_hash, is_admin) \
             VALUES ($1, $2, $3, $4, true) RETURNING id, username, display_name, is_admin",
        )
        .bind(id.as_uuid())
        .bind(username)
        .bind(display_name)
        .bind(password_hash)
        .fetch_one(&mut *tx)
        .await?;
        // Every model from before accounts is the first account's: nothing goes unowned, and nothing
        // is lost by an upgrade.
        sqlx::query("UPDATE library SET owner_id = $1 WHERE owner_id IS NULL")
            .bind(id.as_uuid())
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE part SET owner_id = $1 WHERE owner_id IS NULL")
            .bind(id.as_uuid())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM instance_setting WHERE key = 'setup_code'")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(user(row)))
    }

    /// A person and their password hash, for sign-in only. `username` is already case-folded.
    pub async fn credentials(&self, username: &str) -> Result<Option<(UserRow, String)>, DbError> {
        let row: Option<(Uuid, String, Option<String>, bool, String)> = sqlx::query_as(
            "SELECT id, username, display_name, is_admin, password_hash FROM app_user WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.0)
        .await?;
        Ok(row.map(|(id, username, display_name, is_admin, hash)| {
            (user((id, username, display_name, is_admin)), hash)
        }))
    }

    /// A new session for `user`, found later by `token_hash`, ending `days` days from now unless used.
    pub async fn start_session(
        &self,
        user: UserId,
        token_hash: &[u8; 32],
        days: i32,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO session (token_hash, user_id, expires_at) \
             VALUES ($1, $2, now() + make_interval(days => $3))",
        )
        .bind(&token_hash[..])
        .bind(user.as_uuid())
        .bind(days)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Who a live session belongs to, or `None` for one that is unknown or has expired.
    ///
    /// A session in use slides: its end moves to `days` from now, at most once an hour, so a request
    /// costs one write an hour rather than one write each.
    pub async fn session_user(
        &self,
        token_hash: &[u8; 32],
        days: i32,
    ) -> Result<Option<UserRow>, DbError> {
        let row: Option<(Uuid, String, Option<String>, bool, bool)> = sqlx::query_as(
            "SELECT u.id, u.username, u.display_name, u.is_admin, \
                    s.last_seen_at < now() - interval '1 hour' \
             FROM session s JOIN app_user u ON u.id = s.user_id \
             WHERE s.token_hash = $1 AND s.expires_at > now()",
        )
        .bind(&token_hash[..])
        .fetch_optional(&self.0)
        .await?;
        let Some((id, username, display_name, is_admin, stale)) = row else {
            return Ok(None);
        };
        if stale {
            sqlx::query(
                "UPDATE session SET last_seen_at = now(), expires_at = now() + make_interval(days => $2) \
                 WHERE token_hash = $1",
            )
            .bind(&token_hash[..])
            .bind(days)
            .execute(&self.0)
            .await?;
        }
        Ok(Some(user((id, username, display_name, is_admin))))
    }

    /// Sign one session out. Unknown is not an error: signing out twice is still signed out.
    pub async fn end_session(&self, token_hash: &[u8; 32]) -> Result<(), DbError> {
        sqlx::query("DELETE FROM session WHERE token_hash = $1")
            .bind(&token_hash[..])
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// A new password for `username`, and every session of theirs ended with it. `false` when there
    /// is no such person.
    pub async fn set_password(&self, username: &str, password_hash: &str) -> Result<bool, DbError> {
        let mut tx = self.0.begin().await?;
        let id: Option<Uuid> = sqlx::query_scalar(
            "UPDATE app_user SET password_hash = $2 WHERE username = $1 RETURNING id",
        )
        .bind(username)
        .bind(password_hash)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(id) = id else {
            tx.rollback().await?;
            return Ok(false);
        };
        sqlx::query("DELETE FROM session WHERE user_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Set a person's display name; `None` clears it.
    pub async fn set_display_name(&self, user: UserId, name: Option<&str>) -> Result<(), DbError> {
        sqlx::query("UPDATE app_user SET display_name = $2 WHERE id = $1")
            .bind(user.as_uuid())
            .bind(name.map(str::trim))
            .execute(&self.0)
            .await?;
        Ok(())
    }
}

/// This install's own settings (`instance_setting`), each a JSON value under a key.
pub struct PgSettings(pub PgPool);

impl PgSettings {
    pub async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, DbError> {
        let text: Option<String> =
            sqlx::query_scalar("SELECT value::text FROM instance_setting WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.0)
                .await?;
        Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
    }

    pub async fn set(&self, key: &str, value: &serde_json::Value) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO instance_setting (key, value) VALUES ($1, $2::jsonb) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = now()",
        )
        .bind(key)
        .bind(value.to_string())
        .execute(&self.0)
        .await?;
        Ok(())
    }
}
