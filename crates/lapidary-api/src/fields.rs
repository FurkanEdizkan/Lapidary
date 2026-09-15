//! Custom fields: a library's own named values on its parts (`docs/DATA.md` §3.5).
//!
//! The definitions, per library, and one value per part and field. A value is checked against its
//! field's kind before anything is written. Removing a field takes its definition and leaves every
//! part's value where it was, because user data is never deleted implicitly.

use crate::AppState;
use crate::folders::{internal_error, refused};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lapidary_core::{JobPayload, LibraryId, PartId};
use lapidary_db::{
    CustomFieldPatch, CustomFieldRow, DbError, PgCustomFields, PgJobs, PgPool, ValueSet,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The longest label, in characters. The table's check says the same.
const LABEL_MAX: usize = 80;
/// The longest text value, in characters.
const TEXT_MAX: usize = 512;
/// The most options a choice offers.
const OPTIONS_MAX: usize = 50;
/// The longest option, in characters.
const OPTION_MAX: usize = 80;

/// What a field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FieldKind {
    Text,
    Number,
    Choice,
}

impl FieldKind {
    fn as_str(self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::Number => "number",
            FieldKind::Choice => "choice",
        }
    }

    fn parse(kind: &str) -> Option<Self> {
        match kind {
            "text" => Some(FieldKind::Text),
            "number" => Some(FieldKind::Number),
            "choice" => Some(FieldKind::Choice),
            _ => None,
        }
    }
}

/// One field, as a library defines it.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CustomField {
    /// The field's name in the data: never renamed, and what a filter and a part's value are keyed by.
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    /// A choice's options, in order. Empty for the other kinds.
    pub options: Vec<String>,
    /// Offered as a grid filter.
    pub indexed: bool,
}

/// `POST /api/libraries/{id}/fields`.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct NewCustomField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default)]
    #[ts(optional)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub indexed: Option<bool>,
}

/// `PATCH /api/libraries/{library}/fields/{key}`: what changes. A field's key and kind never do.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct CustomFieldChange {
    #[serde(default)]
    #[ts(optional)]
    pub label: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub indexed: Option<bool>,
}

/// `PUT /api/parts/{id}/fields/{key}`: the part's value for one field, or `null` to clear it.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetFieldValue {
    #[ts(type = "string | number | null")]
    pub value: serde_json::Value,
}

fn to_field(row: CustomFieldRow) -> Option<CustomField> {
    Some(CustomField {
        kind: FieldKind::parse(&row.kind)?,
        key: row.key,
        label: row.label,
        options: row.options,
        indexed: row.indexed,
    })
}

/// `GET /api/libraries/{id}/fields`: a library's fields, oldest first. An id naming no library has
/// none, as its saved filters and its tree have none.
pub async fn list(State(state): State<AppState>, Path(library): Path<LibraryId>) -> Response {
    match PgCustomFields(state.db).list(library).await {
        // The table's check allows only the three kinds, so none is dropped here.
        Ok(rows) => Json(rows.into_iter().filter_map(to_field).collect::<Vec<_>>()).into_response(),
        Err(err) => internal_error(&err, "custom field list failed"),
    }
}

/// `POST /api/libraries/{id}/fields`: define a field.
pub async fn create(
    State(state): State<AppState>,
    Path(library): Path<LibraryId>,
    Json(body): Json<NewCustomField>,
) -> Response {
    if !valid_key(&body.key) {
        return refused(
            StatusCode::BAD_REQUEST,
            "badKey",
            "A field's key is 1 to 40 lowercase letters, digits or underscores, such as `stock_count`. It cannot be renamed later, so choose it with care.",
        );
    }
    let label = match tidy_label(&body.label) {
        Ok(label) => label,
        Err((reason, message)) => return refused(StatusCode::BAD_REQUEST, reason, &message),
    };
    let options = match tidy_options(body.kind, body.options) {
        Ok(options) => options,
        Err((reason, message)) => return refused(StatusCode::BAD_REQUEST, reason, &message),
    };
    let field = CustomFieldRow {
        key: body.key,
        label,
        kind: body.kind.as_str().to_owned(),
        options,
        indexed: body.indexed.unwrap_or(false),
    };
    match PgCustomFields(state.db).create(library, &field).await {
        Ok(()) => match to_field(field) {
            Some(field) => (StatusCode::CREATED, Json(field)).into_response(),
            None => StatusCode::CREATED.into_response(),
        },
        Err(err) => refusal(&err, "custom field create failed"),
    }
}

/// `PATCH /api/libraries/{library}/fields/{key}`: relabel a field, change a choice's options, or
/// offer it as a filter or stop.
pub async fn update(
    State(state): State<AppState>,
    Path((library, key)): Path<(LibraryId, String)>,
    Json(body): Json<CustomFieldChange>,
) -> Response {
    let fields = PgCustomFields(state.db);
    let field = match fields.field(library, &key).await {
        Ok(Some(field)) => field,
        Ok(None) => return no_such_field(),
        Err(err) => return internal_error(&err, "custom field lookup failed"),
    };
    let label = match body.label.as_deref().map(tidy_label).transpose() {
        Ok(label) => label,
        Err((reason, message)) => return refused(StatusCode::BAD_REQUEST, reason, &message),
    };
    let options = match body.options {
        None => None,
        Some(options) => {
            match tidy_options(
                FieldKind::parse(&field.kind).unwrap_or(FieldKind::Text),
                Some(options),
            ) {
                Ok(options) => Some(options),
                Err((reason, message)) => {
                    return refused(StatusCode::BAD_REQUEST, reason, &message);
                }
            }
        }
    };
    let patch = CustomFieldPatch {
        label: label.as_deref(),
        options: options.as_deref(),
        indexed: body.indexed,
    };
    match fields.update(library, &key, &patch).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_field(),
        Err(err) => refusal(&err, "custom field update failed"),
    }
}

/// `DELETE /api/libraries/{library}/fields/{key}`: the definition only. Every part's value stays.
pub async fn remove(
    State(state): State<AppState>,
    Path((library, key)): Path<(LibraryId, String)>,
) -> Response {
    match PgCustomFields(state.db).remove(library, &key).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => no_such_field(),
        Err(err) => internal_error(&err, "custom field remove failed"),
    }
}

/// `PUT /api/parts/{id}/fields/{key}`: one part's value for one field of its library.
pub async fn set_value(
    State(state): State<AppState>,
    Path((part, key)): Path<(PartId, String)>,
    Json(body): Json<SetFieldValue>,
) -> Response {
    let fields = PgCustomFields(state.db.clone());
    let (library, field) = match fields.field_of_part(part, &key).await {
        Ok(Some((library, Some(field)))) => (library, field),
        Ok(Some((_, None))) => return no_such_field(),
        Ok(None) => return no_such_part(),
        Err(err) => return internal_error(&err, "custom field lookup failed"),
    };
    let value = match checked_value(&field, body.value) {
        Ok(value) => value,
        Err(message) => return refused(StatusCode::BAD_REQUEST, "wrongType", &message),
    };
    match fields.set_value(part, &field, value.as_ref()).await {
        Ok(ValueSet::Set) => {
            // `metadata.json` mirrors the rows, and the worker is what writes into a model's directory.
            // Warn-only: the value is kept, and the file catches up on the part's next rewrite.
            let describe = [JobPayload::DescribePart { part }];
            if let Err(err) = PgJobs(state.db).enqueue(library, &describe).await {
                tracing::warn!(
                    error = %err,
                    %part,
                    "could not queue metadata.json's rewrite after a custom field value; the file holds the old value until the part's next rewrite"
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(ValueSet::NoSuchPart) => no_such_part(),
        Ok(ValueSet::FieldChanged) => refused(
            StatusCode::CONFLICT,
            "fieldChanged",
            &format!(
                "“{}” was changed or removed while this value was being saved. Reload the part, then set the value again.",
                field.label
            ),
        ),
        Err(err) => internal_error(&err, "custom field value write failed"),
    }
}

/// The grid's `field` and `fieldValue`, as the JSON object `{"<key>": value}` a `@>` filter matches.
/// `None` when either is absent or blank. Refused, as the response to send, when the key is not a
/// field this library offers as a filter or the value is not of that field's kind.
pub(crate) async fn filter_of(
    db: &PgPool,
    library: LibraryId,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<Option<String>, Response> {
    let (Some(key), Some(value)) = (
        key.map(str::trim).filter(|key| !key.is_empty()),
        value.map(str::trim).filter(|value| !value.is_empty()),
    ) else {
        return Ok(None);
    };
    let field = match PgCustomFields(db.clone()).field(library, key).await {
        Ok(Some(field)) if field.indexed => field,
        Ok(_) => {
            return Err(refused(
                StatusCode::BAD_REQUEST,
                "notAFilter",
                &format!(
                    "`{key}` is not a field this library offers as a filter. Choose one from the grid's filters, or offer the field as a filter in the library's fields."
                ),
            ));
        }
        Err(err) => return Err(internal_error(&err, "custom field lookup failed")),
    };
    let typed = if field.kind == "number" {
        match value
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
        {
            Some(number) => serde_json::Value::Number(number),
            None => {
                return Err(refused(
                    StatusCode::BAD_REQUEST,
                    "wrongType",
                    &format!(
                        "“{}” is a number field, and “{value}” is not a number.",
                        field.label
                    ),
                ));
            }
        }
    } else {
        serde_json::Value::String(value.to_owned())
    };
    let mut object = serde_json::Map::new();
    object.insert(field.key, typed);
    Ok(Some(serde_json::Value::Object(object).to_string()))
}

/// A value checked against its field. `Ok(None)` clears the part's value; `Err` is the sentence to
/// show.
fn checked_value(
    field: &CustomFieldRow,
    value: serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    let label = &field.label;
    match (field.kind.as_str(), value) {
        (_, serde_json::Value::Null) => Ok(None),
        ("number", serde_json::Value::Number(number)) => {
            Ok(Some(serde_json::Value::Number(number)))
        }
        ("number", other) => Err(format!("“{label}” takes a number, and {other} is not one.")),
        ("text", serde_json::Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                Ok(None)
            } else if text.chars().count() > TEXT_MAX {
                Err(format!(
                    "“{label}” takes at most {TEXT_MAX} characters. Shorten it and try again."
                ))
            } else {
                Ok(Some(serde_json::Value::String(text.to_owned())))
            }
        }
        ("choice", serde_json::Value::String(choice)) if field.options.contains(&choice) => {
            Ok(Some(serde_json::Value::String(choice)))
        }
        ("choice", other) => Err(format!(
            "“{label}” takes one of its options ({}), and {other} is not one of them.",
            field.options.join(", ")
        )),
        (_, other) => Err(format!("“{label}” takes text, and {other} is not text.")),
    }
}

fn valid_key(key: &str) -> bool {
    (1..=40).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn tidy_label(label: &str) -> Result<String, (&'static str, String)> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > LABEL_MAX {
        return Err((
            "badLabel",
            format!("A field's label is 1 to {LABEL_MAX} characters. Type one and try again."),
        ));
    }
    Ok(label.to_owned())
}

/// A choice's options trimmed, with nothing blank, repeated, too long or too many. Any other kind
/// takes none. Refused with the reason and the sentence to show, as `filters.rs`'s `tidy` is.
fn tidy_options(
    kind: FieldKind,
    options: Option<Vec<String>>,
) -> Result<Vec<String>, (&'static str, String)> {
    let options = options.unwrap_or_default();
    let bad = |message: String| Err(("badOptions", message));
    if kind != FieldKind::Choice {
        return if options.is_empty() {
            Ok(Vec::new())
        } else {
            bad(
                "Only a choice field has options. Leave them out, or make the field a choice."
                    .to_owned(),
            )
        };
    }
    let mut kept: Vec<String> = Vec::new();
    for option in options.iter().map(|option| option.trim()) {
        if option.is_empty() || option.chars().count() > OPTION_MAX {
            return bad(format!(
                "Each option is 1 to {OPTION_MAX} characters. Fix the ones that are not, and try again."
            ));
        }
        if kept.iter().any(|seen| seen == option) {
            return bad(format!(
                "“{option}” is listed twice. Keep one, and try again."
            ));
        }
        kept.push(option.to_owned());
    }
    if kept.is_empty() || kept.len() > OPTIONS_MAX {
        return bad(format!(
            "A choice field offers 1 to {OPTIONS_MAX} options. Add or remove some, and try again."
        ));
    }
    Ok(kept)
}

/// The refusals a field write answers with, and a 500 for anything else.
fn refusal(err: &DbError, what: &'static str) -> Response {
    match err {
        DbError::NoSuchLibrary { .. } => {
            refused(StatusCode::NOT_FOUND, "noSuchLibrary", &err.to_string())
        }
        DbError::FieldKeyTaken { .. } => {
            refused(StatusCode::CONFLICT, "keyTaken", &err.to_string())
        }
        DbError::TooManyFields { .. } => {
            refused(StatusCode::CONFLICT, "tooManyFields", &err.to_string())
        }
        DbError::TooManyIndexed { .. } => {
            refused(StatusCode::CONFLICT, "tooManyIndexed", &err.to_string())
        }
        DbError::OptionInUse { .. } => {
            refused(StatusCode::CONFLICT, "optionInUse", &err.to_string())
        }
        DbError::FieldValuesDoNotFit { .. } => {
            refused(StatusCode::CONFLICT, "valuesDoNotFit", &err.to_string())
        }
        other => internal_error(other, what),
    }
}

fn no_such_field() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchField",
        "This library has no field with that key. It may have been removed, so reload and try again.",
    )
}

fn no_such_part() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        "noSuchPart",
        "There is no model with that id. It may have been deleted — reload the grid and try again.",
    )
}
