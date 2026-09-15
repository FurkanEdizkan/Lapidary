//! Custom fields: definitions per library, values in `part.metadata_json->'custom'`.

use lapidary_core::{BlobHash, LibraryId, PartId};
use lapidary_db::{
    CustomFieldPatch, CustomFieldRow, DbError, GridQuery, IngestRequest, PartRepository,
    PgCustomFields, PgIngest, PgParts, Sort, StoredBlobRow, ValueSet,
};
use serde_json::json;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn field(key: &str, label: &str, kind: &str, options: &[&str], indexed: bool) -> CustomFieldRow {
    CustomFieldRow {
        key: key.to_owned(),
        label: label.to_owned(),
        kind: kind.to_owned(),
        options: options.iter().map(|option| (*option).to_owned()).collect(),
        indexed,
    }
}

/// One part's value for the library's field `key`, checked against that field as it is now.
async fn set(
    fields: &PgCustomFields,
    part: PartId,
    key: &str,
    value: Option<serde_json::Value>,
) -> ValueSet {
    let field = fields
        .field(library(), key)
        .await
        .expect("reads")
        .expect("defined");
    fields
        .set_value(part, &field, value.as_ref())
        .await
        .expect("sets")
}

/// A part in the seeded library, database only.
async fn part(pool: &sqlx::PgPool, name: &str, seed: u8) -> PartId {
    let source_path = format!("{name}.stl");
    let storage_path = format!("libraries/default/{name}/{name}.stl");
    PgIngest(pool.clone())
        .record(IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name,
            source_path: &source_path,
            folder: None,
            storage_path: Some(&storage_path),
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 48_112,
                stored_bytes: 0,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [80.0, 40.0, 12.0],
                triangle_count: 1_204,
                surface_area_mm2: 9_140.0,
                volume_mm3: Some(21_600.0),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            thumbnail_webp: None,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("records a part")
}

#[sqlx::test(migrations = "./migrations")]
async fn a_library_offers_at_most_eight_fields_as_filters(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    for n in 0..8 {
        fields
            .create(
                library(),
                &field(
                    &format!("grade_{n}"),
                    &format!("Grade {n}"),
                    "text",
                    &[],
                    true,
                ),
            )
            .await
            .expect("up to eight are offered");
    }
    let ninth = fields
        .create(library(), &field("finish", "Finish", "text", &[], true))
        .await;
    assert!(
        matches!(ninth, Err(DbError::TooManyIndexed { max: 8 })),
        "{ninth:?}"
    );

    fields
        .create(library(), &field("finish", "Finish", "text", &[], false))
        .await
        .expect("a ninth field that is not a filter is fine");
    let offered = fields
        .update(
            library(),
            "finish",
            &CustomFieldPatch {
                indexed: Some(true),
                ..CustomFieldPatch::default()
            },
        )
        .await;
    assert!(
        matches!(offered, Err(DbError::TooManyIndexed { .. })),
        "{offered:?}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_key_is_taken_once_per_library(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field(
                "supplier",
                "Supplier",
                "choice",
                &["Hoffmann", "Misumi"],
                true,
            ),
        )
        .await
        .expect("defines");
    let again = fields
        .create(library(), &field("supplier", "Vendor", "text", &[], false))
        .await;
    assert!(
        matches!(again, Err(DbError::FieldKeyTaken { .. })),
        "{again:?}"
    );

    let listed = fields.list(library()).await.expect("lists");
    assert_eq!(
        listed,
        [field(
            "supplier",
            "Supplier",
            "choice",
            &["Hoffmann", "Misumi"],
            true
        )]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn an_option_a_part_holds_is_not_removed(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field(
                "supplier",
                "Supplier",
                "choice",
                &["Hoffmann", "Misumi"],
                true,
            ),
        )
        .await
        .expect("defines");
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    assert_eq!(
        set(&fields, bracket, "supplier", Some(json!("Misumi"))).await,
        ValueSet::Set
    );

    let kept = ["Hoffmann".to_owned()];
    let dropped = fields
        .update(
            library(),
            "supplier",
            &CustomFieldPatch {
                options: Some(&kept),
                ..CustomFieldPatch::default()
            },
        )
        .await;
    assert!(
        matches!(&dropped, Err(DbError::OptionInUse { option, parts: 1, removed: 0 }) if option == "Misumi"),
        "{dropped:?}"
    );

    let kept = ["Misumi".to_owned(), "Norelem".to_owned()];
    assert!(
        fields
            .update(
                library(),
                "supplier",
                &CustomFieldPatch {
                    label: Some("Supplier (catalogue)"),
                    options: Some(&kept),
                    ..CustomFieldPatch::default()
                },
            )
            .await
            .expect("an unused option goes")
    );
    let supplier = fields
        .field(library(), "supplier")
        .await
        .expect("reads")
        .expect("defined");
    assert_eq!(supplier.label, "Supplier (catalogue)");
    assert_eq!(supplier.options, ["Misumi", "Norelem"]);
}

/// A removed part still holds its value, and it comes back with the part, so its option stays too. The
/// refusal says so, because the grid and the part page no longer reach that part to change it.
#[sqlx::test(migrations = "./migrations")]
async fn an_option_only_a_removed_part_holds_says_to_restore_it(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field(
                "finish",
                "Finish",
                "choice",
                &["Anodised", "Zinc plated"],
                false,
            ),
        )
        .await
        .expect("defines");
    let plate = part(&pool, "mounting-plate-lp-1180-01", 0x42).await;
    assert_eq!(
        set(&fields, plate, "finish", Some(json!("Zinc plated"))).await,
        ValueSet::Set
    );
    assert!(
        PgParts(pool.clone())
            .soft_delete(plate)
            .await
            .expect("removes")
    );

    let kept = ["Anodised".to_owned()];
    let dropped = fields
        .update(
            library(),
            "finish",
            &CustomFieldPatch {
                options: Some(&kept),
                ..CustomFieldPatch::default()
            },
        )
        .await;
    assert!(
        matches!(
            &dropped,
            Err(DbError::OptionInUse {
                parts: 1,
                removed: 1,
                ..
            })
        ),
        "{dropped:?}"
    );
    let message = dropped.expect_err("refused").to_string();
    assert!(
        message.contains("restore those from Removed parts"),
        "{message}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_removed_field_leaves_every_value_where_it_was(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field("stock_count", "Stock count", "number", &[], false),
        )
        .await
        .expect("defines");
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    assert_eq!(
        set(&fields, bracket, "stock_count", Some(json!(12))).await,
        ValueSet::Set
    );

    assert!(
        fields
            .remove(library(), "stock_count")
            .await
            .expect("removes")
    );
    assert!(
        fields
            .field(library(), "stock_count")
            .await
            .expect("reads")
            .is_none()
    );
    let detail = PgParts(pool.clone())
        .detail(bracket)
        .await
        .expect("reads")
        .expect("the part");
    assert_eq!(detail.custom, json!({ "stock_count": 12 }));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_value_is_set_and_cleared_without_touching_what_the_file_said(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    for definition in [
        field("supplier", "Supplier", "text", &[], false),
        field("stock_count", "Stock count", "number", &[], false),
    ] {
        fields
            .create(library(), &definition)
            .await
            .expect("defines");
    }
    let parts = PgParts(pool.clone());
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    parts
        .set_metadata(
            bracket,
            &json!({ "cad": { "authors": ["J. Okafor"] } }),
            &[],
        )
        .await
        .expect("the file's own statement");

    assert_eq!(
        set(&fields, bracket, "supplier", Some(json!("Misumi"))).await,
        ValueSet::Set
    );
    assert_eq!(
        set(&fields, bracket, "stock_count", Some(json!(12))).await,
        ValueSet::Set
    );
    assert_eq!(
        set(&fields, bracket, "stock_count", None).await,
        ValueSet::Set
    );
    // A later statement from the file replaces `cad` and leaves a person's values alone.
    parts
        .set_metadata(bracket, &json!({ "cad": { "authors": ["M. Reyes"] } }), &[])
        .await
        .expect("again");

    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT metadata_json FROM part WHERE id = $1")
            .bind(bracket.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads");
    assert_eq!(
        stored,
        json!({ "cad": { "authors": ["M. Reyes"] }, "custom": { "supplier": "Misumi" } })
    );
    assert_eq!(
        set(&fields, PartId::new(), "supplier", Some(json!("Misumi"))).await,
        ValueSet::NoSuchPart
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_grid_filters_by_a_field_value(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    for definition in [
        field("supplier", "Supplier", "text", &[], false),
        field("stock_count", "Stock count", "number", &[], false),
    ] {
        fields
            .create(library(), &definition)
            .await
            .expect("defines");
    }
    let bracket = part(&pool, "bracket-lp-1042-03", 0x41).await;
    let spacer = part(&pool, "spacer-lp-2001-00", 0x42).await;
    part(&pool, "gear-m2-20t-lp-5140-00", 0x43).await;
    assert_eq!(
        set(&fields, bracket, "supplier", Some(json!("Misumi"))).await,
        ValueSet::Set
    );
    assert_eq!(
        set(&fields, spacer, "supplier", Some(json!("Hoffmann"))).await,
        ValueSet::Set
    );
    assert_eq!(
        set(&fields, spacer, "stock_count", Some(json!(12))).await,
        ValueSet::Set
    );

    let parts = PgParts(pool.clone());
    let ids = |rows: Vec<lapidary_db::PartRow>| {
        rows.into_iter()
            .map(|row| row.summary.id)
            .collect::<Vec<_>>()
    };
    let misumi = json!({ "supplier": "Misumi" }).to_string();
    let grid = GridQuery {
        field: Some(&misumi),
        ..GridQuery::new(library(), 50)
    };
    assert_eq!(
        ids(parts.page(&grid, Sort::Newest).await.expect("pages")),
        [bracket]
    );
    assert_eq!(
        ids(parts.page(&grid, Sort::Volume).await.expect("sorts")),
        [bracket]
    );
    assert_eq!(
        ids(parts.search(&grid, "lp").await.expect("searches")),
        [bracket]
    );

    let twelve = json!({ "stock_count": 12.0 }).to_string();
    let grid = GridQuery {
        field: Some(&twelve),
        ..GridQuery::new(library(), 50)
    };
    assert_eq!(
        ids(parts.page(&grid, Sort::Newest).await.expect("pages")),
        [spacer],
        "12 is 12.0"
    );

    let tags = parts
        .format_facet(
            library(),
            None,
            None,
            lapidary_db::Shows::Live,
            None,
            None,
            Some(&misumi),
            None,
        )
        .await
        .expect("facets");
    assert_eq!(tags.iter().filter_map(|value| value.count).sum::<u64>(), 1);
}

/// A field defined again under a removed field's key takes back the values left under it, as long as it
/// can show them. Values it could not show refuse the key, where they would read as unset.
#[sqlx::test(migrations = "./migrations")]
async fn a_key_defined_again_takes_back_only_values_it_can_show(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field("supplier", "Supplier", "text", &[], false),
        )
        .await
        .expect("defines");
    let bracket = part(&pool, "bracket-lp-1042-03", 0x43).await;
    assert_eq!(
        set(&fields, bracket, "supplier", Some(json!("Misumi Europe"))).await,
        ValueSet::Set
    );
    assert!(fields.remove(library(), "supplier").await.expect("removes"));

    for (kind, options) in [("choice", &["Misumi", "Hoffmann"][..]), ("number", &[][..])] {
        let refused = fields
            .create(
                library(),
                &field("supplier", "Supplier", kind, options, false),
            )
            .await;
        assert!(
            matches!(&refused, Err(DbError::FieldValuesDoNotFit { key, parts: 1 }) if key == "supplier"),
            "{kind}: {refused:?}"
        );
    }
    fields
        .create(
            library(),
            &field(
                "supplier",
                "Supplier",
                "choice",
                &["Misumi Europe", "Hoffmann"],
                false,
            ),
        )
        .await
        .expect("a field that shows the values takes them back");
    let custom: String =
        sqlx::query_scalar("SELECT (metadata_json->'custom')::text FROM part WHERE id = $1")
            .bind(bracket.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&custom).expect("json"),
        json!({ "supplier": "Misumi Europe" })
    );
}

/// An option removed while a value naming it is on its way: the write waits for the removal's lock, then
/// finds the field changed, rather than leaving a part holding an option that is gone.
#[sqlx::test(migrations = "./migrations")]
async fn a_value_checked_against_a_field_that_changes_meanwhile_is_not_written(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field(
                "supplier",
                "Supplier",
                "choice",
                &["Hoffmann", "Misumi"],
                true,
            ),
        )
        .await
        .expect("defines");
    let bracket = part(&pool, "bracket-lp-1042-03", 0x44).await;
    let checked = fields
        .field(library(), "supplier")
        .await
        .expect("reads")
        .expect("defined");

    // Removing the option, caught between taking the field's lock and committing.
    let mut removing = pool.begin().await.expect("begins");
    sqlx::query("SELECT 1 FROM custom_field WHERE library_id = $1 AND key = 'supplier' FOR UPDATE")
        .bind(library().as_uuid())
        .execute(&mut *removing)
        .await
        .expect("locks");
    let writing = tokio::spawn({
        let fields = PgCustomFields(pool.clone());
        async move {
            fields
                .set_value(bracket, &checked, Some(&json!("Misumi")))
                .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !writing.is_finished(),
        "the write waits for the field's lock"
    );
    sqlx::query(
        "UPDATE custom_field SET options_json = '[\"Hoffmann\"]' WHERE library_id = $1 AND key = 'supplier'",
    )
    .bind(library().as_uuid())
    .execute(&mut *removing)
    .await
    .expect("drops Misumi");
    removing.commit().await.expect("commits");

    assert_eq!(
        writing.await.expect("joins").expect("writes nothing"),
        ValueSet::FieldChanged
    );
    let custom: Option<String> =
        sqlx::query_scalar("SELECT (metadata_json->'custom')::text FROM part WHERE id = $1")
            .bind(bracket.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("reads");
    assert_eq!(custom, None, "the part holds no value");
}

/// A value written while an option is being removed: the removal waits for the write's lock on the
/// field, then counts the part the write left holding the option.
#[sqlx::test(migrations = "./migrations")]
async fn removing_an_option_waits_for_a_value_being_written_and_then_counts_it(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(
            library(),
            &field(
                "supplier",
                "Supplier",
                "choice",
                &["Hoffmann", "Misumi"],
                true,
            ),
        )
        .await
        .expect("defines");
    let bracket = part(&pool, "bracket-lp-1042-03", 0x45).await;

    // A value being written, caught between its share lock on the field and its commit.
    let mut writing = pool.begin().await.expect("begins");
    sqlx::query("SELECT 1 FROM custom_field WHERE library_id = $1 AND key = 'supplier' FOR SHARE")
        .bind(library().as_uuid())
        .execute(&mut *writing)
        .await
        .expect("locks");
    let removing = tokio::spawn({
        let fields = PgCustomFields(pool.clone());
        async move {
            let kept = ["Hoffmann".to_owned()];
            fields
                .update(
                    library(),
                    "supplier",
                    &CustomFieldPatch {
                        options: Some(&kept),
                        ..CustomFieldPatch::default()
                    },
                )
                .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !removing.is_finished(),
        "removing the option waits for the value's lock"
    );
    sqlx::query(
        "UPDATE part SET metadata_json = jsonb_set(metadata_json, '{custom}', '{\"supplier\": \"Misumi\"}') WHERE id = $1",
    )
    .bind(bracket.as_uuid())
    .execute(&mut *writing)
    .await
    .expect("writes Misumi");
    writing.commit().await.expect("commits");

    let removed = removing.await.expect("joins");
    assert!(
        matches!(&removed, Err(DbError::OptionInUse { parts: 1, .. })),
        "{removed:?}"
    );
}

/// A range narrows the grid, a sort, a search and a facet to the parts whose number lies in it, each bound
/// inclusive. A part holding words under the same key is passed over rather than failing the query.
#[sqlx::test(migrations = "./migrations")]
async fn a_number_range_narrows_the_grid_and_passes_over_words(pool: sqlx::PgPool) {
    let fields = PgCustomFields(pool.clone());
    fields
        .create(library(), &field("bore_mm", "Bore", "number", &[], true))
        .await
        .expect("defines");
    let bushing = part(&pool, "bushing-d8-lp-3008-00", 0x41).await;
    let flange = part(&pool, "flange-d22-lp-3022-00", 0x42).await;
    let sleeve = part(&pool, "sleeve-lp-3030-00", 0x43).await;
    for (part, bore) in [(bushing, 8), (flange, 22)] {
        assert_eq!(
            set(&fields, part, "bore_mm", Some(json!(bore))).await,
            ValueSet::Set
        );
    }
    // Words under the key, as a manifest written elsewhere could leave them: a cast would fail the query.
    sqlx::query(
        "UPDATE part SET metadata_json = jsonb_set(metadata_json, '{custom}', '{\"bore_mm\": \"about 12\"}') WHERE id = $1",
    )
    .bind(sleeve.as_uuid())
    .execute(&pool)
    .await
    .expect("words");

    let parts = PgParts(pool.clone());
    let ids = |rows: Vec<lapidary_db::PartRow>| {
        rows.into_iter()
            .map(|row| row.summary.id)
            .collect::<Vec<_>>()
    };
    let grid = |range: &'static str| GridQuery {
        field_range: Some(range),
        ..GridQuery::new(library(), 50)
    };
    let within = grid(r#"$."bore_mm" ? (@ >= 8 && @ <= 20)"#);
    assert_eq!(
        ids(parts.page(&within, Sort::Newest).await.expect("pages")),
        [bushing]
    );
    assert_eq!(
        ids(parts.page(&within, Sort::Volume).await.expect("sorts")),
        [bushing]
    );
    assert_eq!(
        ids(parts.search(&within, "lp").await.expect("searches")),
        [bushing]
    );
    let above = grid(r#"$."bore_mm" ? (@ >= 10)"#);
    assert_eq!(
        ids(parts.page(&above, Sort::Newest).await.expect("pages")),
        [flange]
    );
    let formats = parts
        .format_facet(
            library(),
            None,
            None,
            lapidary_db::Shows::Live,
            None,
            None,
            None,
            Some(r#"$."bore_mm" ? (@ <= 22)"#),
        )
        .await
        .expect("facets");
    assert_eq!(
        formats.iter().filter_map(|value| value.count).sum::<u64>(),
        2
    );
}
