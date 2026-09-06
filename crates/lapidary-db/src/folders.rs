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
    /// Live parts in this folder **and every folder under it**. Subtree-inclusive because
    /// a delete cascades through subcategories, so a count that stopped at the folder
    /// itself would understate what the confirmation is about to hide. Counts only
    /// `deleted_at IS NULL` rows, for the same reason: the number beside a category has to
    /// be the number of cards the grid shows for it, or the dialog contradicts the screen
    /// next to it.
    pub part_count: i64,
}

/// Matches `scan.rs`'s `MAX_DEPTH`. Real trees do not cycle, but a bound keeps a corrupt
/// `parent_id` from looping the walk forever.
pub(crate) const MAX_DEPTH: i32 = 16;

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

    /// Create a category the user asked for, refusing a name a sibling already holds.
    ///
    /// Not `get_or_create`: that one exists for the scan, which races itself over
    /// directories it did not invent and wants the row either way. A person typing a name
    /// into a form is making a claim about a category that does not exist yet, and handing
    /// them back somebody else's folder as if they had made it is the quieter of the two
    /// wrong answers. So the collision is an error here and a no-op there, and both lean
    /// on the same constraint.
    pub async fn create(
        &self,
        library: LibraryId,
        parent: Option<FolderId>,
        name: &str,
        slug: &str,
    ) -> Result<FolderId, DbError> {
        let id = FolderId::new();
        sqlx::query(
            "INSERT INTO folder (id, library_id, parent_id, name, slug) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(id.as_uuid())
        .bind(library.as_uuid())
        .bind(parent.map(|p| p.as_uuid()))
        .bind(name)
        .bind(slug)
        .execute(&self.0)
        .await
        .map_err(|err| collision(name, slug, err))?;
        Ok(id)
    }

    /// The whole tree of one library, flat, each node carrying its subtree part count.
    ///
    /// One recursive CTE for every node's count, not one query per node: the sidebar reads
    /// this once and the delete confirmation reads the count off the node it already has,
    /// so a per-folder count query would be N round trips for a panel that is one request.
    /// `down` pairs every folder with each of its descendants (itself included, which is
    /// what makes the count subtree-*inclusive*), and the aggregate below counts live parts
    /// per root.
    ///
    /// The descent is not filtered on `deleted_at`: a soft-deleted subfolder's parts were
    /// soft-deleted with it by `soft_delete_subtree`, so they fall out at the part filter
    /// anyway, and filtering the walk as well would only add a way for the two rules to
    /// disagree. The roots are filtered, because a deleted folder is not a row this returns.
    pub async fn tree(&self, library: LibraryId) -> Result<Vec<FolderRow>, DbError> {
        let rows: Vec<(Uuid, Option<Uuid>, String, String, i64)> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id AS root, id, 1 AS depth FROM folder \
             WHERE library_id = $1 AND deleted_at IS NULL \
             UNION ALL \
             SELECT d.root, f.id, d.depth + 1 FROM folder f \
             JOIN down d ON f.parent_id = d.id WHERE d.depth < $2), \
             counts AS ( \
             SELECT d.root, count(p.id) AS n FROM down d \
             JOIN part p ON p.folder_id = d.id AND p.deleted_at IS NULL \
             GROUP BY d.root) \
             SELECT f.id, f.parent_id, f.name, f.slug, coalesce(c.n, 0) \
             FROM folder f LEFT JOIN counts c ON c.root = f.id \
             WHERE f.library_id = $1 AND f.deleted_at IS NULL ORDER BY f.name",
        )
        .bind(library.as_uuid())
        .bind(MAX_DEPTH)
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, parent_id, name, slug, part_count)| FolderRow {
                id: FolderId::from_uuid(id),
                parent_id: parent_id.map(FolderId::from_uuid),
                name,
                slug,
                part_count,
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

    /// Rename in place. The two sibling-uniqueness constraints come back as
    /// [`DbError::FolderNameTaken`] and [`DbError::FolderSlugTaken`] rather than as a raw
    /// database error — see [`collision`].
    pub async fn rename(&self, folder: FolderId, name: &str, slug: &str) -> Result<bool, DbError> {
        let done = sqlx::query("UPDATE folder SET name = $2, slug = $3 WHERE id = $1")
            .bind(folder.as_uuid())
            .bind(name)
            .bind(slug)
            .execute(&self.0)
            .await
            .map_err(|err| collision(name, slug, err))?;
        Ok(done.rows_affected() == 1)
    }

    /// Move `folder` under `parent` (or to the library root if `None`), refusing a move
    /// that would put it inside its own subtree.
    ///
    /// The refusal has to be atomic with the write, not a `would_cycle` call the caller
    /// makes first: check-then-act across two round trips is not atomic. Two concurrent
    /// `reparent` calls can each read `would_cycle == false` and then both commit — A
    /// under B and B under A, each individually "checked" and each wrong the instant the
    /// other lands. So this opens its own transaction, takes a `pg_advisory_xact_lock`
    /// keyed on the folder's `library_id` (serializing concurrent moves within that
    /// library — a lock scoped to the whole database would block an unrelated library's
    /// scan for no reason), re-runs the ancestry check inside that lock, and only then
    /// writes. A self-parent (`reparent(x, Some(x))`) is refused by this same path: the
    /// ancestry walk's base row is the proposed parent itself, so `folder == new_parent`
    /// matches on the first row without needing a special case.
    pub async fn reparent(
        &self,
        folder: FolderId,
        parent: Option<FolderId>,
    ) -> Result<bool, DbError> {
        let mut tx = self.0.begin().await?;

        // Not `library_of`: that filters `deleted_at IS NULL`, and a soft-deleted folder
        // must still resolve here so the UPDATE below runs exactly as it always has for
        // one (unfiltered) — this SELECT exists only to name a lock key, not to gate
        // the move.
        //
        // The name and slug come along because the UPDATE at the end can violate the same
        // two sibling-uniqueness constraints a rename can — moving `Terrain` under a parent
        // that already holds a `Terrain` is a collision reached by the other route — and
        // [`collision`] needs them to say which folder is in the way.
        let row: Option<(Uuid, String, String)> =
            sqlx::query_as("SELECT library_id, name, slug FROM folder WHERE id = $1")
                .bind(folder.as_uuid())
                .fetch_optional(&mut *tx)
                .await?;
        let Some((library, name, slug)) = row else {
            return Ok(false);
        };

        // One lock, keyed by library so two libraries' moves never contend. Held for the
        // rest of this transaction and released automatically at commit or rollback —
        // nothing to unlock by hand.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(library.to_string())
            .execute(&mut *tx)
            .await?;

        if let Some(new_parent) = parent {
            // Same query as `would_cycle`, run inside this transaction rather than
            // `&self.0`'s pool so it reads the state as of right now, under the lock just
            // taken — a stale read here is exactly the race this method exists to close.
            let would_cycle: bool = sqlx::query_scalar(
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
            .fetch_one(&mut *tx)
            .await?;
            if would_cycle {
                return Err(DbError::WouldCreateCycle { folder, new_parent });
            }
        }

        let done = sqlx::query("UPDATE folder SET parent_id = $2 WHERE id = $1")
            .bind(folder.as_uuid())
            .bind(parent.map(|p| p.as_uuid()))
            .execute(&mut *tx)
            .await
            .map_err(|err| collision(&name, &slug, err))?;

        tx.commit().await?;
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

/// Name the sibling that is in the way, instead of handing back a raw constraint failure.
///
/// Two constraints, two different things to tell the user, and `0008_folders.sql` says why
/// both exist: `folder_name_unique_per_parent` is the one a person can see — a sibling
/// already carries this name — while `folder_slug_unique_per_parent` catches the pair that
/// looks distinct on screen and is not on disk, `Rocks?` and `Rocks*` both wanting the
/// directory `Rocks-`. A message that said only "that name is taken" for the second would
/// be telling the user something they can check and find false.
///
/// Anything else passes through untouched: this reads the constraint name off the error the
/// same way `lapidary-ingest`'s `classify_write` does, and a violation of some other
/// constraint is not a collision this function knows how to describe.
fn collision(name: &str, slug: &str, err: sqlx::Error) -> DbError {
    let constraint = match &err {
        sqlx::Error::Database(db) => db.constraint(),
        _ => None,
    };
    match constraint {
        Some("folder_name_unique_per_parent") => DbError::FolderNameTaken {
            name: name.to_owned(),
        },
        Some("folder_slug_unique_per_parent") => DbError::FolderSlugTaken {
            name: name.to_owned(),
            slug: slug.to_owned(),
        },
        _ => DbError::Query(err),
    }
}
