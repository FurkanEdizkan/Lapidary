//! Custom fields: a library's own named values on its parts (`docs/DATA.md` §3.5).
//!
//! A field's definition is a row here; a part's value is `part.metadata_json->'custom'->'<key>'`.
//! What a value may be is the API's decision, made against the definition before anything is written.
//! Removing a definition leaves every value where it is.

use crate::DbError;
use crate::folders::constraint_of;
use lapidary_core::{LibraryId, PartId};
use sqlx::PgPool;
use uuid::Uuid;

/// The most fields one library holds.
pub const MAX_FIELDS: i64 = 32;
/// The most fields one library offers as grid filters.
pub const MAX_INDEXED: i64 = 8;

/// One field as defined.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomFieldRow {
    pub key: String,
    pub label: String,
    /// `text`, `number` or `choice`.
    pub kind: String,
    /// A `choice` field's options, in order. Empty for the other kinds.
    pub options: Vec<String>,
    /// Offered as a grid filter.
    pub indexed: bool,
}

/// What a person changes on a field. `None` leaves that part of it as it is; the key and the kind
/// never change.
#[derive(Debug, Clone, Default)]
pub struct CustomFieldPatch<'a> {
    pub label: Option<&'a str>,
    pub options: Option<&'a [String]>,
    pub indexed: Option<bool>,
}

type FieldColumns = (String, String, String, Vec<String>, bool);

fn to_row((key, label, kind, options, indexed): FieldColumns) -> CustomFieldRow {
    CustomFieldRow {
        key,
        label,
        kind,
        options,
        indexed,
    }
}

/// What setting one part's value came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSet {
    Set,
    /// No live part by that id.
    NoSuchPart,
    /// The field is no longer what the value was checked against: an option or the field itself was
    /// removed, or its options changed, while the value was on its way.
    FieldChanged,
}

pub struct PgCustomFields(pub PgPool);

impl PgCustomFields {
    /// A library's fields, oldest first. An id naming no library has none.
    pub async fn list(&self, library: LibraryId) -> Result<Vec<CustomFieldRow>, DbError> {
        let rows: Vec<FieldColumns> = sqlx::query_as(
            "SELECT key, label, type, ARRAY(SELECT jsonb_array_elements_text(options_json)), indexed \
             FROM custom_field WHERE library_id = $1 ORDER BY created_at, id",
        )
        .bind(library.as_uuid())
        .fetch_all(&self.0)
        .await?;
        Ok(rows.into_iter().map(to_row).collect())
    }

    /// One field of a library, if it has one by that key.
    pub async fn field(
        &self,
        library: LibraryId,
        key: &str,
    ) -> Result<Option<CustomFieldRow>, DbError> {
        let row: Option<FieldColumns> = sqlx::query_as(
            "SELECT key, label, type, ARRAY(SELECT jsonb_array_elements_text(options_json)), indexed \
             FROM custom_field WHERE library_id = $1 AND key = $2",
        )
        .bind(library.as_uuid())
        .bind(key)
        .fetch_optional(&self.0)
        .await?;
        Ok(row.map(to_row))
    }

    /// Define a field. The caps are counted under the library's row lock, so two requests cannot
    /// both take the last place; the constraint decides a key already taken.
    pub async fn create(&self, library: LibraryId, field: &CustomFieldRow) -> Result<(), DbError> {
        let mut tx = self.0.begin().await?;
        lock_library(&mut tx, library).await?;
        let (fields, indexed): (i64, i64) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE indexed) FROM custom_field WHERE library_id = $1",
        )
        .bind(library.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        if fields >= MAX_FIELDS {
            return Err(DbError::TooManyFields { max: MAX_FIELDS });
        }
        if field.indexed && indexed >= MAX_INDEXED {
            return Err(DbError::TooManyIndexed { max: MAX_INDEXED });
        }
        // A removed field's values stay (DATA §3.5), and a field defined again under its key takes them
        // back. Values this field could not show refuse the key, where they would read as unset and slip
        // past its filter and its option check. Removed parts count: a value comes back with its part.
        let unfit: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM part \
             WHERE library_id = $1 AND jsonb_typeof(metadata_json->'custom') = 'object' \
               AND metadata_json->'custom' ? $2 \
               AND NOT CASE $3 \
                   WHEN 'number' THEN jsonb_typeof(metadata_json->'custom'->$2) = 'number' \
                   WHEN 'text' THEN jsonb_typeof(metadata_json->'custom'->$2) = 'string' \
                   ELSE jsonb_typeof(metadata_json->'custom'->$2) = 'string' \
                        AND metadata_json->'custom'->>$2 = ANY($4::text[]) END",
        )
        .bind(library.as_uuid())
        .bind(&field.key)
        .bind(&field.kind)
        .bind(&field.options)
        .fetch_one(&mut *tx)
        .await?;
        if unfit > 0 {
            return Err(DbError::FieldValuesDoNotFit {
                key: field.key.clone(),
                parts: unfit,
            });
        }
        sqlx::query(
            "INSERT INTO custom_field (id, library_id, key, label, type, options_json, indexed) \
             VALUES ($1, $2, $3, $4, $5, to_jsonb($6::text[]), $7)",
        )
        .bind(Uuid::now_v7())
        .bind(library.as_uuid())
        .bind(&field.key)
        .bind(&field.label)
        .bind(&field.kind)
        .bind(&field.options)
        .bind(field.indexed)
        .execute(&mut *tx)
        .await
        .map_err(|err| match constraint_of(&err).as_deref() {
            Some("custom_field_key_unique_per_library") => DbError::FieldKeyTaken {
                key: field.key.clone(),
            },
            _ => DbError::Query(err),
        })?;
        tx.commit().await?;
        Ok(())
    }

    /// Change a field's label, options or whether it is offered as a filter. `false` when the library
    /// has no field by that key. An option some part still holds is not removed.
    pub async fn update(
        &self,
        library: LibraryId,
        key: &str,
        patch: &CustomFieldPatch<'_>,
    ) -> Result<bool, DbError> {
        let mut tx = self.0.begin().await?;
        lock_library(&mut tx, library).await?;
        let current: Option<(bool, Vec<String>)> = sqlx::query_as(
            "SELECT indexed, ARRAY(SELECT jsonb_array_elements_text(options_json)) \
             FROM custom_field WHERE library_id = $1 AND key = $2 FOR UPDATE",
        )
        .bind(library.as_uuid())
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((indexed, options)) = current else {
            return Ok(false);
        };
        if patch.indexed == Some(true) && !indexed {
            let offered: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM custom_field WHERE library_id = $1 AND indexed",
            )
            .bind(library.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
            if offered >= MAX_INDEXED {
                return Err(DbError::TooManyIndexed { max: MAX_INDEXED });
            }
        }
        if let Some(kept) = patch.options {
            for dropped in options.iter().filter(|option| !kept.contains(option)) {
                let (parts, removed): (i64, i64) = sqlx::query_as(
                    "SELECT count(*), count(*) FILTER (WHERE deleted_at IS NOT NULL) \
                     FROM part WHERE library_id = $1 \
                     AND metadata_json->'custom' @> jsonb_build_object($2::text, $3::text)",
                )
                .bind(library.as_uuid())
                .bind(key)
                .bind(dropped)
                .fetch_one(&mut *tx)
                .await?;
                if parts > 0 {
                    return Err(DbError::OptionInUse {
                        option: dropped.clone(),
                        parts,
                        removed,
                    });
                }
            }
        }
        sqlx::query(
            "UPDATE custom_field SET label = coalesce($3, label), \
                    options_json = coalesce(to_jsonb($4::text[]), options_json), \
                    indexed = coalesce($5, indexed) \
             WHERE library_id = $1 AND key = $2",
        )
        .bind(library.as_uuid())
        .bind(key)
        .bind(patch.label)
        .bind(patch.options)
        .bind(patch.indexed)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Remove a field's definition. Every part's value stays in `metadata_json`. `false` when the
    /// library has no field by that key.
    pub async fn remove(&self, library: LibraryId, key: &str) -> Result<bool, DbError> {
        let removed = sqlx::query("DELETE FROM custom_field WHERE library_id = $1 AND key = $2")
            .bind(library.as_uuid())
            .bind(key)
            .execute(&self.0)
            .await?;
        Ok(removed.rows_affected() == 1)
    }

    /// The live part's library, and its library's field by that key if there is one. `None` when
    /// there is no live part by that id.
    pub async fn field_of_part(
        &self,
        part: PartId,
        key: &str,
    ) -> Result<Option<(LibraryId, Option<CustomFieldRow>)>, DbError> {
        #[allow(clippy::type_complexity)]
        let row: Option<(
            Uuid,
            Option<String>,
            Option<String>,
            Option<String>,
            Vec<String>,
            Option<bool>,
        )> = sqlx::query_as(
            "SELECT p.library_id, f.key, f.label, f.type, \
                        ARRAY(SELECT jsonb_array_elements_text(f.options_json)), f.indexed \
                 FROM part p \
                 LEFT JOIN custom_field f ON f.library_id = p.library_id AND f.key = $2 \
                 WHERE p.id = $1 AND p.deleted_at IS NULL",
        )
        .bind(part.as_uuid())
        .bind(key)
        .fetch_optional(&self.0)
        .await?;
        Ok(row.map(|(library, key, label, kind, options, indexed)| {
            let field = match (key, label, kind, indexed) {
                (Some(key), Some(label), Some(kind), Some(indexed)) => Some(CustomFieldRow {
                    key,
                    label,
                    kind,
                    options,
                    indexed,
                }),
                _ => None,
            };
            (LibraryId::from_uuid(library), field)
        }))
    }

    /// Set one part's value for the field `checked`, or clear it with `None`, while the field is still what
    /// the value was checked against. Only this key under `custom` changes.
    ///
    /// The field's row is share-locked before it is read again, so removing one of its options, or the
    /// field, waits for this write, and this write waits for either. `update` locks the same row before it
    /// counts the parts holding an option, so neither sees the other half done.
    pub async fn set_value(
        &self,
        part: PartId,
        checked: &CustomFieldRow,
        value: Option<&serde_json::Value>,
    ) -> Result<ValueSet, DbError> {
        let mut tx = self.0.begin().await?;
        let library: Option<Uuid> =
            sqlx::query_scalar("SELECT library_id FROM part WHERE id = $1 AND deleted_at IS NULL")
                .bind(part.as_uuid())
                .fetch_optional(&mut *tx)
                .await?;
        let Some(library) = library else {
            return Ok(ValueSet::NoSuchPart);
        };
        let now: Option<(String, Vec<String>)> = sqlx::query_as(
            "SELECT type, ARRAY(SELECT jsonb_array_elements_text(options_json)) \
             FROM custom_field WHERE library_id = $1 AND key = $2 FOR SHARE",
        )
        .bind(library)
        .bind(&checked.key)
        .fetch_optional(&mut *tx)
        .await?;
        if now.as_ref() != Some(&(checked.kind.clone(), checked.options.clone())) {
            return Ok(ValueSet::FieldChanged);
        }
        let updated = sqlx::query(
            "UPDATE part SET metadata_json = CASE WHEN $3::jsonb IS NULL \
                 THEN metadata_json #- ARRAY['custom', $2::text] \
                 ELSE jsonb_set( \
                     CASE WHEN jsonb_typeof(metadata_json->'custom') = 'object' THEN metadata_json \
                          ELSE jsonb_set(metadata_json, '{custom}', '{}'::jsonb) END, \
                     ARRAY['custom', $2::text], $3::jsonb) END \
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(part.as_uuid())
        .bind(&checked.key)
        .bind(value.map(|value| value.to_string()))
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Ok(ValueSet::NoSuchPart);
        }
        tx.commit().await?;
        Ok(ValueSet::Set)
    }
}

/// The library's row, locked to the end of the transaction. An id naming no library is refused.
async fn lock_library(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    library: LibraryId,
) -> Result<(), DbError> {
    let found: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM library WHERE id = $1 FOR NO KEY UPDATE")
            .bind(library.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
    match found {
        Some(_) => Ok(()),
        None => Err(DbError::NoSuchLibrary { library }),
    }
}
