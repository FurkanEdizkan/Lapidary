//! Saved filters: a name for a set of the grid's filters, kept per library.
//!
//! This crate stores the filters and does not read inside them. What a filter may hold is the
//! API's decision (`lapidary-api`'s `filters.rs`), made once, before anything is written.

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
}

pub struct PgSavedFilters(pub PgPool);

impl PgSavedFilters {
    /// A library's saved filters, by name. An id naming no library has none.
    pub async fn list(&self, library: LibraryId) -> Result<Vec<SavedFilterRow>, DbError> {
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT id, name, search::text FROM saved_filter WHERE library_id = $1 ORDER BY name, id",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, search)| SavedFilterRow {
                id: SavedFilterId::from_uuid(id),
                name,
                search,
            })
            .collect())
    }

    /// Save `search` under `name`. The constraints decide a taken name and an unknown library,
    /// rather than a read first that a concurrent write could still invalidate.
    pub async fn create(
        &self,
        library: LibraryId,
        name: &str,
        search: &serde_json::Value,
    ) -> Result<SavedFilterId, DbError> {
        let id = SavedFilterId::new();
        sqlx::query(
            "INSERT INTO saved_filter (id, library_id, name, search) VALUES ($1, $2, $3, $4::jsonb)",
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
