//! Sharing (S1b): who this installation is to the people it shares with, and who they are.
//!
//! The peer role writes the identity as it starts and records each hello; the api reads both and edits
//! the list. Removing someone is soft, like everything else a person can remove here.

use crate::DbError;
use crate::repo::detail_stamp;
use jiff::Timestamp;
use lapidary_core::DeviceId;
use sqlx::PgPool;

/// How recently a person must have answered to count as online: three of the peer role's hello rounds,
/// so one lost round does not flicker the page. Compared on the database's clock, the one clock the api
/// and the peer role share.
pub const ONLINE_WITHIN_SECS: i64 = 45;

/// The channel that tells the peer role, at once, that somebody was paired or removed or a category was shared
/// or stopped: its hello round starts then rather than at the next tick. The tick stays the floor that
/// correctness rests on, as the job queue's does.
pub const SHARING_CHANNEL: &str = "lapidary_sharing";

/// This installation, as the people it shares with know it.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityRow {
    pub device_id: DeviceId,
    pub name: Option<String>,
}

/// Somebody this installation is paired with.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerRow {
    pub device_id: DeviceId,
    pub address: String,
    /// What they call themselves, as their last hello said. `None` until one has.
    pub name: Option<String>,
    pub added_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
    /// Why the last hello failed. `None` once one succeeds.
    pub last_error: Option<String>,
    /// The last hello succeeded, within [`ONLINE_WITHIN_SECS`].
    pub online: bool,
}

pub struct PgSharing(pub PgPool);

/// Wake the peer role's hello round (see [`SHARING_CHANNEL`]).
pub(crate) async fn tell_the_peer_role(db: &PgPool) -> Result<(), DbError> {
    sqlx::query("SELECT pg_notify($1, '')")
        .bind(SHARING_CHANNEL)
        .execute(db)
        .await?;
    Ok(())
}

impl PgSharing {
    /// This installation's identity. `None` when the peer role has never run here.
    pub async fn identity(&self) -> Result<Option<IdentityRow>, DbError> {
        let row: Option<(Vec<u8>, Option<String>)> =
            sqlx::query_as("SELECT device_id, name FROM peer_identity")
                .fetch_optional(&self.0)
                .await?;
        row.map(|(device_id, name)| {
            Ok(IdentityRow {
                device_id: stored_device("peer_identity.device_id", device_id)?,
                name,
            })
        })
        .transpose()
    }

    /// Record the device id the peer role started with, keeping the name its owner gave. A new key makes
    /// a new id, and the name belongs to the installation rather than to the key.
    pub async fn claim_identity(&self, device: DeviceId) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO peer_identity (device_id) VALUES ($1) \
             ON CONFLICT (singleton) DO UPDATE SET device_id = EXCLUDED.device_id",
        )
        .bind(device.as_bytes().as_slice())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Set, or with `None` clear, what this installation calls itself. `false` when there is no identity
    /// to name yet.
    pub async fn set_name(&self, name: Option<&str>) -> Result<bool, DbError> {
        let result = sqlx::query("UPDATE peer_identity SET name = $1")
            .bind(name)
            .execute(&self.0)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Everyone paired and not removed, the earliest added first.
    pub async fn peers(&self) -> Result<Vec<PeerRow>, DbError> {
        let rows: Vec<PeerTuple> = sqlx::query_as(concat!(
            "SELECT ",
            peer_columns!(),
            " FROM peer WHERE removed_at IS NULL ORDER BY added_at, device_id"
        ))
        .bind(ONLINE_WITHIN_SECS)
        .fetch_all(&self.0)
        .await?;
        rows.into_iter().map(peer_row).collect()
    }

    /// Pair with an installation, or reach one already paired at a new address. Somebody removed comes
    /// back as the row they were, rather than as a second one.
    pub async fn add_peer(&self, device: DeviceId, address: &str) -> Result<PeerRow, DbError> {
        sqlx::query(
            "INSERT INTO peer (device_id, address) VALUES ($1, $2) \
             ON CONFLICT (device_id) DO UPDATE SET address = EXCLUDED.address, removed_at = NULL",
        )
        .bind(device.as_bytes().as_slice())
        .bind(address)
        .execute(&self.0)
        .await?;
        let row: PeerTuple = sqlx::query_as(concat!(
            "SELECT ",
            peer_columns!(),
            " FROM peer WHERE device_id = $2"
        ))
        .bind(ONLINE_WITHIN_SECS)
        .bind(device.as_bytes().as_slice())
        .fetch_one(&self.0)
        .await?;
        tell_the_peer_role(&self.0).await?;
        peer_row(row)
    }

    /// Pair with somebody a folder's owner introduced, recording who introduced them. The same pairing adding
    /// a device id by hand makes — an introduction is how the id arrived, not a second kind of access — with
    /// the introducer kept for the list to say where they came from.
    ///
    /// One transaction: a pairing whose introducer was not written is a person on the list with no answer to
    /// "who is this", which is the one question a page about introductions has to be able to answer.
    pub async fn accept_introduction(
        &self,
        device: DeviceId,
        address: &str,
        introduced_by: DeviceId,
    ) -> Result<PeerRow, DbError> {
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "INSERT INTO peer (device_id, address, introduced_by) VALUES ($1, $2, $3) \
             ON CONFLICT (device_id) DO UPDATE SET address = EXCLUDED.address, removed_at = NULL, \
             introduced_by = COALESCE(peer.introduced_by, EXCLUDED.introduced_by)",
        )
        .bind(device.as_bytes().as_slice())
        .bind(address)
        .bind(introduced_by.as_bytes().as_slice())
        .execute(&mut *tx)
        .await?;
        let row: PeerTuple = sqlx::query_as(concat!(
            "SELECT ",
            peer_columns!(),
            " FROM peer WHERE device_id = $2"
        ))
        .bind(ONLINE_WITHIN_SECS)
        .bind(device.as_bytes().as_slice())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        tell_the_peer_role(&self.0).await?;
        peer_row(row)
    }

    /// Remove somebody: hidden from the list and refused by the peer role, and nothing deleted. `false`
    /// when they were not paired, or were removed already.
    pub async fn remove_peer(&self, device: DeviceId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE peer SET removed_at = now() WHERE device_id = $1 AND removed_at IS NULL",
        )
        .bind(device.as_bytes().as_slice())
        .execute(&self.0)
        .await?;
        tell_the_peer_role(&self.0).await?;
        Ok(result.rows_affected() > 0)
    }

    /// What the peer role's hello round needs: each paired installation and where to reach it.
    pub async fn paired(&self) -> Result<Vec<(DeviceId, String)>, DbError> {
        let rows: Vec<(Vec<u8>, String)> = sqlx::query_as(
            "SELECT device_id, address FROM peer WHERE removed_at IS NULL ORDER BY added_at, device_id",
        )
        .fetch_all(&self.0)
        .await?;
        rows.into_iter()
            .map(|(device_id, address)| Ok((stored_device("peer.device_id", device_id)?, address)))
            .collect()
    }

    /// A hello answered, with the name the other installation gave and what it said it can do.
    ///
    /// The features are what keeps mixed versions working: an installation from before a route existed lists
    /// nothing, and nothing asks it for that route. An installation that stops answering something drops it
    /// from the list, and this follows on the next hello.
    pub async fn seen(
        &self,
        device: DeviceId,
        name: Option<&str>,
        features: &[String],
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE peer SET last_seen_at = now(), last_error = NULL, name = $2, features = $3 \
             WHERE device_id = $1",
        )
        .bind(device.as_bytes().as_slice())
        .bind(name)
        .bind(features)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// What an installation said it can do, as its last hello listed it. Empty for one that has not said
    /// hello here yet, or is not paired at all: nothing new is asked of an installation until it says it
    /// answers, and never asking is the safe direction.
    pub async fn features(&self, device: DeviceId) -> Result<Vec<String>, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT features FROM peer WHERE device_id = $1 AND removed_at IS NULL",
        )
        .bind(device.as_bytes().as_slice())
        .fetch_optional(&self.0)
        .await?
        .unwrap_or_default())
    }

    /// A hello that failed, and why.
    pub async fn unreachable(&self, device: DeviceId, reason: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE peer SET last_error = $2 WHERE device_id = $1")
            .bind(device.as_bytes().as_slice())
            .bind(reason)
            .execute(&self.0)
            .await?;
        Ok(())
    }
}

/// A person's columns, in [`PeerTuple`]'s order. `$1` is [`ONLINE_WITHIN_SECS`]: online is decided on the
/// database's clock, never by comparing a timestamp in whichever process happens to read it.
macro_rules! peer_columns {
    () => {
        "device_id, address, name, \
         (extract(epoch FROM added_at) * 1000000)::bigint, \
         (extract(epoch FROM last_seen_at) * 1000000)::bigint, \
         last_error, \
         (last_error IS NULL AND last_seen_at > now() - make_interval(secs => $1::float8)) IS TRUE"
    };
}
use peer_columns;

type PeerTuple = (
    Vec<u8>,
    String,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
    bool,
);

fn peer_row(
    (device_id, address, name, added_us, seen_us, last_error, online): PeerTuple,
) -> Result<PeerRow, DbError> {
    Ok(PeerRow {
        device_id: stored_device("peer.device_id", device_id)?,
        address,
        name,
        added_at: detail_stamp("peer.added_at", added_us)?,
        last_seen_at: seen_us
            .map(|us| detail_stamp("peer.last_seen_at", us))
            .transpose()?,
        last_error,
        online,
    })
}

/// A stored device id back into the type. The table's check keeps every one 32 bytes.
pub(crate) fn stored_device(column: &'static str, bytes: Vec<u8>) -> Result<DeviceId, DbError> {
    let length = bytes.len();
    <[u8; 32]>::try_from(bytes)
        .map(DeviceId::from_bytes)
        .map_err(|_| DbError::CorruptDeviceId { column, length })
}
