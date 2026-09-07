//! Where a part came from: the link, the vendor, the price, and the licence.
//!
//! `FEATURES.md` §6's other half, and the one `docs/DATA.md` is most insistent about:
//!
//! > `part_source.license` is not bureaucracy — half of hobbyist STL libraries are
//! > non-commercial, and a user selling prints needs to see that on the card.
//!
//! **It is not on the grid card, and that is a decision rather than an omission.** The grid
//! query is at its 16-column `FromRow` ceiling and a licence is a string per part; adding it
//! would mean reworking the tuple that every other column on a card already fits inside, to
//! put a sentence on a tile a person is scanning at a glance. It is on the detail page and
//! in the quick-look dialog — one click from the card, and where somebody deciding whether
//! to print a thing is actually looking.
//!
//! Free text and not an enum, matching the column: the licences in the wild do not fit one,
//! and refusing to record `CC-BY-NC-SA 4.0` because it is not in our list would make the
//! field useless for the case it exists for.

use crate::AppState;
use crate::derive::internal_error;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{PartId, PartSourceId};
use lapidary_db::PgParts;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/parts/{id}/sources` — every source for one part, oldest first.
pub async fn list(State(state): State<AppState>, Path(part): Path<PartId>) -> Response {
    match PgParts(state.db).part_sources(part).await {
        Ok(rows) => Json(
            rows.into_iter()
                .map(|row| PartSource {
                    id: row.id,
                    url: row.url,
                    vendor: row.vendor,
                    external_id: row.external_id,
                    title: row.title,
                    license: row.license,
                    price_minor: row.price_minor,
                    currency: row.currency,
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(err) => internal_error(&err, "reading a part's sources failed"),
    }
}

/// `POST /api/parts/{id}/sources` — record where a part came from.
///
/// **The URL is not fetched.** This route stores what it is given; the only thing in this
/// application that makes an outbound request is `images::from_url`, and keeping it that way
/// is what makes the one security boundary in the codebase findable. Pasting a product page
/// here records the link — it does not go and read it.
pub async fn create(
    State(state): State<AppState>,
    Path(part): Path<PartId>,
    Json(body): Json<NewSource>,
) -> Response {
    // A price is money and a currency says which money. The database has the same opinion —
    // `part_source_price_has_currency` — and checking here is what turns it into a sentence.
    if body.price_minor.is_some() != body.currency.as_ref().is_some_and(|c| !c.trim().is_empty()) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "message": PRICE_NEEDS_CURRENCY })),
        )
            .into_response();
    }
    // Trimmed, and an empty field becomes absent rather than an empty string. A form posts
    // "" for every box nobody typed in, and a row full of empty strings reads as a source
    // that was recorded with no information rather than one that was never given any.
    let blank_is_none = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let (url, vendor, external_id, title, license, currency) = (
        blank_is_none(&body.url),
        blank_is_none(&body.vendor),
        blank_is_none(&body.external_id),
        blank_is_none(&body.title),
        blank_is_none(&body.license),
        blank_is_none(&body.currency),
    );
    if url.is_none() && vendor.is_none() && title.is_none() && license.is_none() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "message": NOTHING_TO_RECORD })),
        )
            .into_response();
    }

    match PgParts(state.db)
        .add_part_source(
            part,
            lapidary_db::NewPartSource {
                url: url.as_deref(),
                vendor: vendor.as_deref(),
                external_id: external_id.as_deref(),
                title: title.as_deref(),
                license: license.as_deref(),
                price_minor: body.price_minor,
                currency: currency.as_deref(),
            },
        )
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(err) => internal_error(&err, "recording a part's source failed"),
    }
}

const PRICE_NEEDS_CURRENCY: &str =
    "A price needs a currency, and a currency needs a price. Fill in both, or leave both empty.";

const NOTHING_TO_RECORD: &str = "There is nothing to record yet. A source needs at least a link, a vendor, a title or a licence.";

/// One source, as the interface reads it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PartSource {
    pub id: PartSourceId,
    pub url: Option<String>,
    pub vendor: Option<String>,
    /// The seller's own identifier — an SKU, a Thingiverse id, a number in their catalogue.
    /// Named for what it is rather than `sku`, because it is whatever they call it.
    pub external_id: Option<String>,
    pub title: Option<String>,
    /// Free text. `DATA.md`: not bureaucracy — somebody selling prints needs to see this.
    pub license: Option<String>,
    /// Minor units, so 12.50 EUR is 1250. Never a float — a price is money.
    ///
    /// **`number` on the wire and not `bigint`**, which is what ts-rs emits for an `i64` by
    /// default. `JSON.stringify` throws on a BigInt, so the default would make this field
    /// unsendable; and a JSON number is exact to 2^53, which in minor units is ninety
    /// trillion of any currency. The column stays `bigint` — the narrowing is the wire's,
    /// not the store's.
    ///
    /// ponytail: exact to 2^53 minor units. If a price ever needs more than that, the field
    /// becomes a decimal string, not a wider integer.
    #[ts(type = "number | null")]
    pub price_minor: Option<i64>,
    /// ISO 4217. Meaningless without `price_minor`, which the database also insists on.
    pub currency: Option<String>,
}

/// What the form posts. Every field optional; the route refuses one that is entirely empty.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NewSource {
    pub url: Option<String>,
    pub vendor: Option<String>,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub license: Option<String>,
    /// `number` on the wire, for the reason [`PartSource::price_minor`] gives.
    #[ts(type = "number | null")]
    pub price_minor: Option<i64>,
    pub currency: Option<String>,
}
