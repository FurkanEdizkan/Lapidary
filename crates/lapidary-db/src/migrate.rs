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
use sqlx::PgPool;
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

pub struct PgStorageMigration(pub PgPool);

impl PgStorageMigration {
    /// The next page of work: up to `hashes` distinct blobs this library still holds in the
    /// content-addressed store, and every un-migrated `file` row that names one of them.
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
        let rows: Vec<PendingRow> = sqlx::query_as(
            "WITH targets AS ( \
               SELECT DISTINCT f.blake3 FROM file f \
                 JOIN revision r ON r.id = f.revision_id \
                 JOIN part p ON p.id = r.part_id \
                WHERE f.storage_path IS NULL AND p.library_id = $1 \
                ORDER BY f.blake3 LIMIT $2) \
             SELECT f.id AS file_id, f.revision_id, f.role, f.format, f.size_bytes, \
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
                AND f.blake3 IN (SELECT blake3 FROM targets) \
              ORDER BY f.blake3, f.id",
        )
        .bind(library.as_uuid())
        .bind(hashes)
        .fetch_all(&self.0)
        .await?;

        rows.into_iter()
            .map(|r| {
                let hash =
                    BlobHash::parse_hex(&r.blake3).map_err(|_| DbError::CorruptBlobHash {
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
            })
            .collect()
    }

    /// Record one hash's move: every `file` row in `moved` gets the path its bytes were
    /// just written to, and the shared `blob` row stops claiming a compression the model
    /// directories do not use.
    ///
    /// Returns whether the content-addressed copy may now be unlinked — read INSIDE this
    /// transaction, after the updates, so the answer cannot be invalidated between the
    /// question and the `unlink` it authorises.
    ///
    /// `AND storage_path IS NULL` guards the update the way `PgJobs::complete` guards its
    /// own: if a second worker reclaimed this job's lease and settled these rows first, its
    /// path is the one the row already holds, and overwriting it with ours would point the
    /// row at one copy while the other is the one that stays.
    pub async fn settle(&self, hash: &BlobHash, moved: &[(Uuid, String)]) -> Result<bool, DbError> {
        let hex = hash.to_hex();
        let ids: Vec<Uuid> = moved.iter().map(|(id, _)| *id).collect();
        let paths: Vec<String> = moved.iter().map(|(_, path)| path.clone()).collect();

        let mut tx = self.0.begin().await?;
        sqlx::query(
            "UPDATE file SET storage_path = t.path \
               FROM unnest($1::uuid[], $2::text[]) AS t(id, path) \
              WHERE file.id = t.id AND file.storage_path IS NULL",
        )
        .bind(&ids)
        .bind(&paths)
        .execute(&mut *tx)
        .await?;

        // Level 0 and `stored_bytes = size_bytes` together, because a row saying it is
        // uncompressed while reporting a compressed size on disk is a row that contradicts
        // itself — and `DATA.md` §1.1's storage panel reads the second column.
        sqlx::query("UPDATE blob SET zstd_level = 0, stored_bytes = size_bytes WHERE blake3 = $1")
            .bind(&hex)
            .execute(&mut *tx)
            .await?;

        // Two questions, both about whether anything still reads `blobs/ab/cd/<hash>`. The
        // `derivative` half cannot fire in practice — it would need a rendered glTF whose
        // bytes hash identically to a source mesh — but the cost of asking is one index
        // probe and the cost of being wrong is deleting bytes a row still serves.
        let unreferenced: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (SELECT 1 FROM file WHERE blake3 = $1 AND storage_path IS NULL) \
                AND NOT EXISTS (SELECT 1 FROM derivative WHERE blake3 = $1)",
        )
        .bind(&hex)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(unreferenced)
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
