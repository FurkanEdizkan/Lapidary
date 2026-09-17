//! Sharing (S2b): other installations' shares, mirrored here by the peer role's hello round and read by the api.
//!
//! A cache of another machine's list, not this installation's data (`0039`'s comment says what that allows).
//! Every read for a page requires the sharer to be paired and not removed, so removing somebody hides what was
//! mirrored from them at once, without deleting it.

use crate::DbError;
use crate::repo::detail_stamp;
use crate::shares::BlobLocation;
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
    /// Whose folder it is, when the installation offering it is not its owner — a member passing on a folder
    /// it holds (S7). `None` is the ordinary case: the installation answering owns what it offers.
    pub owner: Option<DeviceId>,
    /// When the offering installation read this catalogue from its owner, for a folder it does not own. What
    /// decides between two copies of the same folder; `None` is a relay that cannot say, and is never taken.
    pub as_of: Option<Timestamp>,
}

/// A mirrored share whose catalogue must be read again, and the digest to record once it has been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleShare {
    pub id: PeerShareId,
    pub remote: ShareId,
    pub digest: String,
    /// Whose folder it is. The same as the installation being read, except for a folder relayed by a member.
    pub owner: DeviceId,
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

/// Whether this installation may serve another of a folder's people its files (S8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Serving {
    Yes(PeerShareId),
    /// Not mirrored here, or the caller is not on the folder's roster, or is on it without leave to fetch.
    NotShared,
    /// Mirrored here, and this installation has been told to stop passing that folder's files on.
    NotSeeding,
}

/// A folder mirrored here, as it is passed on to another of its people (S7).
#[derive(Debug, Clone, PartialEq)]
pub struct RelayedShare {
    pub owner: DeviceId,
    /// Its owner's id for it, which is what every installation holding it keys it by.
    pub remote: ShareId,
    pub name: String,
    pub part_count: i64,
    /// The owner's digest, as it was when this copy was read, so the reader compares like with like.
    pub digest: String,
    /// When this copy was read from the folder's owner.
    pub as_of: Option<Timestamp>,
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
    /// The member this copy of the catalogue was read from, when it was not read from the folder's owner, and
    /// what they call themselves. `None` is the ordinary case: read from the owner (S7).
    pub read_from: Option<DeviceId>,
    pub read_from_name: Option<String>,
    /// When the copy held here was read from the folder's owner, by whoever read it.
    pub as_of: Option<Timestamp>,
    /// Whether this installation passes the folder's files on to its other people (S8).
    pub seeding: bool,
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
         (extract(epoch FROM ps.synced_at) * 1000000)::bigint, \
         ps.catalogue_from, relay.name, \
         (extract(epoch FROM ps.catalogue_as_of) * 1000000)::bigint, ps.seeding \
         FROM peer_share ps JOIN peer pe ON pe.device_id = ps.device_id \
         LEFT JOIN peer relay ON relay.device_id = ps.catalogue_from"
    };
}

type ShareTuple = (
    uuid::Uuid,
    Vec<u8>,
    Option<String>,
    String,
    i64,
    Option<i64>,
    Option<Vec<u8>>,
    Option<String>,
    Option<i64>,
    bool,
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

type RelayedTuple = (Vec<u8>, uuid::Uuid, String, i64, String, Option<i64>);

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
    (id, device, sharer, name, part_count, synced_us, read_from, read_from_name, as_of_us, seeding): ShareTuple,
) -> Result<MirroredShareRow, DbError> {
    Ok(MirroredShareRow {
        id: PeerShareId::from_uuid(id),
        device: stored_device("peer_share.device_id", device)?,
        sharer,
        name,
        part_count,
        synced_at: synced_us
            .map(|us| detail_stamp("peer_share.synced_at", us))
            .transpose()?,
        read_from: read_from
            .map(|bytes| stored_device("peer_share.catalogue_from", bytes))
            .transpose()?,
        read_from_name,
        as_of: as_of_us
            .map(|us| detail_stamp("peer_share.catalogue_as_of", us))
            .transpose()?,
        seeding,
    })
}

pub struct PgMirror(pub PgPool);

impl PgMirror {
    /// Take up the list `device` offers now: add the folders it newly offers, keep each one's name and part
    /// count current, and delete, with their parts, the folders **it owns** and no longer offers. Answers the
    /// folders whose catalogue must be read again.
    ///
    /// A folder is keyed by its owner, not by who mentioned it (S7), so what a member passes on lands beside
    /// what its owner says rather than as a second copy. That is also why the delete is scoped to the folders
    /// `device` owns: a list from a member says what that member still holds, never what somebody else still
    /// shares, so relayed knowledge only ever adds. A folder its owner stops offering goes when that owner is
    /// next read, which is the one installation entitled to say so.
    ///
    /// A folder is read again when its digest has moved — and, for a relayed one, only when the relay read it
    /// from its owner later than the copy held here was read. Freshness alone decides, in both directions: a
    /// relayed copy that is newer wins, and a relay that is behind is left alone.
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
        let owners: Vec<Vec<u8>> = offered
            .iter()
            .map(|o| o.owner.unwrap_or(device).as_bytes().to_vec())
            .collect();
        // Microseconds, as every timestamp crosses this crate's edge, and null for a copy read from its owner.
        let as_of: Vec<Option<i64>> = offered
            .iter()
            .map(|o| o.as_of.map(|at| at.as_microsecond()))
            .collect();
        let own: Vec<uuid::Uuid> = offered
            .iter()
            .filter(|o| o.owner.is_none_or(|owner| owner == device))
            .map(|o| o.remote.as_uuid())
            .collect();
        let device_bytes = device.as_bytes().as_slice();
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "INSERT INTO peer_share (id, device_id, remote_id, name, part_count) \
             SELECT o.id, o.owner, o.remote, o.name, o.part_count \
             FROM UNNEST($1::uuid[], $2::bytea[], $3::uuid[], $4::text[], $5::int8[]) \
             AS o(id, owner, remote, name, part_count) \
             ON CONFLICT (device_id, remote_id) DO UPDATE SET name = EXCLUDED.name, part_count = EXCLUDED.part_count",
        )
        .bind(&ids)
        .bind(&owners)
        .bind(&remotes)
        .bind(&names)
        .bind(&counts)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM peer_share WHERE device_id = $1 AND NOT (remote_id = ANY($2))")
            .bind(device_bytes)
            .bind(&own)
            .execute(&mut *tx)
            .await?;
        let stale: Vec<(uuid::Uuid, uuid::Uuid, String, Vec<u8>)> = sqlx::query_as(
            "SELECT ps.id, ps.remote_id, o.digest, ps.device_id \
             FROM UNNEST($1::bytea[], $2::uuid[], $3::text[], $4::int8[]) \
             AS o(owner, remote, digest, as_of) \
             JOIN peer_share ps ON ps.device_id = o.owner AND ps.remote_id = o.remote \
             WHERE (ps.synced_at IS NULL OR ps.digest IS DISTINCT FROM o.digest) \
             AND (o.owner = $5 OR (o.as_of IS NOT NULL AND (ps.catalogue_as_of IS NULL \
               OR to_timestamp(o.as_of / 1000000.0) > ps.catalogue_as_of))) \
             ORDER BY ps.name, ps.id",
        )
        .bind(&owners)
        .bind(&remotes)
        .bind(&digests)
        .bind(&as_of)
        .bind(device_bytes)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        stale
            .into_iter()
            .map(|(id, remote, digest, owner)| {
                Ok(StaleShare {
                    id: PeerShareId::from_uuid(id),
                    remote: ShareId::from_uuid(remote),
                    digest,
                    owner: stored_device("peer_share.device_id", owner)?,
                })
            })
            .collect()
    }

    /// Replace a share's parts with a whole catalogue read from its owner, in one transaction: a read that
    /// fails part-way leaves the mirror as it was.
    pub async fn replace_catalogue(
        &self,
        share: PeerShareId,
        digest: &str,
        parts: &[MirroredPartIn<'_>],
    ) -> Result<(), DbError> {
        self.write_catalogue(share, digest, parts, None, None).await
    }

    /// The same, for a catalogue read from a member of the folder rather than from its owner (S7): what is
    /// written is that installation's reading, under the owner's digest and the owner's as-of, so the next
    /// round compares like with like — and a page can say whose reading it is showing.
    pub async fn relay_catalogue(
        &self,
        share: PeerShareId,
        digest: &str,
        parts: &[MirroredPartIn<'_>],
        from: DeviceId,
        as_of: Timestamp,
    ) -> Result<(), DbError> {
        self.write_catalogue(share, digest, parts, Some(from), Some(as_of))
            .await
    }

    async fn write_catalogue(
        &self,
        share: PeerShareId,
        digest: &str,
        parts: &[MirroredPartIn<'_>],
        from: Option<DeviceId>,
        as_of: Option<Timestamp>,
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
        // `synced_at` is when this installation wrote the row; `catalogue_as_of` is when the copy was read
        // from the folder's owner, by whoever read it. For a direct read those are the same moment.
        sqlx::query(
            "UPDATE peer_share SET digest = $2, synced_at = now(), catalogue_from = $3, \
             catalogue_as_of = coalesce(to_timestamp($4 / 1000000.0), now()) WHERE id = $1",
        )
        .bind(share.as_uuid())
        .bind(digest)
        .bind(from.map(|device| device.as_bytes().to_vec()))
        .bind(as_of.map(|at| at.as_microsecond()))
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

    /// The folders mirrored here that may be passed on to `caller`: the ones whose roster, as their owner
    /// published it, names them (S7). Their own folders are not among them — those are read from their owner.
    ///
    /// This is what lets a folder stay browsable while its owner is away. It says nothing a member could not
    /// read from the owner: the roster the owner published is what decides, and this installation is only
    /// repeating a catalogue it was given.
    pub async fn relayable_to(&self, caller: DeviceId) -> Result<Vec<RelayedShare>, DbError> {
        let rows: Vec<RelayedTuple> = sqlx::query_as(
            "SELECT ps.device_id, ps.remote_id, ps.name, ps.part_count, ps.digest, \
             (extract(epoch FROM ps.catalogue_as_of) * 1000000)::bigint \
             FROM peer_share ps JOIN peer_share_member m ON m.peer_share_id = ps.id \
             JOIN peer owner ON owner.device_id = ps.device_id AND owner.removed_at IS NULL \
             WHERE m.device_id = $1 AND ps.device_id <> $1 AND ps.synced_at IS NOT NULL \
             ORDER BY ps.name, ps.id",
        )
        .bind(caller.as_bytes().as_slice())
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(|(owner, remote, name, part_count, digest, as_of_us)| {
                Ok(RelayedShare {
                    owner: stored_device("peer_share.device_id", owner)?,
                    remote: ShareId::from_uuid(remote),
                    name,
                    part_count,
                    digest,
                    as_of: as_of_us
                        .map(|us| detail_stamp("peer_share.catalogue_as_of", us))
                        .transpose()?,
                })
            })
            .collect()
    }

    /// Whether this installation may serve `caller` a file of the folder `owner` owns as `remote` (S8).
    ///
    /// Four things, and all four: this installation mirrors that folder, its owner is somebody it is still
    /// paired with, it is seeding that folder, and the caller is on the roster the folder's owner published
    /// **with the owner's leave to fetch**. The roster is the authorization, exactly as it is for the
    /// catalogue: holding a file is not what entitles anybody to it.
    pub async fn serves(
        &self,
        owner: DeviceId,
        remote: ShareId,
        caller: DeviceId,
    ) -> Result<Serving, DbError> {
        let row: Option<(uuid::Uuid, bool, bool)> = sqlx::query_as(
            "SELECT ps.id, ps.seeding, (m.may_fetch IS TRUE) \
             FROM peer_share ps \
             JOIN peer owner ON owner.device_id = ps.device_id AND owner.removed_at IS NULL \
             LEFT JOIN peer_share_member m ON m.peer_share_id = ps.id AND m.device_id = $3 \
             WHERE ps.device_id = $1 AND ps.remote_id = $2 AND ps.synced_at IS NOT NULL",
        )
        .bind(owner.as_bytes().as_slice())
        .bind(remote.as_uuid())
        .bind(caller.as_bytes().as_slice())
        .fetch_optional(&self.0)
        .await?;
        Ok(match row {
            // Not on the folder's roster, or on it without leave to fetch, reads the same as never having
            // heard of the folder: an answer that distinguished them would say who else is in it.
            None => Serving::NotShared,
            Some((_, _, false)) => Serving::NotShared,
            Some((_, false, _)) => Serving::NotSeeding,
            Some((id, true, true)) => Serving::Yes(PeerShareId::from_uuid(id)),
        })
    }

    /// Where a file of a mirrored folder is, when this installation holds it: the hash is in **that folder's**
    /// catalogue, and a live part here has it as a source file.
    ///
    /// Both halves, never either alone. Holding a hash that the folder does not list would make knowing a hash
    /// enough to be given the bytes, which is the one thing content addressing must never mean.
    ///
    /// **Any** live part here holding those bytes satisfies the second half, not only the one pulled from that
    /// folder: the file is the same file, and the folder listing it is what says this caller may have it. So a
    /// part pulled from the folder and then removed is still served while some other part here holds the same
    /// bytes — which is what a content-addressed store means by holding a file at all.
    pub async fn held(
        &self,
        share: PeerShareId,
        blake3: &str,
    ) -> Result<Option<BlobLocation>, DbError> {
        let row: Option<(Option<String>, Option<i16>, i64)> = sqlx::query_as(
            "SELECT f.storage_path, f.zstd_level, f.size_bytes FROM peer_share_part psp \
             JOIN file f ON f.blake3 = psp.blake3 AND f.role = 'source' \
             JOIN revision r ON r.id = f.revision_id \
             JOIN part p ON p.id = r.part_id AND p.deleted_at IS NULL \
             WHERE psp.peer_share_id = $1 AND psp.blake3 = $2 \
             ORDER BY f.created_at DESC, f.id DESC LIMIT 1",
        )
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

    /// How many of a mirrored folder's files this installation holds, and how many it lists — the page's
    /// "137 of 402 files here can be served from this installation".
    pub async fn held_count(&self, share: PeerShareId) -> Result<(i64, i64), DbError> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE EXISTS ( \
               SELECT 1 FROM file f JOIN revision r ON r.id = f.revision_id \
               JOIN part p ON p.id = r.part_id AND p.deleted_at IS NULL \
               WHERE f.blake3 = psp.blake3 AND f.role = 'source')), count(*) \
             FROM peer_share_part psp WHERE psp.peer_share_id = $1 AND psp.blake3 IS NOT NULL",
        )
        .bind(share.as_uuid())
        .fetch_one(&self.0)
        .await?;
        Ok(row)
    }

    /// Seed a mirrored folder, or stop. `false` when there is no such folder mirrored here.
    pub async fn set_seeding(&self, share: PeerShareId, seeding: bool) -> Result<bool, DbError> {
        let result = sqlx::query("UPDATE peer_share SET seeding = $2 WHERE id = $1")
            .bind(share.as_uuid())
            .bind(seeding)
            .execute(&self.0)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// The mirrored folder `owner` owns as `remote`, when `caller` is on the roster its owner published for it
    /// — the one question every relayed read asks first (S7).
    ///
    /// `None` is the refusal a stranger gets: this installation does not mirror that folder, or the folder's
    /// owner never said it goes to them. Holding a folder's bytes is not what entitles anybody to them; being
    /// on its owner's list is.
    pub async fn relayed_to(
        &self,
        owner: DeviceId,
        remote: ShareId,
        caller: DeviceId,
    ) -> Result<Option<PeerShareId>, DbError> {
        let row: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT ps.id FROM peer_share ps JOIN peer_share_member m ON m.peer_share_id = ps.id \
             JOIN peer owner ON owner.device_id = ps.device_id AND owner.removed_at IS NULL \
             WHERE ps.device_id = $1 AND ps.remote_id = $2 AND m.device_id = $3 \
             AND ps.synced_at IS NOT NULL",
        )
        .bind(owner.as_bytes().as_slice())
        .bind(remote.as_uuid())
        .bind(caller.as_bytes().as_slice())
        .fetch_optional(&self.0)
        .await?;
        Ok(row.map(PeerShareId::from_uuid))
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

    /// A mirrored part's thumbnail, by the owner's id for the part — which is how another installation asks
    /// for it, since that is the id the folder's own catalogue gave them (S7).
    pub async fn thumbnail_of(
        &self,
        share: PeerShareId,
        remote_part: PartId,
    ) -> Result<Option<Vec<u8>>, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT psp.thumbnail FROM peer_share_part psp JOIN peer_share ps ON ps.id = psp.peer_share_id \
             JOIN peer pe ON pe.device_id = ps.device_id \
             WHERE psp.peer_share_id = $1 AND psp.remote_part = $2 AND pe.removed_at IS NULL \
             AND psp.thumbnail IS NOT NULL",
        )
        .bind(share.as_uuid())
        .bind(remote_part.as_uuid())
        .fetch_optional(&self.0)
        .await?
        .flatten())
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
