//! Sharing (S2b): other installations' shares, mirrored here by the peer role's hello round and read by the api.
//!
//! A cache of another machine's list, not this installation's data (`0039`'s comment says what that allows).
//! Every read for a page requires the sharer to be paired and not removed, so removing somebody hides what was
//! mirrored from them at once, without deleting it.

use crate::DbError;
use crate::repo::detail_stamp;
use crate::sharing::stored_device;
use jiff::Timestamp;
use lapidary_core::{DeviceId, PartId, PeerShareId, ShareId};
use sqlx::PgPool;

/// One share as another installation's list names it now.
#[derive(Debug, Clone, Copy)]
pub struct OfferedRemote<'a> {
    pub remote: ShareId,
    pub name: &'a str,
    pub part_count: i64,
    pub digest: &'a str,
}

/// A mirrored share whose catalogue must be read again, and the digest to record once it has been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleShare {
    pub id: PeerShareId,
    pub remote: ShareId,
    pub digest: String,
}

/// One part of a catalogue, as the hello round writes it.
#[derive(Debug, Clone, Copy)]
pub struct MirroredPartIn<'a> {
    pub source_path: &'a str,
    pub remote_part: PartId,
    pub name: &'a str,
    pub part_number: Option<&'a str>,
    pub tags: &'a [String],
    pub licences: &'a [String],
    pub blake3: Option<&'a str>,
    pub size_bytes: Option<i64>,
    pub format: Option<&'a str>,
    pub thumbnail: Option<&'a [u8]>,
}

/// One person on a mirrored folder's roster, as its owner published it.
#[derive(Debug, Clone, Copy)]
pub struct RemoteMember<'a> {
    pub device: DeviceId,
    pub name: Option<&'a str>,
    pub address: &'a str,
    pub may_fetch: bool,
}

/// Somebody a folder's owner has introduced, and this installation has not answered yet.
#[derive(Debug, Clone, PartialEq)]
pub struct IntroductionRow {
    pub share: PeerShareId,
    pub share_name: String,
    pub device: DeviceId,
    /// What they call themselves, as the folder's owner last heard it.
    pub name: Option<String>,
    pub address: String,
    /// Who published the roster they are on.
    pub introducer: DeviceId,
    pub introducer_name: Option<String>,
}

/// A mirrored share, as a page lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct MirroredShareRow {
    pub id: PeerShareId,
    pub device: DeviceId,
    /// What the sharer calls themselves, as their last hello said.
    pub sharer: Option<String>,
    pub name: String,
    pub part_count: i64,
    /// When a whole catalogue was last read. `None` until one has been.
    pub synced_at: Option<Timestamp>,
}

/// A mirrored part, as a page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct MirroredPartRow {
    pub source_path: String,
    pub remote_part: PartId,
    pub name: String,
    pub part_number: Option<String>,
    pub tags: Vec<String>,
    pub licences: Vec<String>,
    pub blake3: Option<String>,
    pub size_bytes: Option<i64>,
    pub format: Option<String>,
    pub thumbnail: bool,
}

/// A mirrored share's columns, in [`ShareTuple`]'s order, from `peer_share ps` joined to its sharer `pe`.
macro_rules! share_columns {
    () => {
        "ps.id, ps.device_id, pe.name, ps.name, ps.part_count, \
         (extract(epoch FROM ps.synced_at) * 1000000)::bigint \
         FROM peer_share ps JOIN peer pe ON pe.device_id = ps.device_id"
    };
}

type ShareTuple = (
    uuid::Uuid,
    Vec<u8>,
    Option<String>,
    String,
    i64,
    Option<i64>,
);

type IntroductionTuple = (
    uuid::Uuid,
    String,
    Vec<u8>,
    Option<String>,
    String,
    Vec<u8>,
    Option<String>,
);

type PartTuple = (
    String,
    uuid::Uuid,
    String,
    Option<String>,
    Vec<String>,
    Vec<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    bool,
);

fn share_row(
    (id, device, sharer, name, part_count, synced_us): ShareTuple,
) -> Result<MirroredShareRow, DbError> {
    let length = device.len();
    let device = <[u8; 32]>::try_from(device)
        .map(DeviceId::from_bytes)
        .map_err(|_| DbError::CorruptDeviceId {
            column: "peer_share.device_id",
            length,
        })?;
    Ok(MirroredShareRow {
        id: PeerShareId::from_uuid(id),
        device,
        sharer,
        name,
        part_count,
        synced_at: synced_us
            .map(|us| detail_stamp("peer_share.synced_at", us))
            .transpose()?,
    })
}

pub struct PgMirror(pub PgPool);

impl PgMirror {
    /// Take up the list `device` offers now: add the shares it newly offers, keep each one's name and part count
    /// current, and delete, with their parts, the shares it no longer offers. Answers the shares whose catalogue
    /// was never read or was read under another digest.
    pub async fn take_offer(
        &self,
        device: DeviceId,
        offered: &[OfferedRemote<'_>],
    ) -> Result<Vec<StaleShare>, DbError> {
        let ids: Vec<uuid::Uuid> = offered
            .iter()
            .map(|_| PeerShareId::new().as_uuid())
            .collect();
        let remotes: Vec<uuid::Uuid> = offered.iter().map(|o| o.remote.as_uuid()).collect();
        let names: Vec<&str> = offered.iter().map(|o| o.name).collect();
        let counts: Vec<i64> = offered.iter().map(|o| o.part_count).collect();
        let digests: Vec<&str> = offered.iter().map(|o| o.digest).collect();
        let device_bytes = device.as_bytes().as_slice();
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "INSERT INTO peer_share (id, device_id, remote_id, name, part_count) \
             SELECT o.id, $1, o.remote, o.name, o.part_count \
             FROM UNNEST($2::uuid[], $3::uuid[], $4::text[], $5::int8[]) AS o(id, remote, name, part_count) \
             ON CONFLICT (device_id, remote_id) DO UPDATE SET name = EXCLUDED.name, part_count = EXCLUDED.part_count",
        )
        .bind(device_bytes)
        .bind(&ids)
        .bind(&remotes)
        .bind(&names)
        .bind(&counts)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM peer_share WHERE device_id = $1 AND NOT (remote_id = ANY($2))")
            .bind(device_bytes)
            .bind(&remotes)
            .execute(&mut *tx)
            .await?;
        let stale: Vec<(uuid::Uuid, uuid::Uuid, String)> = sqlx::query_as(
            "SELECT ps.id, ps.remote_id, o.digest \
             FROM UNNEST($2::uuid[], $3::text[]) AS o(remote, digest) \
             JOIN peer_share ps ON ps.device_id = $1 AND ps.remote_id = o.remote \
             WHERE ps.synced_at IS NULL OR ps.digest IS DISTINCT FROM o.digest \
             ORDER BY ps.name, ps.id",
        )
        .bind(device_bytes)
        .bind(&remotes)
        .bind(&digests)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(stale
            .into_iter()
            .map(|(id, remote, digest)| StaleShare {
                id: PeerShareId::from_uuid(id),
                remote: ShareId::from_uuid(remote),
                digest,
            })
            .collect())
    }

    /// Replace a share's parts with a whole catalogue, read under `digest`, in one transaction: a read that fails
    /// part-way leaves the mirror as it was.
    pub async fn replace_catalogue(
        &self,
        share: PeerShareId,
        digest: &str,
        parts: &[MirroredPartIn<'_>],
    ) -> Result<(), DbError> {
        let mut tx = self.0.begin().await?;
        sqlx::query("DELETE FROM peer_share_part WHERE peer_share_id = $1")
            .bind(share.as_uuid())
            .execute(&mut *tx)
            .await?;
        // ponytail: one insert a part, inside the one transaction — about a second for a 1,000-part catalogue.
        // Tags and licences are arrays of differing lengths, which UNNEST cannot take side by side; a catalogue
        // in the tens of thousands wants them as jsonb and one insert.
        for part in parts {
            sqlx::query(
                "INSERT INTO peer_share_part (peer_share_id, source_path, remote_part, name, part_number, tags, \
                 licences, blake3, size_bytes, format, thumbnail) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(share.as_uuid())
            .bind(part.source_path)
            .bind(part.remote_part.as_uuid())
            .bind(part.name)
            .bind(part.part_number)
            .bind(part.tags)
            .bind(part.licences)
            .bind(part.blake3)
            .bind(part.size_bytes)
            .bind(part.format)
            .bind(part.thumbnail)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE peer_share SET digest = $2, synced_at = now() WHERE id = $1")
            .bind(share.as_uuid())
            .bind(digest)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Take up a mirrored folder's roster as its owner publishes it now: who is on it, where they are, and
    /// whether its owner lets them fetch its files. Somebody the roster no longer names leaves it — the folder's
    /// owner took them off, and this installation has no business offering an introduction to them any more.
    ///
    /// An answer already given is kept: a declined introduction stays declined, rather than being offered again
    /// on the next round.
    /// Keyed by the folder's owner and its own id for the folder, which is what the roster came back for, so
    /// the hello round needs no second read to turn that pair into the row it holds here.
    pub async fn take_roster(
        &self,
        device: DeviceId,
        remote: ShareId,
        members: &[RemoteMember<'_>],
    ) -> Result<bool, DbError> {
        let devices: Vec<Vec<u8>> = members
            .iter()
            .map(|member| member.device.as_bytes().to_vec())
            .collect();
        let names: Vec<Option<&str>> = members.iter().map(|member| member.name).collect();
        let addresses: Vec<&str> = members.iter().map(|member| member.address).collect();
        let may_fetch: Vec<bool> = members.iter().map(|member| member.may_fetch).collect();
        let mut tx = self.0.begin().await?;
        let share: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT id FROM peer_share WHERE device_id = $1 AND remote_id = $2")
                .bind(device.as_bytes().as_slice())
                .bind(remote.as_uuid())
                .fetch_optional(&mut *tx)
                .await?;
        // The folder went while its roster was being read: nothing to write it against, and the next round
        // will not ask for it again.
        let Some(share) = share else {
            tx.rollback().await?;
            return Ok(false);
        };
        sqlx::query(
            "INSERT INTO peer_share_member (peer_share_id, device_id, name, address, may_fetch) \
             SELECT $1, m.device, m.name, m.address, m.may_fetch \
             FROM UNNEST($2::bytea[], $3::text[], $4::text[], $5::bool[]) \
             AS m(device, name, address, may_fetch) \
             ON CONFLICT (peer_share_id, device_id) DO UPDATE SET name = EXCLUDED.name, \
             address = EXCLUDED.address, may_fetch = EXCLUDED.may_fetch, seen_at = now()",
        )
        .bind(share)
        .bind(&devices)
        .bind(&names)
        .bind(&addresses)
        .bind(&may_fetch)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM peer_share_member WHERE peer_share_id = $1 AND NOT (device_id = ANY($2))",
        )
        .bind(share)
        .bind(&devices)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Who the owners of the folders mirrored here have introduced, and this installation has neither accepted
    /// nor declined: not itself, not somebody already paired with, and not somebody it turned down.
    ///
    /// One row a person a folder: the same person introduced in two folders is two answers to give, because
    /// accepting is about the folder they were introduced in.
    pub async fn introductions(&self) -> Result<Vec<IntroductionRow>, DbError> {
        let rows: Vec<IntroductionTuple> = sqlx::query_as(
                "SELECT m.peer_share_id, ps.name, m.device_id, m.name, m.address, pe.device_id, pe.name \
                 FROM peer_share_member m JOIN peer_share ps ON ps.id = m.peer_share_id \
                 JOIN peer pe ON pe.device_id = ps.device_id \
                 WHERE m.declined_at IS NULL AND pe.removed_at IS NULL \
                 AND m.device_id <> COALESCE((SELECT device_id FROM peer_identity LIMIT 1), '\\x'::bytea) \
                 AND NOT EXISTS (SELECT 1 FROM peer mine \
                   WHERE mine.device_id = m.device_id AND mine.removed_at IS NULL) \
                 ORDER BY m.seen_at, m.device_id",
            )
            .fetch_all(&self.0)
            .await?;
        rows.into_iter()
            .map(
                |(share, share_name, device, name, address, introducer, introducer_name)| {
                    Ok(IntroductionRow {
                        share: PeerShareId::from_uuid(share),
                        share_name,
                        device: stored_device("peer_share_member.device_id", device)?,
                        name,
                        address,
                        introducer: stored_device("peer_share.device_id", introducer)?,
                        introducer_name,
                    })
                },
            )
            .collect()
    }

    /// Turn an introduction down. `false` when there is none left to answer there — already answered, or the
    /// folder's owner took them off it. Nothing is deleted: the row remembers the answer, so the next round
    /// does not offer it again.
    pub async fn decline(&self, share: PeerShareId, device: DeviceId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE peer_share_member SET declined_at = now() \
             WHERE peer_share_id = $1 AND device_id = $2 AND declined_at IS NULL \
             AND NOT EXISTS (SELECT 1 FROM peer mine \
               WHERE mine.device_id = $2 AND mine.removed_at IS NULL)",
        )
        .bind(share.as_uuid())
        .bind(device.as_bytes().as_slice())
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Where an introduction says to reach somebody, and who introduced them. `None` when there is no such
    /// introduction left to answer.
    pub async fn introduction(
        &self,
        share: PeerShareId,
        device: DeviceId,
    ) -> Result<Option<(String, DeviceId)>, DbError> {
        let row: Option<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT m.address, ps.device_id FROM peer_share_member m \
             JOIN peer_share ps ON ps.id = m.peer_share_id \
             JOIN peer pe ON pe.device_id = ps.device_id AND pe.removed_at IS NULL \
             WHERE m.peer_share_id = $1 AND m.device_id = $2 AND m.declined_at IS NULL \
             AND NOT EXISTS (SELECT 1 FROM peer mine \
               WHERE mine.device_id = $2 AND mine.removed_at IS NULL)",
        )
        .bind(share.as_uuid())
        .bind(device.as_bytes().as_slice())
        .fetch_optional(&self.0)
        .await?;
        row.map(|(address, introducer)| {
            Ok((address, stored_device("peer_share.device_id", introducer)?))
        })
        .transpose()
    }

    /// What `device` shares, while it is paired and not removed.
    pub async fn shares_of(&self, device: DeviceId) -> Result<Vec<MirroredShareRow>, DbError> {
        let rows: Vec<ShareTuple> = sqlx::query_as(concat!(
            "SELECT ",
            share_columns!(),
            " WHERE ps.device_id = $1 AND pe.removed_at IS NULL ORDER BY ps.name, ps.id"
        ))
        .bind(device.as_bytes().as_slice())
        .fetch_all(&self.0)
        .await?;
        rows.into_iter().map(share_row).collect()
    }

    /// One mirrored share, while its sharer is paired and not removed.
    pub async fn share(&self, share: PeerShareId) -> Result<Option<MirroredShareRow>, DbError> {
        let row: Option<ShareTuple> = sqlx::query_as(concat!(
            "SELECT ",
            share_columns!(),
            " WHERE ps.id = $1 AND pe.removed_at IS NULL"
        ))
        .bind(share.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        row.map(share_row).transpose()
    }

    /// A page of a mirrored share's parts, by source path.
    pub async fn parts(
        &self,
        share: PeerShareId,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<MirroredPartRow>, DbError> {
        let rows: Vec<PartTuple> = sqlx::query_as(
            "SELECT psp.source_path, psp.remote_part, psp.name, psp.part_number, psp.tags, psp.licences, \
             psp.blake3, psp.size_bytes, psp.format, psp.thumbnail IS NOT NULL \
             FROM peer_share_part psp JOIN peer_share ps ON ps.id = psp.peer_share_id \
             JOIN peer pe ON pe.device_id = ps.device_id \
             WHERE psp.peer_share_id = $1 AND pe.removed_at IS NULL \
             AND ($2::text IS NULL OR psp.source_path > $2) \
             ORDER BY psp.source_path LIMIT $3",
        )
        .bind(share.as_uuid())
        .bind(after)
        .bind(limit)
        .fetch_all(&self.0)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    source_path,
                    remote_part,
                    name,
                    part_number,
                    tags,
                    licences,
                    blake3,
                    size_bytes,
                    format,
                    thumbnail,
                )| {
                    MirroredPartRow {
                        source_path,
                        remote_part: PartId::from_uuid(remote_part),
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

    /// A mirrored part's thumbnail.
    pub async fn thumbnail(
        &self,
        share: PeerShareId,
        source_path: &str,
    ) -> Result<Option<Vec<u8>>, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT psp.thumbnail FROM peer_share_part psp JOIN peer_share ps ON ps.id = psp.peer_share_id \
             JOIN peer pe ON pe.device_id = ps.device_id \
             WHERE psp.peer_share_id = $1 AND psp.source_path = $2 AND pe.removed_at IS NULL \
             AND psp.thumbnail IS NOT NULL",
        )
        .bind(share.as_uuid())
        .bind(source_path)
        .fetch_optional(&self.0)
        .await?)
    }
}
