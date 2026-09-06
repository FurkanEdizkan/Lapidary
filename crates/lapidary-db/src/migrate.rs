//! The `migrate_storage` job's three statements: what is left to move, the transaction
//! that records one move, and whether anything is left after it.
//!
//! # Why a batch is keyed on the HASH and not on the file row
//!
//! `file.storage_path` is per file; `blob.zstd_level` is per blob. Before the store became
//! a folder tree, ingest deduplicated source bytes — `PgIngest::link_existing` gave two
//! parts (in one library or two) one `blob` row and one file on disk — so a single hash can
//! carry several `file` rows, and every one of them reads through that blob row's recorded
//! level.
//!
//! A model directory holds its file uncompressed, so moving one of those rows means the
//! blob row has to say level 0 afterwards. Do that for one row while a sibling still points
//! at the compressed content-addressed copy and the sibling's every read decodes bytes that
//! were never encoded. There is no ordering of per-row moves that avoids it: the level is
//! one column shared by rows that would need two different values.
//!
//! So [`PgStorageMigration::pending_sources`] selects a page of *hashes* and returns every
//! un-migrated `file` row for each of them — including rows in other libraries, which the
//! caller migrates into their own libraries' directories. The whole group settles in one
//! transaction, which is the only shape where the blob row is true of every reader at every
//! instant.
//!
//! # One runner at a time, per hash
//!
//! Two `migrate_storage` jobs for one library run at once as a matter of course --
//! `PgJobs::reenqueue_migration_if_absent` is a best-effort check with no unique
//! constraint behind it since migration `0011`, and its own doc says the safety of two
//! runs overlapping is established at the execution boundary rather than in the queue.
//! This module is that boundary.
//!
//! [`PgStorageMigration::pending_sources`] is a work list read without exclusion, so it
//! is a stale snapshot the instant it returns. [`PgStorageMigration::claim_hash`] is what
//! a runner acts on: it takes a transaction-scoped advisory lock on the hash and re-reads
//! that hash's un-migrated rows INSIDE the lock, so for as long as the claim is alive
//! every row it holds still says NULL and no second runner can write, settle, or reap the
//! same files. A runner that cannot take the lock is handed `Ok(None)` and skips the hash
//! rather than waiting on it: the lock is held across multi-megabyte file copies, and a
//! queue of runners blocking on one another is a worker pool asleep.
//!
//! # What is deliberately not filtered
//!
//! **Not `deleted_at`.** A soft-deleted part's bytes are still on the volume and must stay
//! readable — delete is soft and purge is separate (CLAUDE.md). Skipping its row would also
//! leave a hash whose group can never complete, so the level could never go to 0 and a
//! migrated sibling would stay broken forever.
//!
//! **Not `role`.** Only `'source'` rows exist today, but a row of any role with a null
//! `storage_path` reads through the same blob row and would be broken by the same level
//! change.

use crate::DbError;
use lapidary_core::{BlobHash, LibraryId, PartId, RevisionId};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// One `file` row still sitting in the content-addressed store, with everything needed to
/// write it into a model directory and describe it in a `metadata.json`.
///
/// Wide because a manifest is wide. The alternative — a narrow row plus a second query per
/// part and per revision — would read the same columns in three round trips instead of one.
#[derive(Debug, Clone)]
pub struct PendingSource {
    /// `file.id`. No newtype: `file` has none anywhere in the workspace.
    pub file_id: Uuid,
    pub revision: RevisionId,
    pub part: PartId,
    /// The row's OWN library, which need not be the one whose job is running — see this
    /// module's doc on hash grouping.
    pub library: LibraryId,
    pub name: String,
    pub part_number: Option<String>,
    pub classification: Option<String>,
    /// Where the file sat in the ingest directory. Identity, and what the model directory
    /// is derived from.
    pub source_path: String,
    pub metadata: serde_json::Value,
    pub rev_label: String,
    pub origin: String,
    pub role: String,
    pub format: String,
    pub size_bytes: i64,
    pub hash: BlobHash,
    /// `blob.zstd_level` exactly as recorded. What the old copy has to be decoded with —
    /// never re-derived from the format, for [`crate::DownloadSource::zstd_level`]'s reason.
    pub zstd_level: Option<i16>,
    pub volume_mm3: Option<f64>,
    pub volume_source: Option<String>,
    pub bbox_mm: Option<[f64; 3]>,
    pub triangle_count: Option<i32>,
    pub is_watertight: Option<bool>,
    pub units: Option<String>,
}

/// The columns, joins and null-`storage_path` filter every reader of an un-migrated row
/// needs, wrapped in the caller's own prefix and predicate.
///
/// One text with two call sites -- [`PgStorageMigration::pending_sources`]'s page and
/// [`PgStorageMigration::claim_hash`]'s authoritative re-read -- rather than the same 24
/// columns written out twice. Both build the same [`PendingRow`], and a column added to
/// one and forgotten in the other would be a `FromRow` failure at runtime, inside a job
/// that runs unattended. A macro rather than a `format!`, because `concat!` resolves at
/// compile time and keeps both queries `&'static str`: nothing here is ever built out of
/// anything read back from the database.
macro_rules! pending_rows_query {
    ($prefix:literal, $predicate:literal) => {
        concat!(
            $prefix,
            "SELECT f.id AS file_id, f.revision_id, f.role, f.format, f.size_bytes, \
                    f.blake3, \
                    p.id AS part_id, p.library_id, p.name, p.part_number, p.classification, \
                    p.source_path, p.metadata_json, \
                    r.rev_label, r.origin, r.volume, r.volume_source, \
                    r.bbox_x, r.bbox_y, r.bbox_z, r.triangle_count, r.is_watertight, \
                    r.units, \
                    b.zstd_level \
               FROM file f \
               JOIN revision r ON r.id = f.revision_id \
               JOIN part p ON p.id = r.part_id \
               LEFT JOIN blob b ON b.blake3 = f.blake3 \
              WHERE f.storage_path IS NULL \
                AND ",
            $predicate
        )
    };
}

/// The first argument of every advisory lock this job takes.
///
/// The TWO-argument lock space, deliberately: `PgFolders::reparent` takes
/// `pg_advisory_xact_lock(hashtext($1))` and the sqlx migrator takes its own, both in the
/// one-argument space, and those two spaces do not intersect. A migration holds its lock
/// across file I/O, so sharing a space with a folder reparent would let a background move
/// stall an interactive one.
const MIGRATE_HASH_LOCK: i32 = 0x4d49_4752; // "MIGR"

/// The `int4` an advisory lock can be keyed on, from the first 32 bits of a 256-bit hash.
///
/// The other 224 bits are thrown away, and that fails SAFE in the only direction that
/// matters. Two different hashes that collide here make one runner skip a hash nobody is
/// holding -- deferred to the next run, which is what a skip already means. It cannot do
/// the reverse and let two runners into one hash, because equal hashes always produce
/// equal keys. The price of a 1-in-4-billion collision is one delayed file move.
fn hash_lock_key(hash: &BlobHash) -> i32 {
    // Infallible in practice: `to_hex` writes 64 lowercase hex digits. `0` if that ever
    // stopped being true, which is a shared key -- more skips, never less exclusion.
    u32::from_str_radix(&hash.to_hex()[..8], 16).unwrap_or(0) as i32
}

/// The raw columns, one per database name, so the mapping below is the only place a column
/// is paired with a field. `FromRow` rather than a tuple because there are more of them
/// than sqlx implements `FromRow` for tuples.
#[derive(sqlx::FromRow)]
struct PendingRow {
    file_id: Uuid,
    revision_id: Uuid,
    part_id: Uuid,
    library_id: Uuid,
    name: String,
    part_number: Option<String>,
    classification: Option<String>,
    source_path: String,
    metadata_json: serde_json::Value,
    rev_label: String,
    origin: String,
    role: String,
    format: String,
    size_bytes: i64,
    blake3: String,
    zstd_level: Option<i16>,
    volume: Option<f64>,
    volume_source: Option<String>,
    bbox_x: Option<f64>,
    bbox_y: Option<f64>,
    bbox_z: Option<f64>,
    triangle_count: Option<i32>,
    is_watertight: Option<bool>,
    units: Option<String>,
}

impl PendingRow {
    /// The one place a column is paired with a field, for both readers of
    /// `pending_rows_query`.
    fn into_source(self) -> Result<PendingSource, DbError> {
        let r = self;
        let hash = BlobHash::parse_hex(&r.blake3).map_err(|_| DbError::CorruptBlobHash {
            column: "file.blake3",
            value: r.blake3.clone(),
        })?;
        Ok(PendingSource {
            file_id: r.file_id,
            revision: RevisionId::from_uuid(r.revision_id),
            part: PartId::from_uuid(r.part_id),
            library: LibraryId::from_uuid(r.library_id),
            name: r.name,
            part_number: r.part_number,
            classification: r.classification,
            source_path: r.source_path,
            metadata: r.metadata_json,
            rev_label: r.rev_label,
            origin: r.origin,
            role: r.role,
            format: r.format,
            size_bytes: r.size_bytes,
            hash,
            zstd_level: r.zstd_level,
            // All three or none: a bounding box missing one axis is not a bounding
            // box, and reporting two of its three numbers would be a measurement
            // that lies.
            bbox_mm: match (r.bbox_x, r.bbox_y, r.bbox_z) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            },
            volume_mm3: r.volume,
            volume_source: r.volume_source,
            triangle_count: r.triangle_count,
            is_watertight: r.is_watertight,
            units: r.units,
        })
    }
}

pub struct PgStorageMigration(pub PgPool);

impl PgStorageMigration {
    /// The next page of work: up to `hashes` distinct blobs this library still holds in the
    /// content-addressed store, and every un-migrated `file` row that names one of them.
    ///
    /// A WORK LIST, not the row set a move acts on. This read takes no lock and is stale
    /// the instant it returns -- another runner can settle any of these rows a moment
    /// later. [`Self::claim_hash`] re-reads each hash's rows under exclusion, and that is
    /// what the copy loop and the settle use. The full rows are still returned here, and
    /// the duplication is deliberate: the caller's re-slug pre-pass needs to know every
    /// LIBRARY this page reaches (a shared hash pulls in rows from libraries whose job is
    /// not running), and that pre-pass is a per-library write that has to happen BEFORE
    /// any claim is taken, because a claim holds a lock across file I/O.
    ///
    /// Ordered by hash and then by file id, so the caller can group consecutive rows without
    /// a map, and so two runs over the same store see the same order. The sort is on the hex
    /// TEXT while the caller compares parsed `BlobHash`es, and that is sound under any
    /// collation for the only reason it needs to be: equal strings sort adjacently. If it
    /// ever stopped being true, one hash would settle as two groups and the second would read
    /// the old copy at the level the first had already dropped to 0.
    pub async fn pending_sources(
        &self,
        library: LibraryId,
        hashes: i64,
    ) -> Result<Vec<PendingSource>, DbError> {
        let rows: Vec<PendingRow> = sqlx::query_as(pending_rows_query!(
            "WITH targets AS ( \
               SELECT DISTINCT f.blake3 FROM file f \
                 JOIN revision r ON r.id = f.revision_id \
                 JOIN part p ON p.id = r.part_id \
                WHERE f.storage_path IS NULL AND p.library_id = $1 \
                ORDER BY f.blake3 LIMIT $2) ",
            "f.blake3 IN (SELECT blake3 FROM targets) \
              ORDER BY f.blake3, f.id"
        ))
        .bind(library.as_uuid())
        .bind(hashes)
        .fetch_all(&self.0)
        .await?;

        rows.into_iter().map(PendingRow::into_source).collect()
    }

    /// Take exclusive ownership of one hash's un-migrated rows for the duration of a move.
    ///
    /// `Ok(None)` means another runner holds this hash right now: the caller skips it and
    /// goes on to the next. Never a failure, and never a wait -- `pg_try_advisory_xact_lock`
    /// returns rather than queueing, because the lock below is held across the whole copy
    /// loop and a worker pool blocking on one multi-megabyte file move is a worker pool
    /// asleep. The skipped hash is still un-migrated, `any_pending` still sees it, and the
    /// runner that holds it settles it.
    ///
    /// The rows come back from a re-read INSIDE the claim's own transaction, never from the
    /// caller's page. That is the whole point: a page is read without exclusion, and by the
    /// time a claim is granted another runner may have settled half of it. Acting on the
    /// page would mean writing files for rows that already point somewhere else, and
    /// reaping those files on the way out.
    ///
    /// **The lock is held across file I/O**, for as long as the caller keeps the claim --
    /// seconds, on a large mesh. That is the accepted trade for a background migration: the
    /// alternative is a shorter lock that does not cover the copy loop, which is the defect
    /// this exists to close. It costs a Postgres connection per in-flight claim, so a worker
    /// running at concurrency N needs headroom for N claims plus the queries the copy loop
    /// makes on the pool. If that ever became the constraint, the answer is a claim that
    /// records ownership in a row with a lease rather than one that holds a transaction --
    /// not a shorter lock.
    pub async fn claim_hash(&self, hash: &BlobHash) -> Result<Option<HashClaim>, DbError> {
        let mut tx = self.0.begin().await?;
        // Transaction-scoped, never `pg_advisory_lock`: this lock is released by the commit
        // or the rollback that ends `tx`, including the rollback a dropped `HashClaim`
        // performs. A session-scoped lock on a POOLED connection would outlive the work,
        // ride back into the pool still held, and lock out every later borrower of that
        // connection -- with nothing left holding a handle that could release it.
        let held: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1, $2)")
            .bind(MIGRATE_HASH_LOCK)
            .bind(hash_lock_key(hash))
            .fetch_one(&mut *tx)
            .await?;
        if !held {
            return Ok(None);
        }

        let hex = hash.to_hex();
        let rows: Vec<PendingRow> = sqlx::query_as(pending_rows_query!(
            "",
            "f.blake3 = $1 \
              ORDER BY f.id"
        ))
        .bind(&hex)
        .fetch_all(&mut *tx)
        .await?;
        let rows = rows
            .into_iter()
            .map(PendingRow::into_source)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(HashClaim { tx, hex, rows }))
    }

    /// Is anything in THIS library still un-migrated? What decides whether the job
    /// re-enqueues itself.
    ///
    /// Scoped to the library on purpose: a global question would have one library's job
    /// re-enqueue forever over another library's rows, which nothing in that batch will
    /// ever move.
    pub async fn any_pending(&self, library: LibraryId) -> Result<bool, DbError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM file f \
               JOIN revision r ON r.id = f.revision_id \
               JOIN part p ON p.id = r.part_id \
              WHERE f.storage_path IS NULL AND p.library_id = $1)",
        )
        .bind(library.as_uuid())
        .fetch_one(&self.0)
        .await?)
    }

    /// Every library that still holds at least one un-migrated source file -- the
    /// worker startup guard's own "who needs a job" read (`bin/lapidary-server`, worker
    /// role only, feeding `PgJobs::enqueue_migration_if_absent` once per library this
    /// returns).
    ///
    /// An ordinary, ungoverned SELECT: it does not need to be race-free with itself,
    /// because the write it feeds is guarded on its own, per library. A library this
    /// read misses on one boot -- because, say, its migration finished a moment after
    /// this query ran -- costs nothing: there is no job left to queue for it anyway. A
    /// library this read finds but a concurrent caller already queued a job for costs
    /// nothing either: `enqueue_migration_if_absent` is the one call in this path that
    /// actually decides, and it is a no-op when there is nothing left to do.
    ///
    /// Not scoped to `role = 'source'`, matching `any_pending`'s own reasoning above:
    /// only `'source'` rows exist today, but any row with a null `storage_path` reads
    /// through the same shared `blob` row and would be broken by the same
    /// compression-level change -- a second, narrower definition of "needs migration"
    /// here would only be free to drift from the first.
    pub async fn libraries_needing_migration(&self) -> Result<Vec<LibraryId>, DbError> {
        let rows: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT p.library_id FROM file f \
               JOIN revision r ON r.id = f.revision_id \
               JOIN part p ON p.id = r.part_id \
              WHERE f.storage_path IS NULL",
        )
        .fetch_all(&self.0)
        .await?;
        Ok(rows.into_iter().map(LibraryId::from_uuid).collect())
    }
}

/// Exclusive ownership of one hash's un-migrated rows while their bytes are moved.
///
/// Opaque on purpose: `lapidary-ingest` holds one across a copy loop and never names an
/// sqlx type to do it ("No SQL outside `lapidary-db`", CLAUDE.md).
///
/// Dropping one WITHOUT calling [`HashClaim::settle`] -- the error path of a copy loop, or
/// a panic -- rolls its transaction back and releases the hash in the same instant. That is
/// the behaviour the copy loop's reap depends on: a claim that ended without settling wrote
/// no path, so every row it held still says NULL, and the files it wrote are files nothing
/// points at.
pub struct HashClaim {
    /// Owns the advisory lock and the read that came after it. `'static` because
    /// `Pool::begin` hands out a transaction that owns its connection, so this struct can
    /// be held across the caller's file I/O without borrowing the pool.
    tx: Transaction<'static, Postgres>,
    /// The claimed hash, hex, as every statement below binds it. Kept here rather than
    /// passed back in at settle time, so there is no way to settle one hash's rows against
    /// another hash's `blob` row.
    hex: String,
    rows: Vec<PendingSource>,
}

impl HashClaim {
    /// Every `file` row that still named the content-addressed copy at the instant this
    /// claim's lock was granted, re-read inside it.
    ///
    /// Empty is a real answer, and it means the runner that held this hash before us
    /// settled it between the caller's page read and this claim. There is nothing to move
    /// and nothing went wrong.
    pub fn rows(&self) -> &[PendingSource] {
        &self.rows
    }

    /// Record this claim's move: every `file` row in `moved` gets the path its bytes were
    /// just written to, and the shared `blob` row stops claiming a compression the model
    /// directories do not use. Consumes the claim -- the commit that records the paths is
    /// the same commit that releases the hash.
    ///
    /// Returns whether the content-addressed copy may now be unlinked — read INSIDE this
    /// transaction, after the updates, so the answer cannot be invalidated between the
    /// question and the `unlink` it authorises.
    ///
    /// `AND storage_path IS NULL` stays on the update although the claim has made it
    /// unreachable for the case it was written against: these rows were read under the
    /// advisory lock this transaction still holds, so no second migration runner can have
    /// recorded a path since. It costs one index probe, and it is the difference between a
    /// row pointing at one copy while the other is the one that stays, and a row keeping
    /// the path its bytes are really at, if a writer nobody has thought of ever appears.
    pub async fn settle(mut self, moved: &[(Uuid, String)]) -> Result<bool, DbError> {
        let ids: Vec<Uuid> = moved.iter().map(|(id, _)| *id).collect();
        let paths: Vec<String> = moved.iter().map(|(_, path)| path.clone()).collect();

        sqlx::query(
            "UPDATE file SET storage_path = t.path \
               FROM unnest($1::uuid[], $2::text[]) AS t(id, path) \
              WHERE file.id = t.id AND file.storage_path IS NULL",
        )
        .bind(&ids)
        .bind(&paths)
        .execute(&mut *self.tx)
        .await?;

        // Level 0 and `stored_bytes = size_bytes` together, because a row saying it is
        // uncompressed while reporting a compressed size on disk is a row that contradicts
        // itself — and `DATA.md` §1.1's storage panel reads the second column.
        sqlx::query("UPDATE blob SET zstd_level = 0, stored_bytes = size_bytes WHERE blake3 = $1")
            .bind(&self.hex)
            .execute(&mut *self.tx)
            .await?;

        // Two questions, both about whether anything still reads `blobs/ab/cd/<hash>`. The
        // `derivative` half cannot fire in practice — it would need a rendered glTF whose
        // bytes hash identically to a source mesh — but the cost of asking is one index
        // probe and the cost of being wrong is deleting bytes a row still serves.
        let unreferenced: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (SELECT 1 FROM file WHERE blake3 = $1 AND storage_path IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM derivative WHERE blake3 = $1)",
        )
        .bind(&self.hex)
        .fetch_one(&mut *self.tx)
        .await?;

        self.tx.commit().await?;
        Ok(unreferenced)
    }
}
