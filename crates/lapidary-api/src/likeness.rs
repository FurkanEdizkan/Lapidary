//! Parts that look alike (Phase 6; design in `docs/goals/phase-6.md`): a part's likeness, a library's
//! near-duplicates, and what a person decides about a pair.
//!
//! The routes are goal G3's: `GET /api/parts/{id}/likeness`, `GET /api/libraries/{id}/duplicates?since=`,
//! `PUT`/`DELETE /api/parts/{id}/links/{other}`, `POST /api/parts/{id}/fold` and `GET /api/libraries/{id}/folds`.
//! No score is ever sent: a person sees "identical", "near-duplicate" or "similar", never a number, so there is
//! no approximate figure to label.

use crate::AppState;
use crate::parts::PartCard;
use axum::Router;
use jiff::Timestamp;
use lapidary_core::{LibraryId, PartId, PartLinkKind};
use lapidary_db::PgPool;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/parts/{id}/likeness`: the parts that look like this one, each list closest first.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Likeness {
    /// Whether this part has a current shape profile. `false` until the worker has made one — then
    /// `near_duplicates` and `similar` are empty because nothing is known yet, not because nothing is alike.
    pub profiled: bool,
    /// The same bytes: parts in this library whose current source file has this one's hash. Needs no profile.
    pub identical: Vec<PartCard>,
    /// Alike in shape and within 2% in size ([`lapidary_core::shape::is_near_duplicate`]), and not yet decided
    /// about.
    pub near_duplicates: Vec<PartCard>,
    /// "More like this": the closest shapes whatever their size, a few, excluding the lists above.
    pub similar: Vec<PartCard>,
    /// Parts somebody said belong with this one.
    pub variants: Vec<PartCard>,
}

/// One group of parts that look like duplicates of each other.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateCluster {
    /// Two or more, largest `size_mm` first.
    pub parts: Vec<PartCard>,
    /// Every part in the group has the same source bytes, so there is no shape judgement in it.
    pub identical: bool,
}

/// `GET /api/libraries/{id}/duplicates`: the review queue. Computed when read, never stored; a pair somebody
/// called `variant` or `distinct` is never in it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateClusters {
    pub clusters: Vec<DuplicateCluster>,
    /// Live parts with no current profile yet, which could not be compared. The page says so rather than
    /// implying the library was checked in full.
    pub unprofiled: u32,
}

/// The `PUT /api/parts/{id}/links/{other}` body. `foldedInto` is refused here — folding soft-removes a part, and
/// is [`FoldPart`]'s.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct SetLink {
    pub kind: PartLinkKind,
}

/// The `POST /api/parts/{id}/fold` body: fold this part into `into`. In one transaction this part is removed,
/// by the same rule as any removal, and a `foldedInto` link records the part kept. Nothing is moved and nothing
/// is deleted; Restore brings it back. Both parts must be in one library.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct FoldPart {
    pub into: PartId,
}

/// One entry of `GET /api/libraries/{id}/folds`: a removed part that was folded, and the part it was folded
/// into, for the removed page to say where it went.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Fold {
    pub part: PartId,
    pub into: PartCard,
}

/// A library's duplicate clusters, as the duplicates route and the dashboard's `duplicates` widget (G4) both
/// read them. `since` keeps only the clusters holding a part made at or after it — "these new parts look like
/// parts already here".
///
/// Built by goal G3; until then it finds nothing.
pub(crate) async fn clusters(
    db: &PgPool,
    library: LibraryId,
    since: Option<Timestamp>,
) -> Result<DuplicateClusters, lapidary_db::DbError> {
    let _ = (db, library, since);
    Ok(DuplicateClusters {
        clusters: Vec::new(),
        unprofiled: 0,
    })
}

/// This file's routes, merged for the api role. Empty until goal G3.
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
}
