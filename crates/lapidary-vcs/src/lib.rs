//! Versioning: immutable content-addressed snapshots, a lineage DAG, and pessimistic
//! locks — Perforce-shaped, not Git-shaped. No merges, no branches.
//!
//! Empty. Implementation lands in **Phase 4** (`ROADMAP.md`), the phase the roadmap
//! calls the differentiator. See `docs/DATA.md` §4.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VcsError {
    #[error(
        "This part is locked by {by}. Ask them to release it, or wait — Lapidary uses pessimistic locks instead of merges, so there is no conflict resolution to fall back on."
    )]
    PartLocked { by: String },

    #[error(
        "No revision {revision} exists for this part. Check the revision id, or look at the part's lineage to find the one you meant."
    )]
    RevisionNotFound { revision: String },
}
