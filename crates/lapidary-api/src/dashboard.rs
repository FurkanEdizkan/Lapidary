//! The dashboard (Phase 6; design in `docs/goals/phase-6.md`): what a widget is, and the one route that answers
//! all of them, `POST /api/dashboard/resolve` (goal G4).
//!
//! [`Widget`] is the dashboard's config schema: the web keys its widget registry by this enum's `kind`, so a
//! kind added here fails the web's type check until the web knows how to draw it. A layout is stored per browser
//! (until Phase 8 brings users), and opening the dashboard costs one resolve. There is no per-widget endpoint and
//! no polling: a widget is asked again when the event stream says its library changed.

use crate::AppState;
use crate::parts::{FacetValue, InstanceStorageView, LibraryStorage, PartCard};
use axum::Router;
use lapidary_core::{LibraryId, SavedFilterId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One widget, as a layout stores it and a resolve asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Widget {
    /// What one library costs on disk: [`LibraryStorage`].
    Storage { library: LibraryId },
    /// What the whole installation costs, the storage page's figures: [`InstanceStorageView`].
    InstanceStorage,
    /// A library's newest parts, at most 12.
    Recent { library: LibraryId, limit: u8 },
    /// The parts one saved filter finds, at most 12.
    SavedFilter {
        library: LibraryId,
        filter: SavedFilterId,
        limit: u8,
    },
    /// The commonest values of one facet in a library.
    Facet {
        library: LibraryId,
        facet: FacetKind,
        limit: u8,
    },
    /// A library's work in the queue.
    Queue { library: LibraryId },
    /// How many groups of near-duplicates a library holds, waiting for a person to decide.
    Duplicates { library: LibraryId },
}

/// Which facet a [`Widget::Facet`] counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FacetKind {
    Format,
    Material,
    Tag,
}

/// One widget asked for, under the key the layout knows it by.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct WidgetRequest {
    pub key: String,
    pub widget: Widget,
}

/// The `POST /api/dashboard/resolve` body: 1 to 32 widgets, each key once. Anything else is refused whole (422),
/// so a runaway layout cannot hold the database's connections.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ResolveRequest {
    pub widgets: Vec<WidgetRequest>,
}

/// The answer: one [`KeyResult`] per key, in the order asked. Always 200 once the body is valid — a widget
/// that failed or ran out of time says so in its own result, and the others still arrive.
#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ResolveResponse {
    pub results: Vec<KeyResult>,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyResult {
    pub key: String,
    pub result: WidgetResult,
}

/// How one widget came out.
#[derive(Debug, Serialize, TS)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum WidgetResult {
    Ok {
        value: WidgetValue,
    },
    /// It did not answer within its 2 seconds, counted from when it started rather than from when it was asked.
    TimedOut,
    /// It could not be answered: a library or saved filter that does not exist, or a query that failed. The
    /// message says which, in words a person can act on.
    Failed {
        message: String,
    },
}

/// A widget's value, tagged by the same `kind` as the [`Widget`] it answers.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
#[ts(export)]
pub enum WidgetValue {
    Storage(LibraryStorage),
    InstanceStorage(InstanceStorageView),
    Recent(Vec<PartCard>),
    SavedFilter(FilteredParts),
    Facet(Vec<FacetValue>),
    Queue(QueueSummary),
    Duplicates(DuplicateSummary),
}

/// A saved filter's parts, with its name as it is now — renamed since the layout was saved, it shows the new
/// one.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FilteredParts {
    pub name: String,
    pub parts: Vec<PartCard>,
}

/// A library's jobs that are not finished, and the ones that failed.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueueSummary {
    pub pending: u32,
    pub running: u32,
    pub failed: u32,
}

/// The duplicates review queue in brief; the duplicates page holds the groups themselves.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DuplicateSummary {
    pub clusters: u32,
    /// Parts with no shape profile yet, so not compared.
    pub unprofiled: u32,
}

/// This file's routes, merged for the api role. Empty until goal G4.
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
}
