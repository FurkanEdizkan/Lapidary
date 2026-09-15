//! Saved filters: a name for a set of the grid's filters, kept per library, in an order people choose.
//!
//! This crate stores the filters and does not read inside them, with one exception: whether the
//! category a filter names is still there. What a filter may hold is the API's decision (`lapidary-api`'s
//! `filters.rs`), made once, before anything is written.

use crate::DbError;
use crate::folders::constraint_of;
use lapidary_core::{LibraryId, SavedFilterId};
use sqlx::PgPool;
use uuid::Uuid;

/// One saved filter as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFilterRow {
    pub id: SavedFilterId,
    pub name: String,
    /// The filters, as the JSON text the database hands back.
    pub search: String,
    /// It names a category that has been deleted since, or that no longer exists at all.
    pub folder_gone: bool,
}

/// Which way a saved filter moves in its library's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMove {
    Up,
    Down,
}

pub struct PgSavedFilters(pub PgPool);

impl PgSavedFilters {
    /// A library's saved filters, in their order. An id naming no library has none.
    ///
    /// Read on every list, so a category that is restored clears its filters' mark again.
    pub async fn list(&self, library: LibraryId) -> Result<Vec<SavedFilterRow>, DbError> {
        let rows: Vec<(Uuid, String, String, bool)> = sqlx::query_as(
            "SELECT s.id, s.name, s.search::text, \
                    (s.search ? 'folderId') AND (f.id IS NULL OR f.deleted_at IS NOT NULL) \
             FROM saved_filter s \
             LEFT JOIN folder f ON f.id = (s.search->>'folderId')::uuid \
             WHERE s.library_id = $1 \
             ORDER BY s.position, s.created_at, s.id",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, search, folder_gone)| SavedFilterRow {
                id: SavedFilterId::from_uuid(id),
                name,
                search,
                folder_gone,
            })
            .collect())
    }

    /// Save `search` under `name`, last in the library's list. The constraints decide a taken name and
    /// an unknown library, rather than a read first that a concurrent write could still invalidate.
    pub async fn create(
        &self,
        library: LibraryId,
        name: &str,
        search: &serde_json::Value,
    ) -> Result<SavedFilterId, DbError> {
        let id = SavedFilterId::new();
        sqlx::query(
            "INSERT INTO saved_filter (id, library_id, name, search, position) \
             VALUES ($1, $2, $3, $4::jsonb, \
                     (SELECT coalesce(max(position) + 1, 0) FROM saved_filter WHERE library_id = $2))",
        )
        .bind(id.as_uuid())
        .bind(library.as_uuid())
        .bind(name)
        .bind(search.to_string())
        .execute(&self.0)
        .await
        .map_err(|err| match constraint_of(&err).as_deref() {
            Some("saved_filter_library_id_fkey") => DbError::NoSuchLibrary { library },
            Some("saved_filter_name_unique_per_library") => DbError::SavedFilterNameTaken {
                name: name.to_owned(),
            },
            _ => DbError::Query(err),
        })?;
        Ok(id)
    }

    /// Give a saved filter a new name, still unique in its library. `false` when that library holds no
    /// such filter.
    pub async fn rename(
        &self,
        library: LibraryId,
        filter: SavedFilterId,
        name: &str,
    ) -> Result<bool, DbError> {
        let renamed =
            sqlx::query("UPDATE saved_filter SET name = $3 WHERE id = $1 AND library_id = $2")
                .bind(filter.as_uuid())
                .bind(library.as_uuid())
                .bind(name)
                .execute(&self.0)
                .await
                .map_err(|err| match constraint_of(&err).as_deref() {
                    Some("saved_filter_name_unique_per_library") => DbError::SavedFilterNameTaken {
                        name: name.to_owned(),
                    },
                    _ => DbError::Query(err),
                })?;
        Ok(renamed.rows_affected() == 1)
    }

    /// Move a saved filter one place up or down its library's list. At either end nothing moves, and
    /// that is still `true`; `false` when the library holds no such filter.
    ///
    /// Under the library's row lock, and every position is written again from the list's order, so two
    /// filters that were made at once and share a position are told apart from here on.
    pub async fn move_filter(
        &self,
        library: LibraryId,
        filter: SavedFilterId,
        direction: FilterMove,
    ) -> Result<bool, DbError> {
        let mut tx = self.0.begin().await?;
        let locked: Option<i32> =
            sqlx::query_scalar("SELECT 1 FROM library WHERE id = $1 FOR UPDATE")
                .bind(library.as_uuid())
                .fetch_optional(&mut *tx)
                .await?;
        if locked.is_none() {
            return Ok(false);
        }
        let mut order: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM saved_filter WHERE library_id = $1 ORDER BY position, created_at, id",
        )
        .bind(library.as_uuid())
        .fetch_all(&mut *tx)
        .await?;
        let Some(at) = order.iter().position(|id| *id == filter.as_uuid()) else {
            return Ok(false);
        };
        let to = match direction {
            FilterMove::Up => at.checked_sub(1),
            FilterMove::Down => Some(at + 1).filter(|to| *to < order.len()),
        };
        let Some(to) = to else {
            return Ok(true);
        };
        order.swap(at, to);
        sqlx::query(
            "UPDATE saved_filter s SET position = o.n::integer \
             FROM unnest($1::uuid[]) WITH ORDINALITY AS o(id, n) \
             WHERE s.id = o.id AND s.library_id = $2",
        )
        .bind(&order)
        .bind(library.as_uuid())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Take one saved filter off its library's list. `false` when that library holds no such
    /// filter, including when the id belongs to another library's.
    pub async fn remove(&self, library: LibraryId, filter: SavedFilterId) -> Result<bool, DbError> {
        let removed = sqlx::query("DELETE FROM saved_filter WHERE id = $1 AND library_id = $2")
            .bind(filter.as_uuid())
            .bind(library.as_uuid())
            .execute(&self.0)
            .await?;
        Ok(removed.rows_affected() == 1)
    }
}
