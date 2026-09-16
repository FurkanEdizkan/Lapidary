//! Sharing (S2a): the categories this installation offers the people it is paired with.
//!
//! A share is a category and everything under it, so the catalogue is the folder tree walked at read time
//! (`folders.rs`'s descent), never a list kept beside it. Every query that follows a share also requires its
//! category to be live: deleting a shared category stops it being offered.

use crate::DbError;
use crate::repo::detail_stamp;
use jiff::Timestamp;
use lapidary_core::{DeviceId, FolderId, LibraryId, PartId, ShareId};
use sqlx::PgPool;

/// A category this installation shares, as its own sharing page lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareRow {
    pub id: ShareId,
    pub folder: FolderId,
    /// The category's name.
    pub name: String,
    pub created_at: Timestamp,
}

/// A share as another installation sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferedShare {
    pub id: ShareId,
    pub name: String,
    pub part_count: i64,
    /// Changes whenever the catalogue does: its part count and newest change, so a puller re-reads only a
    /// share whose digest moved. The newest change counts a part's revisions as well as the part: recording a
    /// revision leaves `part.updated_at` alone, and a digest that missed it would leave pullers with a stale hash.
    pub digest: String,
}

/// One part of a share's catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogueRow {
    pub part: PartId,
    pub source_path: String,
    pub name: String,
    pub part_number: Option<String>,
    pub tags: Vec<String>,
    /// Every licence recorded against the part's sources, distinct.
    pub licences: Vec<String>,
    /// The current revision's source file. `None` only for a part whose file row is missing.
    pub blake3: Option<String>,
    pub size_bytes: Option<i64>,
    pub format: Option<String>,
    pub thumbnail: bool,
}

/// What sharing a category would offer, counted before anybody confirms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LicenceCounts {
    pub parts: i64,
    /// Parts with no licence recorded against any source.
    pub unrecorded: i64,
    /// Parts with at least one licence that says non-commercial.
    pub non_commercial: i64,
}

/// A licence that says non-commercial, as PostgreSQL's case-insensitive `~*` reads it: `NC` as a token of its
/// own (`CC BY-NC 4.0`, `CC-BY-NC-SA 4.0`), or the words spelled out. Never `nc` inside a word — "Licenced
/// for incidental use" is not a non-commercial licence, and a warning that fires on it teaches people to
/// ignore it.
const NON_COMMERCIAL: &str = "(^|[^[:alnum:]])nc([^[:alnum:]]|$)|non[- ]?commercial";

/// The share's category and every category under it, as `down`, for a query whose `$1` is the share. Empty
/// when the share or its category is not live, which is how every read below refuses a withdrawn share.
macro_rules! share_subtree {
    () => {
        "WITH RECURSIVE down AS ( \
         SELECT f.id FROM share s JOIN folder f ON f.id = s.folder_id \
         WHERE s.id = $1 AND s.removed_at IS NULL AND f.deleted_at IS NULL \
         UNION ALL \
         SELECT f.id FROM folder f JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen"
    };
}
type ShareTuple = (uuid::Uuid, uuid::Uuid, String, i64);

type CatalogueTuple = (
    uuid::Uuid,
    String,
    String,
    Option<String>,
    Vec<String>,
    Vec<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    bool,
);

fn share_row((id, folder, name, created_us): ShareTuple) -> Result<ShareRow, DbError> {
    Ok(ShareRow {
        id: ShareId::from_uuid(id),
        folder: FolderId::from_uuid(folder),
        name,
        created_at: detail_stamp("share.created_at", created_us)?,
    })
}

pub struct PgShares(pub PgPool);

impl PgShares {
    /// Share a live category of `library`. The live share it already has, when it has one. `None` when the
    /// category is not a live one in that library.
    pub async fn create(
        &self,
        library: LibraryId,
        folder: FolderId,
    ) -> Result<Option<ShareRow>, DbError> {
        sqlx::query(
            "INSERT INTO share (id, library_id, folder_id) \
             SELECT $1, f.library_id, f.id FROM folder f \
             WHERE f.id = $2 AND f.library_id = $3 AND f.deleted_at IS NULL \
             ON CONFLICT (folder_id) WHERE removed_at IS NULL DO NOTHING",
        )
        .bind(ShareId::new().as_uuid())
        .bind(folder.as_uuid())
        .bind(library.as_uuid())
        .execute(&self.0)
        .await?;
        let row: Option<ShareTuple> = sqlx::query_as(
            "SELECT s.id, s.folder_id, f.name, (extract(epoch FROM s.created_at) * 1000000)::bigint \
             FROM share s JOIN folder f ON f.id = s.folder_id \
             WHERE s.folder_id = $1 AND s.library_id = $2 AND s.removed_at IS NULL AND f.deleted_at IS NULL",
        )
        .bind(folder.as_uuid())
        .bind(library.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        crate::sharing::tell_the_peer_role(&self.0).await?;
        row.map(share_row).transpose()
    }

    /// A library's live shares, by category name.
    pub async fn list(&self, library: LibraryId) -> Result<Vec<ShareRow>, DbError> {
        let rows: Vec<ShareTuple> = sqlx::query_as(
            "SELECT s.id, s.folder_id, f.name, (extract(epoch FROM s.created_at) * 1000000)::bigint \
             FROM share s JOIN folder f ON f.id = s.folder_id \
             WHERE s.library_id = $1 AND s.removed_at IS NULL AND f.deleted_at IS NULL \
             ORDER BY f.name, s.id",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        rows.into_iter().map(share_row).collect()
    }

    /// Stop sharing. `false` when the share was not live.
    pub async fn remove(&self, share: ShareId) -> Result<bool, DbError> {
        let result =
            sqlx::query("UPDATE share SET removed_at = now() WHERE id = $1 AND removed_at IS NULL")
                .bind(share.as_uuid())
                .execute(&self.0)
                .await?;
        crate::sharing::tell_the_peer_role(&self.0).await?;
        Ok(result.rows_affected() > 0)
    }

    /// Every live share, as the people paired with this installation see them.
    pub async fn offered(&self) -> Result<Vec<OfferedShare>, DbError> {
        let rows: Vec<(uuid::Uuid, String, i64, i64)> = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT s.id AS share, f.id FROM share s JOIN folder f ON f.id = s.folder_id \
             WHERE s.removed_at IS NULL AND f.deleted_at IS NULL \
             UNION ALL \
             SELECT d.share, f.id FROM folder f JOIN down d ON f.parent_id = d.id) \
             CYCLE id SET is_cycle USING seen \
             SELECT s.id, f.name, count(DISTINCT p.id), \
             coalesce((extract(epoch FROM greatest(max(p.updated_at), max(r.created_at))) * 1000000)::bigint, 0) \
             FROM share s JOIN folder f ON f.id = s.folder_id \
             LEFT JOIN down d ON d.share = s.id AND NOT d.is_cycle \
             LEFT JOIN part p ON p.folder_id = d.id AND p.deleted_at IS NULL AND p.library_id = s.library_id \
             LEFT JOIN revision r ON r.part_id = p.id \
             WHERE s.removed_at IS NULL AND f.deleted_at IS NULL \
             GROUP BY s.id, f.name, s.created_at ORDER BY s.created_at, s.id",
        )
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, part_count, newest_us)| OfferedShare {
                id: ShareId::from_uuid(id),
                name,
                part_count,
                digest: format!("{part_count}-{newest_us}"),
            })
            .collect())
    }

    /// Whether `device` may read `share`: paired and not removed, and the share and its category live.
    pub async fn access(&self, device: DeviceId, share: ShareId) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM peer pe, share s JOIN folder f ON f.id = s.folder_id \
             WHERE pe.device_id = $1 AND pe.removed_at IS NULL \
             AND s.id = $2 AND s.removed_at IS NULL AND f.deleted_at IS NULL)",
        )
        .bind(device.as_bytes().as_slice())
        .bind(share.as_uuid())
        .fetch_one(&self.0)
        .await?)
    }

    /// A page of the share's catalogue: the live parts in its category and everything under it, by
    /// `source_path`, after `after`.
    pub async fn catalogue(
        &self,
        share: ShareId,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<CatalogueRow>, DbError> {
        let rows: Vec<CatalogueTuple> = sqlx::query_as(concat!(
            share_subtree!(),
            " SELECT p.id, p.source_path, p.name, p.part_number, p.tags, \
             coalesce((SELECT array_agg(DISTINCT ps.license ORDER BY ps.license) FROM part_source ps \
                       WHERE ps.part_id = p.id AND nullif(btrim(ps.license), '') IS NOT NULL), '{}'), \
             src.blake3, src.size_bytes, src.format, thumb.id IS NOT NULL \
             FROM part p JOIN share s ON s.id = $1 \
             JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             LEFT JOIN LATERAL (SELECT f.blake3, f.size_bytes, f.format FROM file f \
                                WHERE f.revision_id = r.id AND f.role = 'source' \
                                ORDER BY f.created_at DESC, f.id DESC LIMIT 1) src ON true \
             LEFT JOIN LATERAL (SELECT id FROM derivative WHERE revision_id = r.id AND kind = 'thumbnail' \
                                AND thumb_bytes IS NOT NULL ORDER BY created_at DESC, id DESC LIMIT 1) thumb ON true \
             WHERE p.library_id = s.library_id AND p.deleted_at IS NULL AND p.source_path IS NOT NULL \
             AND p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle) \
             AND ($2::text IS NULL OR p.source_path > $2) \
             ORDER BY p.source_path LIMIT $3"
        ))
        .bind(share.as_uuid())
        .bind(after)
        .bind(limit)
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    part,
                    source_path,
                    name,
                    part_number,
                    tags,
                    licences,
                    blake3,
                    size_bytes,
                    format,
                    thumbnail,
                )| {
                    CatalogueRow {
                        part: PartId::from_uuid(part),
                        source_path,
                        name,
                        part_number,
                        tags,
                        licences,
                        blake3,
                        size_bytes,
                        format,
                        thumbnail,
                    }
                },
            )
            .collect())
    }

    /// A part's thumbnail, only when the part is inside the share. Content addressing is not authorization,
    /// and neither is a part id.
    pub async fn thumbnail(
        &self,
        share: ShareId,
        part: PartId,
    ) -> Result<Option<Vec<u8>>, DbError> {
        Ok(sqlx::query_scalar(concat!(
            share_subtree!(),
            " SELECT thumb.thumb_bytes FROM part p JOIN share s ON s.id = $1 \
             JOIN LATERAL (SELECT id FROM revision WHERE part_id = p.id ORDER BY created_at DESC, id DESC LIMIT 1) r ON true \
             JOIN LATERAL (SELECT thumb_bytes FROM derivative WHERE revision_id = r.id AND kind = 'thumbnail' \
                           AND thumb_bytes IS NOT NULL ORDER BY created_at DESC, id DESC LIMIT 1) thumb ON true \
             WHERE p.id = $2 AND p.library_id = s.library_id AND p.deleted_at IS NULL \
             AND p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)"
        ))
        .bind(share.as_uuid())
        .bind(part.as_uuid())
        .fetch_optional(&self.0)
        .await?)
    }

    /// The licence warning for sharing `folder`, before anybody confirms it.
    pub async fn licences(&self, folder: FolderId) -> Result<LicenceCounts, DbError> {
        let (parts, unrecorded, non_commercial): (i64, i64, i64) = sqlx::query_as(
            "WITH RECURSIVE down AS ( \
             SELECT id FROM folder WHERE id = $1 \
             UNION ALL \
             SELECT f.id FROM folder f JOIN down ON f.parent_id = down.id) CYCLE id SET is_cycle USING seen, \
             offered AS (SELECT p.id FROM part p JOIN folder root ON root.id = $1 \
                         WHERE p.deleted_at IS NULL AND p.library_id = root.library_id \
                         AND p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle)) \
             SELECT count(*), \
             count(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM part_source ps WHERE ps.part_id = offered.id \
                                                AND nullif(btrim(ps.license), '') IS NOT NULL)), \
             count(*) FILTER (WHERE EXISTS (SELECT 1 FROM part_source ps WHERE ps.part_id = offered.id \
                                            AND ps.license ~* $2)) \
             FROM offered",
        )
        .bind(folder.as_uuid())
        .bind(NON_COMMERCIAL)
        .fetch_one(&self.0)
        .await?;
        Ok(LicenceCounts {
            parts,
            unrecorded,
            non_commercial,
        })
    }
}
