//! Parts that look alike (Phase 6; design in `docs/goals/phase-6.md`): a part's likeness, a library's
//! near-duplicates, and what a person decides about a pair.
//!
//! The routes are goal G3's: `GET /api/parts/{id}/likeness`, `GET /api/libraries/{id}/duplicates?since=`,
//! `PUT`/`DELETE /api/parts/{id}/links/{other}`, `POST /api/parts/{id}/fold` and `GET /api/libraries/{id}/folds`.
//! No score is ever sent: a person sees "identical", "near-duplicate" or "similar", never a number, so there is
//! no approximate figure to label.

use crate::AppState;
use crate::derive::{internal_error, no_such_part};
use crate::parts::{PartCard, to_card};
use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use jiff::Timestamp;
use lapidary_core::{LibraryId, PartId, PartLinkKind};
use lapidary_db::{Folded, PgLikeness, PgParts, PgPool, Shows};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use ts_rs::TS;

/// How many parts a likeness list holds when the caller says nothing, and the most it will hand back.
/// The cap is the part page's own concern — each card carries an inline thumbnail, so a list of 500 is
/// a megabyte of pictures nobody scrolls to.
const DEFAULT_LIMIT: usize = 24;
const MAX_LIMIT: usize = 96;

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

/// `GET /api/parts/{id}/likeness`'s one knob. An unreadable or absent `limit` is the default rather
/// than a `400`: `?limit=` is the natural shape of the URL before anything is chosen, exactly as the
/// grid's `after=&limit=` is (see `parts.rs`).
#[derive(Debug, Default, Deserialize)]
struct LikenessQuery {
    #[serde(default)]
    limit: Option<String>,
}

/// `GET /api/libraries/{id}/duplicates`'s one knob.
#[derive(Debug, Default, Deserialize)]
struct DuplicatesQuery {
    /// An RFC 3339 timestamp. Absent or empty is every cluster; anything else that is not a timestamp
    /// is a `400`, because quietly widening the answer would report more duplicates than was asked
    /// about and read as a change in the library.
    #[serde(default)]
    since: Option<String>,
}

/// `GET /api/parts/{id}/likeness?limit=24` — the part page's "looks like this" panel.
///
/// Four reads, none of which touches a source file or the kernel: the library's current profiles, the
/// parts sharing this one's bytes, this part's links, and the cards for whatever the ranking kept. A
/// part with no current profile still answers — `identical` and `variants` need no shape — and says
/// `profiled: false` so the page can say "not compared yet" rather than "nothing is alike".
async fn likeness(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Query(query): Query<LikenessQuery>,
) -> Response {
    let limit = query
        .limit
        .as_deref()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let parts = PgParts(state.db.clone());
    let library = match parts.library_of(part).await {
        Ok(Some(library)) => library,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "likeness library lookup failed"),
    };
    let db = PgLikeness(state.db.clone());
    let shapes = match db.shapes(library).await {
        Ok(shapes) => shapes,
        Err(err) => return internal_error(&err, "likeness profile read failed"),
    };
    let identical = match db.identical(library, part).await {
        Ok(identical) => identical,
        Err(err) => return internal_error(&err, "identical parts read failed"),
    };
    let links = match db.links(library, Some(part)).await {
        Ok(links) => links,
        Err(err) => return internal_error(&err, "part links read failed"),
    };

    let variants: Vec<PartId> = links
        .iter()
        .filter(|link| link.kind == PartLinkKind::Variant)
        .filter_map(|link| link.facing(part))
        .collect();
    // `distinct` is "never propose this pair again": out of the near-duplicates, still reachable
    // through "more like this", which is a way to browse rather than a proposal.
    let decided: HashSet<PartId> = links
        .iter()
        .filter(|link| link.kind == PartLinkKind::Distinct)
        .filter_map(|link| link.facing(part))
        .collect();
    // What another list already shows. `similar` excludes these as well as the near-duplicates, so no
    // card appears twice on the panel.
    let mut listed: HashSet<PartId> = HashSet::from([part]);
    listed.extend(identical.iter().copied());
    listed.extend(variants.iter().copied());

    let subject = shapes.iter().find(|row| row.part == part);
    let alike = subject
        .map(|row| lapidary_db::alike(&row.profile, &shapes, &listed, &decided, limit))
        .unwrap_or_default();

    let identical: Vec<PartId> = identical.into_iter().take(limit).collect();
    let variants: Vec<PartId> = variants.into_iter().take(limit).collect();
    let wanted: Vec<PartId> = identical
        .iter()
        .chain(&alike.near_duplicates)
        .chain(&alike.similar)
        .chain(&variants)
        .copied()
        .collect();
    let cards = match cards_by_id(&parts, library, &wanted, Shows::Live).await {
        Ok(cards) => cards,
        Err(err) => return internal_error(&err, "likeness cards read failed"),
    };
    Json(Likeness {
        profiled: subject.is_some(),
        identical: pick(&cards, &identical),
        near_duplicates: pick(&cards, &alike.near_duplicates),
        similar: pick(&cards, &alike.similar),
        variants: pick(&cards, &variants),
    })
    .into_response()
}

/// `GET /api/libraries/{id}/duplicates?since=` — the review queue.
async fn duplicates(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Query(query): Query<DuplicatesQuery>,
) -> Response {
    // Absent or empty is no filter; anything else must be a timestamp. Quietly widening the answer
    // would report more duplicates than was asked about and read as a change in the library.
    let since = match query.since.as_deref() {
        None | Some("") => None,
        Some(raw) => match raw.parse::<Timestamp>() {
            Ok(at) => Some(at),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "message": "`since` must be a timestamp like 2026-09-21T10:00:00Z. Leave \
                                    it off to see every cluster."
                    })),
                )
                    .into_response();
            }
        },
    };
    match PgParts(state.db.clone()).auto_thumbnail(library).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_library_read(),
        Err(err) => return internal_error(&err, "duplicates library lookup failed"),
    }
    match clusters(&state.db, library, since).await {
        Ok(clusters) => Json(clusters).into_response(),
        Err(err) => internal_error(&err, "duplicates read failed"),
    }
}

/// `PUT /api/parts/{id}/links/{other}` — "these belong together", or "these are not the same".
///
/// `204`: there is nothing to say that the caller does not already know, and the pair's new state is
/// what it just sent. Either part being gone, removed or in another library is the one `404` the rest
/// of the crate uses, for the reason `lifecycle.rs` gives — telling the three apart would confirm that
/// an id names something the caller cannot see.
async fn set_link(
    State(state): State<AppState>,
    Path((part, other)): Path<(PartId, PartId)>,
    Json(body): Json<SetLink>,
) -> Response {
    if body.kind == PartLinkKind::FoldedInto {
        return refused(
            "Folding removes a part, so it is not a link somebody sets. Use the fold action \
             instead — it removes the duplicate and records which part was kept, and Restore \
             undoes it.",
        );
    }
    if part == other {
        return refused(
            "A part cannot be linked to itself. Check which two parts the decision is about.",
        );
    }
    let library = match PgParts(state.db.clone()).library_of(part).await {
        Ok(Some(library)) => library,
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "link library lookup failed"),
    };
    match PgLikeness(state.db.clone())
        .set_link(library, part, other, body.kind)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_part(),
        Err(err) => internal_error(&err, "link write failed"),
    }
}

/// `DELETE /api/parts/{id}/links/{other}` — take the decision back, so the pair can be proposed again.
///
/// `404` when there was no decision to take back, for [`crate::lifecycle::remove`]'s reason: reporting
/// success for a delete that changed nothing would have a client believe it undid something.
async fn remove_link(
    State(state): State<AppState>,
    Path((part, other)): Path<(PartId, PartId)>,
) -> Response {
    match PgLikeness(state.db.clone()).unlink(part, other).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "message": "These two parts have no decision recorded, so nothing was undone. \
                            A part folded into another is brought back with Restore instead."
            })),
        )
            .into_response(),
        Err(err) => internal_error(&err, "link removal failed"),
    }
}

/// `POST /api/parts/{id}/fold` `{ into }` — remove this part as a duplicate of `into`, in one
/// transaction, and record which part was kept.
///
/// `204` for the reason [`set_link`] gives. Folding a part into itself is a `422`: the request is
/// well-formed and names two parts that exist, and there is nothing a `404` would be true about.
async fn fold(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<FoldPart>,
) -> Response {
    match PgLikeness(state.db.clone()).fold(part, body.into).await {
        Ok(Folded::Done) => StatusCode::NO_CONTENT.into_response(),
        Ok(Folded::IntoItself) => refused(
            "A part cannot be folded into itself. Choose the part to keep, then fold the \
             duplicate into it.",
        ),
        Ok(Folded::NoSuchPair) => no_such_part(),
        Err(err) => internal_error(&err, "fold failed"),
    }
}

/// `GET /api/libraries/{id}/folds` — which removed parts were folded, and into what, so the removed
/// page can say where each went.
///
/// A kept part that was itself removed since is still named: the line "folded into X" is true whether
/// or not X is in the library today. A kept part that was *purged* takes its `folded_into` row with it,
/// and the part folded into it is then an ordinary removed part — expected, not an orphan (see
/// `docs/goals/phase-6.md`).
async fn folds(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgParts(state.db.clone()).auto_thumbnail(library).await {
        Ok(Some(_)) => {}
        Ok(None) => return no_such_library_read(),
        Err(err) => return internal_error(&err, "folds library lookup failed"),
    }
    let folds = match PgLikeness(state.db.clone()).folds(library).await {
        Ok(folds) => folds,
        Err(err) => return internal_error(&err, "folds read failed"),
    };
    let parts = PgParts(state.db.clone());
    let kept: Vec<PartId> = folds.iter().map(|(_, into)| *into).collect();
    let mut cards = match cards_by_id(&parts, library, &kept, Shows::Live).await {
        Ok(cards) => cards,
        Err(err) => return internal_error(&err, "folds cards read failed"),
    };
    match cards_by_id(&parts, library, &kept, Shows::Removed).await {
        Ok(removed) => cards.extend(removed),
        Err(err) => return internal_error(&err, "folds cards read failed"),
    }
    let folds: Vec<Fold> = folds
        .into_iter()
        .filter_map(|(part, into)| {
            Some(Fold {
                part,
                into: cards.get(&into)?.clone(),
            })
        })
        .collect();
    Json(folds).into_response()
}

/// A library's duplicate clusters, as the duplicates route and the dashboard's `duplicates` widget (G4)
/// both read them. `since` keeps only the clusters holding a part made at or after it — "these new
/// parts look like parts already here".
///
/// Three reads and one pure function: the library's live parts with the bytes each came from, its
/// current profiles, and its decisions. Nothing is stored, so a decision or a new revision changes the
/// answer with no cache to invalidate.
pub(crate) async fn clusters(
    db: &PgPool,
    library: LibraryId,
    since: Option<Timestamp>,
) -> Result<DuplicateClusters, lapidary_db::DbError> {
    let likeness = PgLikeness(db.clone());
    let live = likeness.parts(library).await?;
    let shapes = likeness.shapes(library).await?;
    let links = likeness.links(library, None).await?;
    let decided: Vec<(PartId, PartId)> = links
        .iter()
        .filter(|link| matches!(link.kind, PartLinkKind::Variant | PartLinkKind::Distinct))
        .map(|link| (link.part, link.other))
        .collect();
    // Live parts the worker has not profiled yet. The page says so rather than implying the library
    // was checked in full.
    let unprofiled = u32::try_from(live.len().saturating_sub(shapes.len())).unwrap_or(u32::MAX);
    let groups = lapidary_db::clusters(&live, &shapes, &decided, since);
    let wanted: Vec<PartId> = groups
        .iter()
        .flat_map(|group| group.parts.iter().copied())
        .collect();
    let cards = cards_by_id(&PgParts(db.clone()), library, &wanted, Shows::Live).await?;
    let clusters = groups
        .into_iter()
        .filter_map(|group| {
            let parts = pick(&cards, &group.parts);
            // A part purged between the two reads leaves a group of one, which is no longer a
            // duplicate of anything.
            if parts.len() < 2 {
                return None;
            }
            Some(DuplicateCluster {
                parts,
                identical: group.identical,
            })
        })
        .collect();
    Ok(DuplicateClusters {
        clusters,
        unprofiled,
    })
}

/// The cards for these parts, by id. One query for every list on a response, because each card carries
/// an inline thumbnail and asking per list would read the same rows twice.
async fn cards_by_id(
    parts: &PgParts,
    library: LibraryId,
    ids: &[PartId],
    shows: Shows,
) -> Result<HashMap<PartId, PartCard>, lapidary_db::DbError> {
    Ok(parts
        .rows_by_id(library, ids, shows)
        .await?
        .into_iter()
        .map(to_card)
        .map(|card| (card.id, card))
        .collect())
}

/// The cards for `ids`, in that order. An id with no card — purged since it was ranked, or on the other
/// side of `shows` — is left out rather than turned into a hole in the list.
fn pick(cards: &HashMap<PartId, PartCard>, ids: &[PartId]) -> Vec<PartCard> {
    ids.iter().filter_map(|id| cards.get(id).cloned()).collect()
}

/// A request that is well-formed and names parts that exist, and still cannot be carried out.
fn refused(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}

/// A library id that names nothing, on a read. `derive.rs`'s write-side wording says "nothing was
/// changed", which is not an answer a reader asked for.
fn no_such_library_read() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No library with that id exists, so it has no duplicates to show. Check \
                        the id against the library list."
        })),
    )
        .into_response()
}

/// This file's routes, merged for the api role.
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/parts/{id}/likeness", get(likeness))
        .route("/api/parts/{id}/fold", post(fold))
        .route(
            "/api/parts/{id}/links/{other}",
            put(set_link).delete(remove_link),
        )
        .route("/api/libraries/{id}/duplicates", get(duplicates))
        .route("/api/libraries/{id}/folds", get(folds))
}
