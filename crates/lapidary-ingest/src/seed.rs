//! The bundled example parts, ingested once so a first run is never an empty grid.
//!
//! `ROADMAP.md`'s Phase 1 list: *"First run seeds a bundled licence-clean example part —
//! never an empty grid."* The six parts in `example/parts/` are already licence-clean and
//! already have a source — `example/parts/generate.py` produces them, deterministically,
//! from real dimensions — and `deploy/Containerfile` copies them into the worker image.
//!
//! # Why this is not a migration
//!
//! Because a migration cannot write a blob. A seeded `file` row naming bytes the store
//! does not hold is a part whose download answers 500, and a part seeded *without* a
//! source row renders the "no source file on this revision" state — a broken-looking part
//! is worse than an empty grid, not better. Seeding through ingest means real bytes, real
//! measurements and a real thumbnail, produced by the same code path every other part
//! goes through.
//!
//! # Why it is not the ingest mount
//!
//! `deploy/compose.yaml` defaults `LAPIDARY_INGEST_DIR` to this same directory, so a
//! default first run would find these files anyway. But that variable is the *user's*, and
//! an operator who pointed it at a 320 GB corpus must not have it scanned because they
//! restarted the worker. So the examples are read from a path the image owns, and the
//! user's mount is never touched by anything they did not ask for.
//!
//! # Why "has this library ever had a job", and not "is it empty"
//!
//! A library with no parts is not the same as a library nothing has ever happened to. A
//! user who deleted every part has scanned before, and re-seeding on the next restart
//! would put back data they removed — which is the implicit write `CLAUDE.md` forbids in
//! the other direction. A `job` row is the durable record that something has happened
//! here, and it survives deleting every part.

use crate::handler::WorkerHandler;
use crate::scan::candidates;
use lapidary_core::{LibraryId, Outcome};
use lapidary_db::{PgJobs, PgPool};
use std::path::Path;

/// The library migration `0002_parts.sql` seeds, and the only one Phase 1 has: there is
/// no library picker and no route parameter to read one from, so the example parts go
/// where every other part goes.
///
/// Named here rather than passed in, because which library the bundled examples belong to
/// is this module's business and not the binary's — and because a mismatch would silently
/// seed a library nothing displays.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

/// Ingest the bundled examples, unless anything has ever run in the seeded library.
///
/// Best-effort by design: every failure is logged and none of them stops the worker. A
/// server that refuses to start because its demo content would not load is worse than one
/// that starts with an empty grid, and the operator has both a scan button and a drop
/// target to fill it with.
///
/// Returns how many parts it added, which is zero on every run but the first.
pub async fn seed_examples(db: PgPool, blob_root: &Path, examples: &Path) -> u32 {
    let library: LibraryId = match SEEDED_LIBRARY.parse() {
        Ok(library) => library,
        Err(err) => {
            tracing::warn!(error = %err, "the seeded library id does not parse; skipping");
            return 0;
        }
    };
    match PgJobs(db.clone()).library_has_history(library).await {
        Ok(true) => return 0,
        Ok(false) => {}
        Err(err) => {
            tracing::warn!(error = %err, "could not tell whether to seed the example parts; skipping");
            return 0;
        }
    }
    if !examples.is_dir() {
        // Normal, not a failure: a developer running the binary outside the image has no
        // bundled examples, and `deploy/Containerfile` is what puts them there.
        tracing::debug!(path = %examples.display(), "no bundled example parts to seed");
        return 0;
    }

    let files = match candidates(examples) {
        Ok(files) => files,
        Err(err) => {
            tracing::warn!(error = %err, "could not read the bundled example parts");
            return 0;
        }
    };
    // A handler rooted at the examples directory rather than at the ingest mount. This is
    // the one place two roots exist, and it exists here rather than inside a job payload
    // for the reason the slice 6a design gives: a payload that carries its own root is a
    // payload every future reader has to check.
    let handler = WorkerHandler {
        db,
        ingest_dir: examples.to_path_buf(),
        blob_root: blob_root.to_path_buf(),
    };
    let mut added = 0;
    for file in &files {
        match handler.ingest_one(library, file).await {
            Ok(Outcome::Ingested) => added += 1,
            // Already held. Reachable when a previous seed was interrupted part way, and
            // it is the hash short-circuit doing exactly its job.
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(file = %file, error = %err, "could not seed an example part");
            }
        }
    }
    if added > 0 {
        tracing::info!(added, "seeded the bundled example parts");
    }
    added
}
