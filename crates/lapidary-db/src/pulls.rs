//! Pulls (sharing S3): somebody else's share, fetched into one of this installation's libraries. See `0040_pull.sql`.

use crate::DbError;
use lapidary_core::{BatchId, DeviceId, LibraryId, PartId, PeerShareId, PullId, ShareId};
use sqlx::PgPool;

/// One pull, as the peer role works it and a page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PullRow {
    pub id: PullId,
    /// `None` once the mirror has forgotten the share.
    pub share: Option<PeerShareId>,
    /// The share's id on the sharer's side, which its routes name. `None` with `share`.
    pub remote: Option<ShareId>,
    pub device: DeviceId,
    /// What the sharer calls themselves, as their last hello said.
    pub sharer: Option<String>,
    /// Where the sharer is reached.
    pub address: String,
    /// Whether this installation has removed the sharer.
    pub removed: bool,
    pub share_name: String,
    pub library: LibraryId,
    /// `queued`, `fetching`, `importing`, `done` or `failed`.
    pub state: String,
    pub files_total: i32,
    pub files_done: i32,
    pub bytes_total: i64,
    pub bytes_done: i64,
    pub batch: Option<BatchId>,
    pub error: Option<String>,
    /// The one part this pull is for, by its path in the folder it came from. `None` is the whole folder,
    /// which is every pull before S9 and every "Pull all" since.
    pub source_path: Option<String>,
    /// How many unfinished pulls were started before this one. One pull runs at a time, so a part opened
    /// while another pull is fetching waits its turn, and a page can say how long the queue is.
    pub queued_behind: i64,
}

macro_rules! pull_columns {
    () => {
        "SELECT pu.id AS id, pu.peer_share_id AS share, ps.remote_id AS remote, \
         pu.device_id AS device, pe.name AS sharer, pe.address AS address, \
         (pe.removed_at IS NOT NULL) AS removed, pu.share_name AS share_name, \
         pu.library_id AS library, pu.state AS state, pu.files_total AS files_total, \
         pu.files_done AS files_done, pu.bytes_total AS bytes_total, pu.bytes_done AS bytes_done, \
         pu.batch_id AS batch, pu.error AS error, pu.source_path AS source_path, \
         (SELECT count(*) FROM pull q WHERE q.state IN ('queued', 'fetching', 'waiting', 'importing') \
           AND (q.created_at, q.id) < (pu.created_at, pu.id)) AS queued_behind \
         FROM pull pu JOIN peer pe ON pe.device_id = pu.device_id \
         LEFT JOIN peer_share ps ON ps.id = pu.peer_share_id"
    };
}

// A named row rather than a tuple: seventeen columns is past what sqlx implements `FromRow` for on tuples, and
// every column is aliased above to the field it fills, so adding one is a column, a field and nothing else.
#[derive(sqlx::FromRow)]
struct PullTuple {
    id: uuid::Uuid,
    share: Option<uuid::Uuid>,
    remote: Option<uuid::Uuid>,
    device: Vec<u8>,
    sharer: Option<String>,
    address: String,
    removed: bool,
    share_name: String,
    library: uuid::Uuid,
    state: String,
    files_total: i32,
    files_done: i32,
    bytes_total: i64,
    bytes_done: i64,
    batch: Option<uuid::Uuid>,
    error: Option<String>,
    source_path: Option<String>,
    queued_behind: i64,
}

fn pull_row(row: PullTuple) -> Result<PullRow, DbError> {
    let length = row.device.len();
    let device = <[u8; 32]>::try_from(row.device)
        .map(DeviceId::from_bytes)
        .map_err(|_| DbError::CorruptDeviceId {
            column: "pull.device_id",
            length,
        })?;
    Ok(PullRow {
        id: PullId::from_uuid(row.id),
        share: row.share.map(PeerShareId::from_uuid),
        remote: row.remote.map(ShareId::from_uuid),
        device,
        sharer: row.sharer,
        address: row.address,
        removed: row.removed,
        share_name: row.share_name,
        library: LibraryId::from_uuid(row.library),
        state: row.state,
        files_total: row.files_total,
        files_done: row.files_done,
        bytes_total: row.bytes_total,
        bytes_done: row.bytes_done,
        batch: row.batch.map(BatchId::from_uuid),
        error: row.error,
        source_path: row.source_path,
        queued_behind: row.queued_behind,
    })
}

pub struct PgPulls(pub PgPool);

impl PgPulls {
    /// Record a pull of a mirrored share into `library`, and wake the peer role. `None` when the share is not mirrored
    /// here, or its sharer was removed.
    pub async fn start(
        &self,
        share: PeerShareId,
        library: LibraryId,
        source_path: Option<&str>,
    ) -> Result<Option<PullId>, DbError> {
        let id = PullId::new();
        let inserted = sqlx::query(
            "INSERT INTO pull (id, peer_share_id, device_id, share_name, library_id, source_path) \
             SELECT $1, ps.id, ps.device_id, ps.name, $2, $4 FROM peer_share ps \
             JOIN peer pe ON pe.device_id = ps.device_id WHERE ps.id = $3 AND pe.removed_at IS NULL",
        )
        .bind(id.as_uuid())
        .bind(library.as_uuid())
        .bind(share.as_uuid())
        .bind(source_path)
        .execute(&self.0)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Ok(None);
        }
        crate::sharing::tell_the_peer_role(&self.0).await?;
        Ok(Some(id))
    }

    /// The oldest unfinished pull, paused ones aside: what the peer role works next, and what it picks up again after a
    /// restart. A pull whose sharer was removed here is picked up too, so that it is told so rather than left waiting.
    pub async fn next(&self) -> Result<Option<PullRow>, DbError> {
        let row: Option<PullTuple> = sqlx::query_as(concat!(
            pull_columns!(),
            " WHERE pu.state IN ('queued', 'fetching', 'waiting', 'importing') \
             ORDER BY pu.created_at, pu.id LIMIT 1"
        ))
        .fetch_optional(&self.0)
        .await?;
        row.map(pull_row).transpose()
    }

    /// One pull.
    pub async fn get(&self, pull: PullId) -> Result<Option<PullRow>, DbError> {
        let row: Option<PullTuple> = sqlx::query_as(concat!(pull_columns!(), " WHERE pu.id = $1"))
            .bind(pull.as_uuid())
            .fetch_optional(&self.0)
            .await?;
        row.map(pull_row).transpose()
    }

    /// Waiting for the sharer to grant it. Not while paused.
    pub async fn waiting(&self, pull: PullId) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE pull SET state = 'waiting', updated_at = now() \
             WHERE id = $1 AND state IN ('queued', 'fetching', 'waiting')",
        )
        .bind(pull.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Pause a pull that has not reached its import. The peer role stops after the file it is fetching, and what is
    /// staged stays. `false` when there was nothing to pause.
    pub async fn pause(&self, pull: PullId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE pull SET state = 'paused', updated_at = now() \
             WHERE id = $1 AND state IN ('queued', 'fetching', 'waiting')",
        )
        .bind(pull.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Resume a paused pull, and wake the peer role. `false` when it was not paused.
    pub async fn resume(&self, pull: PullId) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE pull SET state = 'queued', error = NULL, updated_at = now() WHERE id = $1 AND state = 'paused'",
        )
        .bind(pull.as_uuid())
        .execute(&self.0)
        .await?;
        if result.rows_affected() == 0 {
            return Ok(false);
        }
        crate::sharing::tell_the_peer_role(&self.0).await?;
        Ok(true)
    }

    /// The newest pulls, whichever share they were of.
    pub async fn recent(&self, limit: i64) -> Result<Vec<PullRow>, DbError> {
        let rows: Vec<PullTuple> = sqlx::query_as(concat!(
            pull_columns!(),
            " ORDER BY pu.created_at DESC, pu.id DESC LIMIT $1"
        ))
        .bind(limit)
        .fetch_all(&self.0)
        .await?;
        rows.into_iter().map(pull_row).collect()
    }

    /// A share's newest pull, for its page.
    pub async fn latest(&self, share: PeerShareId) -> Result<Option<PullRow>, DbError> {
        let row: Option<PullTuple> = sqlx::query_as(concat!(
            pull_columns!(),
            " WHERE pu.peer_share_id = $1 ORDER BY pu.created_at DESC, pu.id DESC LIMIT 1"
        ))
        .bind(share.as_uuid())
        .fetch_optional(&self.0)
        .await?;
        row.map(pull_row).transpose()
    }

    /// Fetching has begun, and how much there is to fetch. Clears an error a stalled attempt left. `false` when the pull
    /// was paused, and nothing changed.
    pub async fn fetching(
        &self,
        pull: PullId,
        files_total: i32,
        bytes_total: i64,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE pull SET state = 'fetching', files_total = $2, bytes_total = $3, files_done = 0, bytes_done = 0, \
             error = NULL, updated_at = now() WHERE id = $1 AND state <> 'paused'",
        )
        .bind(pull.as_uuid())
        .bind(files_total)
        .bind(bytes_total)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// How much has been fetched, staged files already whole included. `false` when the pull is no longer fetching:
    /// it was paused, and the peer role stops.
    pub async fn progress(
        &self,
        pull: PullId,
        files_done: i32,
        bytes_done: i64,
    ) -> Result<bool, DbError> {
        let result = sqlx::query(
            "UPDATE pull SET files_done = $2, bytes_done = $3, updated_at = now() WHERE id = $1 AND state = 'fetching'",
        )
        .bind(pull.as_uuid())
        .bind(files_done)
        .bind(bytes_done)
        .execute(&self.0)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Stopped for now, for a reason another attempt may not have: the sharer is offline, say. The pull stays
    /// unfinished, and says why.
    pub async fn stalled(&self, pull: PullId, error: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE pull SET error = $2, updated_at = now() WHERE id = $1")
            .bind(pull.as_uuid())
            .bind(error)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// Every file is fetched and its bundles are queued into `batch`.
    pub async fn importing(&self, pull: PullId, batch: BatchId) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE pull SET state = 'importing', batch_id = $2, error = NULL, updated_at = now() WHERE id = $1",
        )
        .bind(pull.as_uuid())
        .bind(batch.as_uuid())
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Finished: `done`, or `failed` with what to tell the person who asked for it.
    pub async fn finish(&self, pull: PullId, error: Option<&str>) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE pull SET state = CASE WHEN $2::text IS NULL THEN 'done' ELSE 'failed' END, error = $2, \
             updated_at = now() WHERE id = $1",
        )
        .bind(pull.as_uuid())
        .bind(error)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Name the sharer on each part now at one of `source_paths` in `library`. Answers how many parts were named.
    pub async fn record_provenance(
        &self,
        library: LibraryId,
        device: DeviceId,
        sharer: Option<&str>,
        source_paths: &[String],
    ) -> Result<u64, DbError> {
        Ok(sqlx::query(
            "INSERT INTO part_provenance (part_id, device_id, sharer_name) \
             SELECT p.id, $2, $3 FROM part p WHERE p.library_id = $1 AND p.source_path = ANY($4) \
             ON CONFLICT (part_id) DO UPDATE SET device_id = EXCLUDED.device_id, \
             sharer_name = EXCLUDED.sharer_name, pulled_at = now()",
        )
        .bind(library.as_uuid())
        .bind(device.as_bytes().as_slice())
        .bind(sharer)
        .bind(source_paths)
        .execute(&self.0)
        .await?
        .rows_affected())
    }

    /// Who a part was pulled from: their name when they gave one, and their device id.
    pub async fn provenance(
        &self,
        part: PartId,
    ) -> Result<Option<(DeviceId, Option<String>)>, DbError> {
        let row: Option<(Vec<u8>, Option<String>)> =
            sqlx::query_as("SELECT device_id, sharer_name FROM part_provenance WHERE part_id = $1")
                .bind(part.as_uuid())
                .fetch_optional(&self.0)
                .await?;
        row.map(|(device, name)| {
            let length = device.len();
            <[u8; 32]>::try_from(device)
                .map(|bytes| (DeviceId::from_bytes(bytes), name))
                .map_err(|_| DbError::CorruptDeviceId {
                    column: "part_provenance.device_id",
                    length,
                })
        })
        .transpose()
    }
}
