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
    /// Whether fetching its files needs its owner's grant.
    pub asks_first: bool,
}

/// Where somebody stands with a share's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    /// The share does not ask first.
    Open,
    /// It asks first, and they have not asked.
    NotAsked,
    Asked,
    Granted,
    Denied,
    /// Not shared with them: they are not paired, or were removed, or the share or its category is gone.
    NotShared,
}

impl Grant {
    pub fn as_str(self) -> &'static str {
        match self {
            Grant::Open => "open",
            Grant::NotAsked => "notAsked",
            Grant::Asked => "asked",
            Grant::Granted => "granted",
            Grant::Denied => "denied",
            Grant::NotShared => "notShared",
        }
    }

    /// Whether files may be fetched.
    pub fn allows_files(self) -> bool {
        matches!(self, Grant::Open | Grant::Granted)
    }

    fn from_row(mode: &str, state: Option<&str>) -> Self {
        match (mode, state) {
            ("open", _) => Grant::Open,
            (_, Some("granted")) => Grant::Granted,
            (_, Some("denied")) => Grant::Denied,
            (_, Some(_)) => Grant::Asked,
            (_, None) => Grant::NotAsked,
        }
    }
}

/// Somebody who asked for a share's files, as its owner's page lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct GrantRow {
    pub share: ShareId,
    pub share_name: String,
    pub device: DeviceId,
    /// What they call themselves, as their last hello said.
    pub name: Option<String>,
    pub state: Grant,
    pub asked_at: Timestamp,
}

/// One person a share goes to, as its owner's page lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberRow {
    pub device: DeviceId,
    /// What they call themselves, as their last hello said.
    pub name: Option<String>,
    pub address: String,
    /// Answered on the database's clock, as `PgSharing::peers` answers it.
    pub online: bool,
    pub added_at: Timestamp,
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
    /// Whether fetching its files needs its owner's grant.
    pub asks_first: bool,
    /// Whether it reaches whoever is paired, people paired later included, because nobody has said who it goes
    /// to. A folder whose owner has said, and named nobody, reaches nobody — which reads the same in a list of
    /// members and is not the same thing.
    pub reaches_everyone: bool,
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

/// Where a shared file's bytes are, as the blob route reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobLocation {
    /// The file's path under the store, or `None` while it is still at the old content-addressed path.
    pub storage_path: Option<String>,
    /// Passed to the reader as the file row records it, as the bundle export does.
    pub zstd_level: Option<i16>,
    /// The uncompressed length, which is what is sent.
    pub size_bytes: i64,
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

/// Whether the share aliased `s` reaches the device bound at `$device`: everyone it is paired with, for a share
/// made before members existed and never given a list, or a live row in its list.
///
/// One definition because four reads ask it — the list a peer is offered, whether it may read a share, where it
/// stands with the files, and which requests its owner still sees — and a membership check missing from any one
/// of them is a folder reaching somebody it was taken off.
macro_rules! reaches {
    ($device:literal) => {
        concat!(
            "(s.audience = 'everyone' OR EXISTS (SELECT 1 FROM share_member m \
              WHERE m.share_id = s.id AND m.device_id = ",
            $device,
            " AND m.removed_at IS NULL))"
        )
    };
}

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
type ShareTuple = (uuid::Uuid, uuid::Uuid, String, i64, String);

type GrantTuple = (uuid::Uuid, String, Vec<u8>, Option<String>, String, i64);

type MemberTuple = (Vec<u8>, Option<String>, String, bool, i64);

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

fn share_row((id, folder, name, created_us, mode): ShareTuple) -> Result<ShareRow, DbError> {
    Ok(ShareRow {
        id: ShareId::from_uuid(id),
        folder: FolderId::from_uuid(folder),
        name,
        created_at: detail_stamp("share.created_at", created_us)?,
        asks_first: mode == "ask",
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
            "SELECT s.id, s.folder_id, f.name, (extract(epoch FROM s.created_at) * 1000000)::bigint, s.mode \
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
            "SELECT s.id, s.folder_id, f.name, (extract(epoch FROM s.created_at) * 1000000)::bigint, s.mode \
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

    /// Who a share goes to, by name: the people its list names, whether or not it is using that list yet.
    pub async fn members(&self, share: ShareId) -> Result<Vec<MemberRow>, DbError> {
        // Online on the database's clock, the same predicate and the same window `PgSharing::peers` answers with.
        let rows: Vec<MemberTuple> = sqlx::query_as(
            "SELECT m.device_id, pe.name, pe.address, \
             (pe.last_error IS NULL AND pe.last_seen_at > now() - make_interval(secs => $2::float8)) IS TRUE, \
             (extract(epoch FROM m.added_at) * 1000000)::bigint \
             FROM share_member m JOIN peer pe ON pe.device_id = m.device_id \
             WHERE m.share_id = $1 AND m.removed_at IS NULL AND pe.removed_at IS NULL \
             ORDER BY pe.name NULLS LAST, m.device_id",
        )
        .bind(share.as_uuid())
        .bind(crate::sharing::ONLINE_WITHIN_SECS)
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(|(device, name, address, online, added_us)| {
                let length = device.len();
                Ok(MemberRow {
                    device: <[u8; 32]>::try_from(device)
                        .map(DeviceId::from_bytes)
                        .map_err(|_| DbError::CorruptDeviceId {
                            column: "share_member.device_id",
                            length,
                        })?,
                    name,
                    address,
                    online,
                    added_at: detail_stamp("share_member.added_at", added_us)?,
                })
            })
            .collect()
    }

    /// Say who a live share goes to. `false` when the share is not live.
    ///
    /// The list replaces whatever was there: people not named are taken off softly, so the row remembers that they
    /// were once in it and the parts they pulled stay theirs. Saying who it goes to is also what moves a share off
    /// "everyone paired" for good — including to nobody, when the list is empty.
    pub async fn set_members(&self, share: ShareId, members: &[DeviceId]) -> Result<bool, DbError> {
        let devices: Vec<Vec<u8>> = members
            .iter()
            .map(|device| device.as_bytes().to_vec())
            .collect();
        let mut tx = self.0.begin().await?;
        let live = sqlx::query(
            "UPDATE share SET audience = 'members' WHERE id = $1 AND removed_at IS NULL",
        )
        .bind(share.as_uuid())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if live == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        // Only people this installation is paired with: an id nobody paired with names no machine this one can
        // reach, and `share_member.device_id` references `peer` besides.
        sqlx::query(
            "INSERT INTO share_member (share_id, device_id) \
             SELECT $1, pe.device_id FROM peer pe \
             WHERE pe.device_id = ANY($2) AND pe.removed_at IS NULL \
             ON CONFLICT (share_id, device_id) DO UPDATE SET removed_at = NULL",
        )
        .bind(share.as_uuid())
        .bind(&devices)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE share_member SET removed_at = now() \
             WHERE share_id = $1 AND removed_at IS NULL AND NOT (device_id = ANY($2))",
        )
        .bind(share.as_uuid())
        .bind(&devices)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        crate::sharing::tell_the_peer_role(&self.0).await?;
        Ok(true)
    }

    /// Ask first, or stop asking. `false` when the share is not live. Nothing already granted or denied changes.
    pub async fn set_asks_first(&self, share: ShareId, asks_first: bool) -> Result<bool, DbError> {
        let result = sqlx::query("UPDATE share SET mode = $2 WHERE id = $1 AND removed_at IS NULL")
            .bind(share.as_uuid())
            .bind(if asks_first { "ask" } else { "open" })
            .execute(&self.0)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Where `device` stands with `share`'s files.
    pub async fn grant(&self, device: DeviceId, share: ShareId) -> Result<Grant, DbError> {
        let row: Option<(String, Option<String>)> = sqlx::query_as(concat!(
            "SELECT s.mode, g.state FROM peer pe, share s JOIN folder f ON f.id = s.folder_id \
             LEFT JOIN share_grant g ON g.share_id = s.id AND g.device_id = $1 \
             WHERE pe.device_id = $1 AND pe.removed_at IS NULL \
             AND s.id = $2 AND s.removed_at IS NULL AND f.deleted_at IS NULL AND ",
            reaches!("$1")
        ))
        .bind(device.as_bytes().as_slice())
        .bind(share.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        Ok(row.map_or(Grant::NotShared, |(mode, state)| {
            Grant::from_row(&mode, state.as_deref())
        }))
    }

    /// `device` asks for `share`'s files. Recorded once; asking again changes nothing, a denial included. `None` when the
    /// share is not shared with them.
    pub async fn ask(&self, device: DeviceId, share: ShareId) -> Result<Option<Grant>, DbError> {
        match self.grant(device, share).await? {
            Grant::NotShared => return Ok(None),
            Grant::NotAsked => {}
            standing => return Ok(Some(standing)),
        }
        sqlx::query(
            "INSERT INTO share_grant (share_id, device_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(share.as_uuid())
        .bind(device.as_bytes().as_slice())
        .execute(&self.0)
        .await?;
        let standing = self.grant(device, share).await?;
        Ok((standing != Grant::NotShared).then_some(standing))
    }

    /// Everybody who asked for a live share's files and is still offered it, newest first, whatever was decided.
    ///
    /// A person taken off a folder's list drops out: their ask is about a folder that no longer reaches them, and an
    /// owner asked to decide it would be deciding nothing.
    pub async fn requests(&self) -> Result<Vec<GrantRow>, DbError> {
        let rows: Vec<GrantTuple> = sqlx::query_as(concat!(
            "SELECT s.id, f.name, g.device_id, pe.name, g.state, (extract(epoch FROM g.asked_at) * 1000000)::bigint \
             FROM share_grant g JOIN share s ON s.id = g.share_id JOIN folder f ON f.id = s.folder_id \
             JOIN peer pe ON pe.device_id = g.device_id \
             WHERE s.removed_at IS NULL AND f.deleted_at IS NULL AND pe.removed_at IS NULL AND ",
            reaches!("g.device_id"),
            " ORDER BY g.asked_at DESC, s.id, g.device_id"
        ))
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(|(share, share_name, device, name, state, asked_us)| {
                let length = device.len();
                Ok(GrantRow {
                    share: ShareId::from_uuid(share),
                    share_name,
                    device: <[u8; 32]>::try_from(device)
                        .map(DeviceId::from_bytes)
                        .map_err(|_| DbError::CorruptDeviceId {
                            column: "share_grant.device_id",
                            length,
                        })?,
                    name,
                    state: Grant::from_row("ask", Some(&state)),
                    asked_at: detail_stamp("share_grant.asked_at", asked_us)?,
                })
            })
            .collect()
    }

    /// Grant or deny a request. `false` when there is no such request on a live share.
    pub async fn decide(
        &self,
        share: ShareId,
        device: DeviceId,
        granted: bool,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE share_grant g SET state = $3, decided_at = now() FROM share s \
             WHERE g.share_id = $1 AND g.device_id = $2 AND s.id = g.share_id AND s.removed_at IS NULL",
        )
        .bind(share.as_uuid())
        .bind(device.as_bytes().as_slice())
        .bind(if granted { "granted" } else { "denied" })
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Every live share of this installation, whoever it goes to: the owner's own list.
    pub async fn offered(&self) -> Result<Vec<OfferedShare>, DbError> {
        self.offer_rows(None).await
    }

    /// The live shares `device` is offered: the ones with no member list, and the ones its list names.
    pub async fn offered_to(&self, device: DeviceId) -> Result<Vec<OfferedShare>, DbError> {
        self.offer_rows(Some(device)).await
    }

    async fn offer_rows(&self, device: Option<DeviceId>) -> Result<Vec<OfferedShare>, DbError> {
        let rows: Vec<(uuid::Uuid, String, i64, i64, String, bool)> = sqlx::query_as(concat!(
            "WITH RECURSIVE down AS ( \
             SELECT s.id AS share, f.id FROM share s JOIN folder f ON f.id = s.folder_id \
             WHERE s.removed_at IS NULL AND f.deleted_at IS NULL \
             UNION ALL \
             SELECT d.share, f.id FROM folder f JOIN down d ON f.parent_id = d.id) \
             CYCLE id SET is_cycle USING seen \
             SELECT s.id, f.name, count(DISTINCT p.id), \
             coalesce((extract(epoch FROM greatest(max(p.updated_at), max(r.created_at))) * 1000000)::bigint, 0), s.mode, \
             s.audience = 'everyone' \
             FROM share s JOIN folder f ON f.id = s.folder_id \
             LEFT JOIN down d ON d.share = s.id AND NOT d.is_cycle \
             LEFT JOIN part p ON p.folder_id = d.id AND p.deleted_at IS NULL AND p.library_id = s.library_id \
             LEFT JOIN revision r ON r.part_id = p.id \
             WHERE s.removed_at IS NULL AND f.deleted_at IS NULL \
             AND ($1::bytea IS NULL OR ",
            reaches!("$1"),
            ") GROUP BY s.id, f.name, s.created_at, s.mode ORDER BY s.created_at, s.id"
        ))
        .bind(device.map(|device| device.as_bytes().to_vec()))
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, name, part_count, newest_us, mode, reaches_everyone)| OfferedShare {
                    id: ShareId::from_uuid(id),
                    name,
                    part_count,
                    digest: format!("{part_count}-{newest_us}"),
                    asks_first: mode == "ask",
                    reaches_everyone,
                },
            )
            .collect())
    }

    /// Whether `device` may read `share`: paired and not removed, the share and its category live, and the
    /// share reaching them.
    pub async fn access(&self, device: DeviceId, share: ShareId) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(concat!(
            "SELECT EXISTS (SELECT 1 FROM peer pe, share s JOIN folder f ON f.id = s.folder_id \
             WHERE pe.device_id = $1 AND pe.removed_at IS NULL \
             AND s.id = $2 AND s.removed_at IS NULL AND f.deleted_at IS NULL AND ",
            reaches!("$1"),
            ")"
        ))
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

    /// Where a file is, only when it is the source file of a revision of a live part inside the share. The walk and
    /// the filters are the catalogue's own, so a file is reachable exactly when its part is offered: content
    /// addressing is not authorization.
    pub async fn blob(
        &self,
        share: ShareId,
        blake3: &str,
    ) -> Result<Option<BlobLocation>, DbError> {
        // Any revision's file, not only the current one: a pull that began before the sharer recorded a revision
        // finishes, and the next pull brings the new file.
        let row: Option<(Option<String>, Option<i16>, i64)> = sqlx::query_as(concat!(
            share_subtree!(),
            " SELECT f.storage_path, f.zstd_level, f.size_bytes FROM part p JOIN share s ON s.id = $1 \
             JOIN revision r ON r.part_id = p.id \
             JOIN file f ON f.revision_id = r.id AND f.role = 'source' \
             WHERE f.blake3 = $2 AND p.library_id = s.library_id AND p.deleted_at IS NULL \
             AND p.folder_id IN (SELECT id FROM down WHERE NOT is_cycle) \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1"
        ))
        .bind(share.as_uuid())
        .bind(blake3)
        .fetch_optional(&self.0)
        .await?;
        Ok(
            row.map(|(storage_path, zstd_level, size_bytes)| BlobLocation {
                storage_path,
                zstd_level,
                size_bytes,
            }),
        )
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
