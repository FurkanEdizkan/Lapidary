//! The grid: listing parts in a library.
//! `GET /api/libraries/{id}/parts?folderId=&after=&limit=`, `api` role only. The open
//! path's main read — this is what the grid renders from — and it reads metadata and
//! derivatives only, never a source file and never the CAD
//! kernel (structurally: this crate cannot link `lapidary-cad`, see `lib.rs`). The
//! storage figures on a card are `file` and `blob` rows — how large a source file is
//! and how it was stored — and reading a row about a file is not opening one; nothing
//! here ever asks the blob store for bytes.
//!
//! `GET /api/libraries/{id}/storage` is the same figures summed over the library, and it
//! lives here rather than in `derive.rs` because it is the per-card storage line's total,
//! not a trigger or a setting. It reads rows too, and no bytes.
//!
//! `after=` with nothing after the `=` is not a client bug: it is the literal shape of
//! `` `…/parts?after=${cursor ?? ''}&limit=${n}` ``, the natural way to build this URL
//! before a cursor exists, and this handler treats it the same as `after` being absent
//! entirely rather than rejecting it as an invalid id.

use crate::AppState;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use lapidary_core::{BlobHash, FolderId, LibraryId, PartId, RevisionId};
use lapidary_db::{DbError, GridQuery, PartRepository, PartRow, PgParts, Shows, Sort};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Page size when the query string omits `limit`.
const DEFAULT_LIMIT: u16 = 50;

/// The hard ceiling on `limit`, regardless of what the query string asks for. Trusting
/// an unbounded `limit` is a trivial way to make one request materialise an entire
/// library — thumbnails and all — into memory.
///
/// Raised from 100 to 500 so the grid can offer the page sizes `FEATURES.md` names:
/// 50, 100, 250 and 500. The ceiling is still a ceiling — 500 is a number somebody chose,
/// not the absence of one — and it is what bounds the cost of a single request.
///
/// **What 500 actually costs, since the point of the limit is the memory:** a page carries
/// its thumbnails inline as `data:` URLs, and a thumbnail is capped at `MAX_THUMB_BYTES`
/// (64 KB) by the rasterizer. So the worst case is around 32 MB of preview bytes in one
/// response, base64'd to roughly 43 MB on the wire — inside the api container's 512 MB
/// limit, and slow enough on a bad connection that it is a choice a user makes rather than
/// a default they get.
const MAX_LIMIT: u16 = 500;

/// One grid card — the wire shape, deliberately not `PartSummary`.
///
/// `PartSummary.thumbnail` is `Option<BlobHash>`: a content hash for a hash-addressed
/// thumbnail endpoint that arrives with the viewer. Slice 1 has no such endpoint — it
/// stores the WebP inline as `bytea` so a grid page costs one query, not a round trip
/// per card — so this type carries the decoded bytes themselves, as a `data:` URL a
/// browser can drop straight into an `<img src>`. Widening `PartSummary` instead would
/// put a transport concern into a domain type the viewer will reuse differently.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartCard {
    pub id: PartId,
    pub library: LibraryId,
    /// The revision this card's numbers describe, and the one its download link names.
    /// Carried verbatim from `PartSummary.revision` — see there for why the frontend
    /// must not resolve "latest" a second time of its own.
    pub revision: RevisionId,
    pub name: String,
    pub part_number: Option<String>,
    /// The path this part is known by — its identity since slice 6a, carried verbatim
    /// from `PartSummary`. The removed list names a part by this in its purge
    /// confirmation, because two parts can share a name and only one of them is the one
    /// being destroyed.
    pub source_path: String,
    /// `data:image/webp;base64,<...>`. `None` when the part's latest revision has no
    /// thumbnail derivative.
    pub thumbnail: Option<String>,
    pub triangle_count: Option<u32>,
    /// True when *any* geometric figure on this part is mesh-derived — carried
    /// verbatim from `PartSummary.approximate`, not narrowed to "every". Every figure
    /// on a slice-1 (STL-only) part is tessellated, so this reads `true` today, but the
    /// meaning must stay "any" for when analytic B-rep figures arrive alongside mesh
    /// ones on the same part.
    pub approximate: bool,
    /// The source file's hash, `None` when the revision has no source row. Rendered as
    /// a short hash beside the download link so a user can check what they got against
    /// what the card claimed (`DATA.md` §5.1) — holding it is not authorization to read
    /// it, exactly as `PartSummary.thumbnail` is not.
    pub source_hash: Option<BlobHash>,
    /// The L0 tessellation's hash, carried verbatim from `PartSummary` for the reason
    /// `source_hash` is: it is the only thing that makes those bytes addressable, and
    /// `GET /api/blob/{blake3}` had no possible caller without it. Holding it is not
    /// authorization — that route checks reachability before serving.
    pub tessellation_l0: Option<BlobHash>,
    /// Ingested size and size on disk, from `PartSummary`. `number | null`, not
    /// `bigint`, for the reason given there: it is what serde puts on the wire.
    #[ts(type = "number | null")]
    pub source_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub stored_bytes: Option<u64>,
    /// Whether the stored bytes are a zstd frame. `Some(false)` covers both "stored
    /// raw" and "level unrecorded" — see `PartSummary.compressed`.
    pub compressed: Option<bool>,
    /// The model's own directory in the store, relative to the storage root:
    /// `libraries/default/Terrain/rock`. Shown, never opened — no browser can navigate a
    /// `file://` URL from a page, and the api runs in a container where an absolute path
    /// would name a filesystem the user is not looking at. The Tauri shell is what will
    /// eventually have a host to ask.
    ///
    /// Kept beside `storagePath` below rather than derived from it, because the two answer
    /// different questions and one of them is a *move* target: this is what the move route
    /// renames, and what the card offers to move. Deriving it in the client would put a
    /// second definition of "the parent of a model file" in TypeScript, next to
    /// `model_directory`'s in Rust.
    ///
    /// `None` is a real state and not a missing value — the part predates the folder layout
    /// and its bytes are still content-addressed, so it has no directory to show and cannot
    /// be moved until `migrate_storage` reaches it. The card says so rather than offering a
    /// move that the route would refuse.
    pub directory: Option<String>,
    /// The model's file, path and all: `libraries/default/Terrain/rock/rock.stl`.
    ///
    /// The `directory` above with the filename back on, and the client cannot reconstruct
    /// it: `model_dir_for` disambiguates a colliding model name — the second `cliff` becomes
    /// `cliff_a1b2c3` — so a path joined from a part's name would be confidently wrong
    /// exactly where a person is most likely to be looking for it.
    ///
    /// Shown, never opened. No browser navigates a `file://` URL from a page, so this is
    /// selectable text; and prefixed with `InstanceStorageView::host_storage_root` when the
    /// deployment has said where the store is, which is what makes it a path somebody can
    /// paste into a file manager rather than one they have to work out.
    ///
    /// `None` alongside `directory`, and for the same reason.
    pub storage_path: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A keyset page of the grid.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartsPage {
    pub parts: Vec<PartCard>,
    /// Pass back as `after` to fetch the next page. `None` when this page was not
    /// full — a short page proves there is nothing left, so there is no id to hand
    /// back that would not just fetch another empty page.
    pub next: Option<PartId>,
}

/// What a library costs, the library-level half of the figures on every card.
///
/// Both totals are bytes on disk after compression, deduplicated — see
/// `PgParts::storage_totals`, which is where the accounting is written down. `number`,
/// not `bigint`: serde puts a JSON number on the wire, and ts-rs 12 would otherwise
/// promise the frontend something `JSON.parse` never produces.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryStorage {
    #[ts(type = "number")]
    pub source_bytes: u64,
    #[ts(type = "number")]
    pub derivative_bytes: u64,
    /// Derivative bytes ÷ source bytes. `None` for a library holding no source bytes,
    /// where the division has no answer — a library with nothing in it, and a `0` there
    /// would read as "derivatives cost nothing", which is a different claim.
    ///
    /// Sent rather than left to the client because the direction is the whole meaning:
    /// this is the figure that made slice 4's 92.5% drop legible (spec §4), and a second
    /// consumer dividing the other way would report the same library twice, differently.
    pub derivative_ratio: Option<f64>,
    /// What this library's removed parts still occupy on the volume.
    ///
    /// Shown only when non-zero. It exists so the panel does not report a removal as a
    /// saving: the two totals above exclude soft-deleted parts, so without this figure the
    /// number falls the moment somebody removes a part and the disk does not.
    ///
    /// Quarantined bytes are deliberately absent and are not merely missing — see
    /// `PgParts::storage_totals` for why a quarantined blob has no library to be counted
    /// against.
    #[ts(type = "number")]
    pub removed_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    /// The previous page's last id, or absent/empty for the first page.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    after: Option<PartId>,
    /// One category, **and everything under it**. Absent is the whole library, which is
    /// what the sidebar's "All models" row selects — there is no id meaning "no category",
    /// so dropping the parameter is how the client says that.
    ///
    /// Subtree-inclusive is the route's own promise, not the client's: a client cannot
    /// walk the tree and send a list without racing every move and rename in flight, and
    /// two clients doing it would disagree. See `PartRepository::page`.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    folder_id: Option<FolderId>,
    /// Parsed as `i64`, not `u16`: the query string is untrusted text, and a value
    /// like `100000` must reach the `clamp` below and come out as `MAX_LIMIT`, not
    /// fail deserialization because it does not fit a 16-bit type before the clamp
    /// ever runs — `u16`'s serde impl rejects an out-of-range number outright rather
    /// than saturating.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    limit: Option<i64>,
    /// `?state=removed` lists the parts a person deleted and has not purged; anything
    /// else, including absent, lists the library.
    ///
    /// A string rather than `?removed=true`, because this is the axis a third value joins
    /// when Phase 2 adds revision states — `state=draft` reads as one more value of a
    /// dimension, where `draft=true` beside `removed=true` reads as two independent
    /// booleans that can both be set and mean nothing together.
    ///
    /// An unknown value shows the library rather than failing. The grid is a read, the
    /// wrong answer is visible immediately, and 400ing a typo'd query string would take a
    /// bookmark that used to work and turn it into an error page.
    #[serde(default)]
    state: Option<String>,
    /// Free text to search for. Absent, empty or whitespace is the unfiltered grid.
    ///
    /// A parameter on this route rather than a `/search` route of its own. A separate one
    /// would duplicate the limit clamp, `empty_str_as_none`, the `Shows` mapping, the
    /// folder filter, `to_card` and the `next` computation — and hand the front end a
    /// second `useInfiniteQuery` to keep in step with the first. It answers the same
    /// question about the same library and returns the same cards.
    ///
    /// Empty means absent for the same reason `after` does: `` `…&q=${term}` `` is the
    /// natural shape of a URL before anything is typed, and 400ing a bookmark somebody
    /// cleared the box on would be answering a read with an error page.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    q: Option<String>,
    /// One source format, as ingest records it: the extension, lowercased. Absent is every
    /// format.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    format: Option<String>,
    /// One material, exactly as the file names it. Absent is every material.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    material: Option<String>,
    /// One tag, exactly as a person wrote it. Absent is every tag.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    tag: Option<String>,
    /// One custom field of this library, offered as a filter, and the value it must hold
    /// (`docs/DATA.md` §3.5). Both or neither: either alone is no filter.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field_value: Option<String>,
    /// With `field`, a number field's range in place of a value: either bound, or both.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field_min: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field_max: Option<String>,
    /// `volume`, `surface_area`, `longest_side` or `triangles`, largest first. Absent — or
    /// anything else, for the reason `state` gives — is newest first. A search ignores it:
    /// relevance is a search's order.
    #[serde(default)]
    sort: Option<String>,
}

/// Treats an empty query-string value the same as an absent key. `#[serde(default)]`
/// alone only covers the key being missing entirely — a key that is present with
/// nothing after the `=` still reaches the field's own deserializer as the empty
/// string, and `T::from_str("")` rejects it for every `T` this endpoint uses (`PartId`,
/// `i64`), which is what turned the documented `after=&limit=` URL shape into a 400.
fn empty_str_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let raw = String::deserialize(deserializer)?;
    if raw.is_empty() {
        Ok(None)
    } else {
        raw.parse().map(Some).map_err(serde::de::Error::custom)
    }
}

/// `GET /api/libraries/{id}/parts?after=&limit=` — the grid's one read. Keyset paged,
/// newest first, thumbnails inline.
pub async fn page(
    State(app): State<AppState>,
    Path(library): Path<LibraryId>,
    query: Result<Query<PageQuery>, QueryRejection>,
) -> Response {
    let PageQuery {
        after,
        folder_id,
        limit,
        state,
        q,
        format,
        material,
        tag,
        field,
        field_value,
        field_min,
        field_max,
        sort,
    } = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return bad_query(&rejection),
    };

    // Zero would ask PgParts::page for a LIMIT 0 query and then always report `next:
    // null` (a short page, by definition) even though more rows exist — clamp the
    // bottom as well as the top. The clamp runs on the widened `i64` so an
    // out-of-range value (too large *or* negative) lands inside [1, MAX_LIMIT] instead
    // of failing to parse; the cast back to `u16` afterward is safe because the value
    // is now guaranteed to fit.
    let limit = limit
        .unwrap_or(i64::from(DEFAULT_LIMIT))
        .clamp(1, i64::from(MAX_LIMIT)) as u16;

    let shows = if state.as_deref() == Some("removed") {
        Shows::Removed
    } else {
        Shows::Live
    };

    // Trimmed, then checked for emptiness: a box holding three spaces is a box nobody has
    // typed in, and searching for them would answer an empty grid to somebody who thinks
    // they cleared it.
    let query = q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let format = format
        .map(|format| format.trim().to_ascii_lowercase())
        .filter(|format| !format.is_empty());
    // Trimmed and nothing else: a material is named as its file names it, capitals included.
    let material = material
        .map(|material| material.trim().to_owned())
        .filter(|material| !material.is_empty());
    let tag = tag
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty());
    let field = match crate::fields::filter_of(
        &app.db,
        library,
        field.as_deref(),
        field_value.as_deref(),
        field_min.as_deref(),
        field_max.as_deref(),
    )
    .await
    {
        Ok(field) => field,
        Err(refusal) => return refusal,
    };
    let grid = GridQuery {
        library,
        folder: folder_id,
        after,
        limit,
        shows,
        format: format.as_deref(),
        material: material.as_deref(),
        tag: tag.as_deref(),
        field: field.exact.as_deref(),
        field_range: field.range.as_deref(),
    };
    let sort = sort
        .as_deref()
        .and_then(Sort::parse)
        .unwrap_or(Sort::Newest);
    let repository = PgParts(app.db);
    let result = match query {
        Some(q) => repository.search(&grid, q).await,
        None => repository.page(&grid, sort).await,
    };
    match result {
        Ok(rows) => {
            // A page shorter than `limit` proves there is no further page. A full page
            // might or might not be the last one, so it hands back the last id and lets
            // the next request find out.
            let next = if rows.len() == usize::from(limit) {
                rows.last().map(|row| row.summary.id)
            } else {
                None
            };
            let parts = rows.into_iter().map(to_card).collect();
            Json(PartsPage { parts, next }).into_response()
        }
        Err(err) => internal_error(&err, "grid page query failed"),
    }
}

/// One source format, and how many of the grid's parts carry it. `count` is `null` past the
/// exact-count threshold, where only the values that occur are shown.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FacetValue {
    pub value: String,
    #[ts(type = "number | null")]
    pub count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Facets {
    pub formats: Vec<FacetValue>,
    /// The materials the parts' files declare. Empty for a library of meshes, which declare none.
    pub materials: Vec<FacetValue>,
    /// The tags people gave the parts. Empty until somebody tags one.
    pub tags: Vec<FacetValue>,
    /// Each choice field offered as a filter, with how many of the parts hold each of its values. A field
    /// filter never narrows its own field's counts.
    pub fields: Vec<FieldFacet>,
}

/// One choice field's values among the grid's parts, each as a [`FacetValue`]. A value no part holds is left
/// out.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldFacet {
    pub key: String,
    pub values: Vec<FacetValue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetQuery {
    #[serde(default, deserialize_with = "empty_str_as_none")]
    folder_id: Option<FolderId>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    q: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    format: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    material: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    tag: Option<String>,
    /// One custom field of this library, offered as a filter, and the value it must hold
    /// (`docs/DATA.md` §3.5). Both or neither: either alone is no filter.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field_value: Option<String>,
    /// With `field`, a number field's range in place of a value: either bound, or both.
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field_min: Option<String>,
    #[serde(default, deserialize_with = "empty_str_as_none")]
    field_max: Option<String>,
}

/// `GET /api/libraries/{id}/facets` — the formats, materials and tags among the parts `page`
/// would show for the same category, query and state. Each list is narrowed by the others' choices
/// and never by its own: counts that obeyed their own choice would show every other value as zero
/// and nobody could choose a second one, while counts that ignored the other choice would offer
/// parts the grid is not showing.
pub async fn facets(
    State(app): State<AppState>,
    Path(library): Path<LibraryId>,
    query: Result<Query<FacetQuery>, QueryRejection>,
) -> Response {
    let FacetQuery {
        folder_id,
        state,
        q,
        format,
        material,
        tag,
        field,
        field_value,
        field_min,
        field_max,
    } = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return bad_query(&rejection),
    };
    // The field a field filter is on, whose own counts it never narrows.
    let filtered = field
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_owned);
    let field = match crate::fields::filter_of(
        &app.db,
        library,
        field.as_deref(),
        field_value.as_deref(),
        field_min.as_deref(),
        field_max.as_deref(),
    )
    .await
    {
        Ok(field) => field,
        Err(refusal) => return refusal,
    };
    let shows = if state.as_deref() == Some("removed") {
        Shows::Removed
    } else {
        Shows::Live
    };
    let q = q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let format = format
        .map(|format| format.trim().to_ascii_lowercase())
        .filter(|format| !format.is_empty());
    let material = material
        .map(|material| material.trim().to_owned())
        .filter(|material| !material.is_empty());
    let tag = tag
        .map(|tag| tag.trim().to_owned())
        .filter(|tag| !tag.is_empty());
    let parts = PgParts(app.db);
    let values = |values: Vec<lapidary_db::FacetValue>| {
        values
            .into_iter()
            .map(|value| FacetValue {
                value: value.value,
                count: value.count,
            })
            .collect()
    };
    let formats = match parts
        .format_facet(
            library,
            folder_id,
            q,
            shows,
            material.as_deref(),
            tag.as_deref(),
            field.exact.as_deref(),
            field.range.as_deref(),
        )
        .await
    {
        Ok(formats) => formats,
        Err(err) => return internal_error(&err, "facet query failed"),
    };
    let materials = match parts
        .material_facet(
            library,
            folder_id,
            q,
            shows,
            format.as_deref(),
            tag.as_deref(),
            field.exact.as_deref(),
            field.range.as_deref(),
        )
        .await
    {
        Ok(materials) => materials,
        Err(err) => return internal_error(&err, "facet query failed"),
    };
    let tags = match parts
        .tag_facet(
            library,
            folder_id,
            q,
            shows,
            format.as_deref(),
            material.as_deref(),
            field.exact.as_deref(),
            field.range.as_deref(),
        )
        .await
    {
        Ok(tags) => tags,
        Err(err) => return internal_error(&err, "facet query failed"),
    };
    // The choice fields offered as filters: the only fields whose values are a list to count.
    let keys: Vec<String> = match lapidary_db::PgCustomFields(parts.0.clone())
        .list(library)
        .await
    {
        Ok(defined) => defined
            .into_iter()
            .filter(|one| one.indexed && one.kind == "choice")
            .map(|one| one.key)
            .collect(),
        Err(err) => return internal_error(&err, "custom field list failed"),
    };
    let mut counted = if keys.is_empty() {
        std::collections::BTreeMap::new()
    } else {
        match parts
            .choice_facet(
                library,
                folder_id,
                q,
                shows,
                format.as_deref(),
                material.as_deref(),
                tag.as_deref(),
                field.exact.as_deref(),
                field.range.as_deref(),
                filtered.as_deref(),
                &keys,
            )
            .await
        {
            Ok(counted) => counted,
            Err(err) => return internal_error(&err, "facet query failed"),
        }
    };
    Json(Facets {
        formats: values(formats),
        materials: values(materials),
        tags: values(tags),
        fields: keys
            .into_iter()
            .map(|key| FieldFacet {
                values: values(counted.remove(&key).unwrap_or_default()),
                key,
            })
            .collect(),
    })
    .into_response()
}

/// `GET /api/libraries/{id}/storage` — source total, derivative total, and the ratio.
///
/// A library that does not exist is a `404`, as it is on every other library route. The
/// grid deliberately answers an unknown id with an empty page — an empty library and an
/// id that names nothing look alike to someone browsing — but a storage panel reporting
/// `0 B` for a mistyped id is a number a person would believe.
pub async fn storage(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgParts(state.db).storage_totals(library).await {
        Ok(Some(totals)) => Json(LibraryStorage {
            source_bytes: totals.source_bytes,
            derivative_bytes: totals.derivative_bytes,
            removed_bytes: totals.removed_bytes,
            // Both casts are lossless below 2^53 bytes, which is 9 petabytes in one
            // library; a ratio is a display figure and does not need more than that.
            derivative_ratio: (totals.source_bytes > 0)
                .then(|| totals.derivative_bytes as f64 / totals.source_bytes as f64),
        })
        .into_response(),
        Ok(None) => no_such_library(),
        Err(err) => internal_error(&err, "library storage query failed"),
    }
}

/// What the whole store holds, and where it is.
///
/// The question `LibraryStorage` cannot answer. That one is per library and deliberately
/// **not** `du`: it charges a blob two libraries share to both of them, and it can show
/// quarantined bytes to nobody because a quarantined blob is keyed by hash and the part
/// that said which library it belonged to is the part that was purged. So a person adding
/// up the panels and comparing the result to their disk finds a discrepancy, and
/// `PgParts::storage_totals`'s own doc predicts it: *"someone will eventually run `du`."*
///
/// This is the answer to that, and it reports both halves rather than picking one:
/// `tracked*` is what the database knows, exactly and instantly; `on_disk_bytes` is a real
/// walk of the root, which is the number `du` gives. Neither is wrong and they do not
/// agree, and the difference is what a person actually wants to see.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStorageView {
    /// Every live model file across every library. One file per model, no deduplication.
    #[ts(type = "number")]
    pub source_bytes: u64,
    /// Rungs **on the storage volume**, each blob counted once however many libraries point
    /// at it — where the per-library figure charges it to each of them in full.
    #[ts(type = "number")]
    pub derivative_bytes: u64,
    /// Thumbnails, which are in Postgres and not on the volume.
    ///
    /// Separate so that the figures above can be compared with `on_disk_bytes` and add up.
    /// Folded in, the tracked total exceeds a walk of the storage root by exactly this
    /// amount — measured at 5,745,760 bytes on the library this was written against — and
    /// a panel reporting more tracked than present reads as bytes having gone missing.
    #[ts(type = "number")]
    pub inline_preview_bytes: u64,
    /// Soft-deleted parts. On the disk, and back the moment somebody restores them.
    #[ts(type = "number")]
    pub removed_bytes: u64,
    /// Purged and inside the thirty-day hold. Bytes no per-library panel can admit to.
    #[ts(type = "number")]
    pub quarantined_bytes: u64,
    /// Rendered previews nobody has opened in 90 days, which "free cache space" would put into
    /// quarantine. Lapidary rebuilds each one when its part is next opened.
    #[ts(type = "number")]
    pub render_cache_bytes: u64,
    /// A real walk of the storage root, or `None` when one was not asked for.
    ///
    /// Behind `?onDisk=true` because it costs a `stat` per file: instant on the 156-part
    /// library this was written against, seconds on a corpus. The default answer is the
    /// one that is free.
    #[ts(type = "number | null")]
    pub on_disk_bytes: Option<u64>,
    /// Where the store is **on the host**, when the deployment has said.
    ///
    /// `None` unless `LAPIDARY_HOST_STORAGE_ROOT` names an absolute path, and the reason is
    /// that this process genuinely cannot work it out. The api sees the store at
    /// `/var/lib/lapidary`, which is a bind mount and a path that exists nowhere on the
    /// machine the user is sitting at. Reporting it would be worse than reporting nothing:
    /// they would paste it into a file manager and find no such directory.
    pub host_storage_root: Option<String>,
}

/// `GET /api/storage` — the instance total, and where the store is.
///
/// Instance-wide and so not under `/api/libraries/{id}`: two of the four figures below
/// belong to no library, and the derivative figure is deliberately *not* what you get by
/// adding the libraries up.
pub async fn instance_storage(
    State(state): State<AppState>,
    Query(query): Query<InstanceStorageQuery>,
) -> Response {
    // The render cache figure reads `last_accessed_at`, so the reads not yet written go first.
    if let Err(err) = state.touches.flush(&state.db).await {
        tracing::warn!(
            error = %err,
            "could not record recent reads; the render cache figure may count parts opened in the last five minutes"
        );
    }
    let totals = match PgParts(state.db).instance_storage().await {
        Ok(totals) => totals,
        Err(err) => return internal_error(&err, "instance storage query failed"),
    };

    // Walked before the response is built and only when asked. A failure here is not a
    // failure of the route: the four tracked figures are still true, and an operator whose
    // storage root has become unreadable is better served by seeing them plus a missing
    // walk than by a 500 that hides them.
    let on_disk = query.on_disk.unwrap_or(false).then(|| {
        walk_bytes(&state.blob_root).unwrap_or_else(|err| {
            tracing::warn!(
                root = %state.blob_root.display(),
                error = %err,
                "could not walk the storage root for an on-disk total; reporting the \
                 tracked figures without it"
            );
            None
        })
    });

    Json(InstanceStorageView {
        source_bytes: totals.source_bytes,
        derivative_bytes: totals.derivative_bytes,
        inline_preview_bytes: totals.inline_preview_bytes,
        removed_bytes: totals.removed_bytes,
        quarantined_bytes: totals.quarantined_bytes,
        render_cache_bytes: totals.render_cache_bytes,
        on_disk_bytes: on_disk.flatten(),
        host_storage_root: state.host_storage_root.clone(),
    })
    .into_response()
}

/// What `POST /api/storage/render-cache` answers: the rungs it removed, and the bytes that entered
/// the thirty-day quarantine. Named for what happened, never "freed".
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RenderCacheFreedView {
    #[ts(type = "number")]
    pub removed: u64,
    #[ts(type = "number")]
    pub quarantined_bytes: u64,
}

/// `POST /api/storage/render-cache` — "free cache space" (`DATA.md` §1.5). Instance-wide, as
/// the figure it acts on is.
pub async fn free_render_cache(State(state): State<AppState>) -> Response {
    // Reads this process has not written yet are reads all the same: a part opened a moment ago must
    // not lose its previews to when the next flush happens to be due.
    if let Err(err) = state.touches.flush(&state.db).await {
        return internal_error(
            &err,
            "recording recent reads before freeing the render cache failed",
        );
    }
    match PgParts(state.db).free_render_cache().await {
        Ok(freed) => Json(RenderCacheFreedView {
            removed: freed.rungs,
            quarantined_bytes: freed.quarantined_bytes,
        })
        .into_response(),
        Err(err) => internal_error(&err, "freeing the render cache failed"),
    }
}

/// `?onDisk=true` asks for the walk. Anything else, including absent, does not.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStorageQuery {
    #[serde(default)]
    on_disk: Option<bool>,
}

/// Every byte under `root`, following no symlinks.
///
/// `metadata` and not `symlink_metadata` would follow a link out of the store and count a
/// file that is not ours — or loop. This walks what is actually there, which is the whole
/// point of the figure: it is the one number that includes `metadata.json`, a stray file
/// somebody dropped in, and anything a crash left behind.
///
/// Returns `Ok(None)` for a root that is not there yet, which is a fresh install rather
/// than a fault.
fn walk_bytes(root: &std::path::Path) -> std::io::Result<Option<u64>> {
    if !root.exists() {
        return Ok(None);
    }
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(Some(total))
}

/// The storage route's `404`. Its own message rather than `derive.rs`'s: that one tells a
/// writer nothing was changed, which is an answer to a question a reader did not ask.
fn no_such_library() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "message": "No library with that id exists, so there is nothing stored under \
                        it. Check the id against the library list."
        })),
    )
        .into_response()
}

/// The query string failed to parse — a malformed (non-empty) `after` or a `limit`
/// that isn't a number. axum's default rejection body is a bare, unstructured line of
/// text; wrap it so the response says what broke and what a client can send instead.
fn bad_query(rejection: &QueryRejection) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "message": format!(
                "Could not read the query string: {rejection}. `after` must be a part id \
                 from a previous page (or omitted/empty for the first page); `folderId` \
                 must be a category id from this library's folder tree (or omitted for the \
                 whole library); `limit` must be a whole number; `q` is free text and is \
                 never a reason to land here."
            )
        })),
    )
        .into_response()
}

/// The query itself failed. Every `DbError` variant already carries its own
/// operator-facing remedy via `Display` — appending fixed connectivity advice on top,
/// as this handler once did, points a `TimestampOutOfRange` (a corrupt row, nothing to
/// do with connectivity) at the wrong system. `client_message` is what decides which
/// variants' text is safe to hand back verbatim; the ones that are not (an upstream
/// `sqlx`/migration error this crate did not compose) get a generic message here while
/// the real detail still reaches the operator, through the log line below rather than
/// the response body — the same asymmetry `health::healthz` already keeps by never
/// putting a live error's text in its response at all.
fn internal_error(err: &DbError, what: &'static str) -> Response {
    tracing::error!(error = %err, "{what}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": err.client_message() })),
    )
        .into_response()
}

fn to_card(row: PartRow) -> PartCard {
    let summary = row.summary;
    PartCard {
        id: summary.id,
        library: summary.library,
        revision: summary.revision,
        name: summary.name,
        part_number: summary.part_number,
        source_path: summary.source_path,
        thumbnail: row
            .thumbnail_webp
            .map(|bytes| format!("data:image/webp;base64,{}", BASE64.encode(bytes))),
        triangle_count: summary.triangle_count,
        approximate: summary.approximate,
        source_hash: summary.source_hash,
        tessellation_l0: summary.tessellation_l0,
        source_bytes: summary.source_bytes,
        stored_bytes: summary.stored_bytes,
        compressed: summary.compressed,
        directory: row.directory,
        storage_path: row.storage_path,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
    }
}
