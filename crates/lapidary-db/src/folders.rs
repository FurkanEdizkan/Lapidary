//! The category tree. Location, never identity.

use crate::DbError;
use lapidary_core::{FolderId, LibraryId};
use sqlx::PgPool;
use uuid::Uuid;

/// One node. The tree is returned flat and assembled by the caller — hundreds of rows at
/// corpus scale, and a nested JSON build in SQL is a second shape to keep in step with the
/// TypeScript one.
///
/// No `#[derive(sqlx::FromRow)]` here: `FolderId` carries no sqlx impls (`ids.rs`'s
/// `uuid_newtype!` derives only serde and ts-rs, deliberately, so that macro does not grow
/// a dependency for one caller). Every query below selects plain `uuid::Uuid` columns into
/// a tuple and maps them by hand, the same shape `PgParts::library_of` (`repo.rs`) uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRow {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub slug: String,
}

/// Matches `scan.rs`'s `MAX_DEPTH`. Real trees do not cycle, but a bound keeps a corrupt
/// `parent_id` from looping the walk forever.
const MAX_DEPTH: i32 = 16;

pub struct PgFolders(pub PgPool);

impl PgFolders {
    /// Insert-or-find, in that order. Two workers scanning concurrently genuinely race the
    /// same directory, so the constraint is what makes it safe rather than a prior SELECT
    /// that another worker can invalidate between statements.
    pub async fn get_or_create(
        &self,
        library: LibraryId,
        parent: Option<FolderId>,
        name: &str,
        slug: &str,
    ) -> Result<FolderId, DbError> {
        let id = FolderId::new();
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO folder (id, library_id, parent_id, name, slug) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(id.as_uuid())
        .bind(library.as_uuid())
        .bind(parent.map(|p| p.as_uuid()))
        .bind(name)
        .bind(slug)
        .fetch_optional(&self.0)
        .await?;

        if let Some(uuid) = inserted {
            return Ok(FolderId::from_uuid(uuid));
        }

        // `IS NOT DISTINCT FROM`, not `=`: parent_id is NULL at the library root, and `=`
        // is never true against NULL, so the plain form would find nothing and the caller
        // would loop forever trying to create a row that already exists.
        let found: Uuid = sqlx::query_scalar(
            "SELECT id FROM folder WHERE library_id = $1 \
             AND parent_id IS NOT DISTINCT FROM $2 AND name = $3",
        )
        .bind(library.as_uuid())
        .bind(parent.map(|p| p.as_uuid()))
        .bind(name)
        .fetch_one(&self.0)
        .await?;
        Ok(FolderId::from_uuid(found))
    }

    pub async fn tree(&self, library: LibraryId) -> Result<Vec<FolderRow>, DbError> {
        let rows: Vec<(Uuid, Option<Uuid>, String, String)> = sqlx::query_as(
            "SELECT id, parent_id, name, slug FROM folder \
             WHERE library_id = $1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, parent_id, name, slug)| FolderRow {
                id: FolderId::from_uuid(id),
                parent_id: parent_id.map(FolderId::from_uuid),
                name,
                slug,
            })
            .collect())
    }

    /// Would moving `folder` under `new_parent` put it inside itself? Walks up from the
    /// proposed parent looking for the folder being moved.
    pub async fn would_cycle(
        &self,
        folder: FolderId,
        new_parent: FolderId,
    ) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(
            "WITH RECURSIVE up AS ( \
             SELECT id, parent_id, 1 AS depth FROM folder WHERE id = $1 \
             UNION ALL \
             SELECT f.id, f.parent_id, up.depth + 1 FROM folder f \
             JOIN up ON f.id = up.parent_id WHERE up.depth < $3) \
             SELECT coalesce(bool_or(id = $2), false) FROM up",
        )
        .bind(new_parent.as_uuid())
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .fetch_one(&self.0)
        .await?)
    }

    /// The `/`-joined slugs from the library root down to this folder — the directory it
    /// lives at inside `libraries/<lib>/`.
    pub async fn slug_path(&self, folder: FolderId) -> Result<String, DbError> {
        Ok(sqlx::query_scalar(
            "WITH RECURSIVE up AS ( \
             SELECT id, parent_id, slug, 1 AS depth FROM folder WHERE id = $1 \
             UNION ALL \
             SELECT f.id, f.parent_id, f.slug, up.depth + 1 FROM folder f \
             JOIN up ON f.id = up.parent_id WHERE up.depth < $2) \
             SELECT string_agg(slug, '/' ORDER BY depth DESC) FROM up",
        )
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .fetch_one(&self.0)
        .await?)
    }

    pub async fn library_of(&self, folder: FolderId) -> Result<Option<LibraryId>, DbError> {
        let found: Option<Uuid> = sqlx::query_scalar(
            "SELECT library_id FROM folder WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(folder.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        Ok(found.map(LibraryId::from_uuid))
    }

    pub async fn rename(&self, folder: FolderId, name: &str, slug: &str) -> Result<bool, DbError> {
        let done = sqlx::query("UPDATE folder SET name = $2, slug = $3 WHERE id = $1")
            .bind(folder.as_uuid())
            .bind(name)
            .bind(slug)
            .execute(&self.0)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn reparent(
        &self,
        folder: FolderId,
        parent: Option<FolderId>,
    ) -> Result<bool, DbError> {
        let done = sqlx::query("UPDATE folder SET parent_id = $2 WHERE id = $1")
            .bind(folder.as_uuid())
            .bind(parent.map(|p| p.as_uuid()))
            .execute(&self.0)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Soft-delete a folder, every descendant, and every part in any of them. Returns
    /// (folders hidden, parts hidden) so the confirmation can name the count it warned
    /// about and the caller can check it matched.
    pub async fn soft_delete_subtree(&self, folder: FolderId) -> Result<(u64, u64), DbError> {
        let mut tx = self.0.begin().await?;

        let folders = sqlx::query(
            "WITH RECURSIVE down AS ( \
             SELECT id, 1 AS depth FROM folder WHERE id = $1 \
             UNION ALL \
             SELECT f.id, down.depth + 1 FROM folder f \
             JOIN down ON f.parent_id = down.id WHERE down.depth < $2) \
             UPDATE folder SET deleted_at = now() \
             WHERE id IN (SELECT id FROM down) AND deleted_at IS NULL",
        )
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let parts = sqlx::query(
            "WITH RECURSIVE down AS ( \
             SELECT id, 1 AS depth FROM folder WHERE id = $1 \
             UNION ALL \
             SELECT f.id, down.depth + 1 FROM folder f \
             JOIN down ON f.parent_id = down.id WHERE down.depth < $2) \
             UPDATE part SET deleted_at = now() \
             WHERE folder_id IN (SELECT id FROM down) AND deleted_at IS NULL",
        )
        .bind(folder.as_uuid())
        .bind(MAX_DEPTH)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        tx.commit().await?;
        Ok((folders, parts))
    }
}
