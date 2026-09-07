//! The schema is the contract every repository depends on. These assert the parts of it
//! that are easy to get wrong and expensive to discover later.

#[sqlx::test(migrations = "./migrations")]
async fn every_expected_table_exists(pool: sqlx::PgPool) {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE' ORDER BY table_name",
    )
    .fetch_all(&pool)
    .await
    .expect("query runs");

    for expected in ["blob", "derivative", "file", "library", "part", "revision"] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected table `{expected}`, found {names:?}"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn the_search_column_is_stored_not_virtual(pool: sqlx::PgPool) {
    // PG18 defaults generated columns to VIRTUAL, and virtual columns cannot be indexed.
    // A virtual `search` column would make Phase 2's search silently unindexable.
    let generation: Option<String> = sqlx::query_scalar(
        "SELECT attgenerated::text FROM pg_attribute \
         WHERE attrelid = 'part'::regclass AND attname = 'search'",
    )
    .fetch_one(&pool)
    .await
    .expect("column exists");
    assert_eq!(generation.as_deref(), Some("s"), "search must be STORED");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_default_library_is_seeded(pool: sqlx::PgPool) {
    // Nothing in this slice creates a library, so the scan endpoint needs one to address.
    let (id, name): (uuid::Uuid, String) =
        sqlx::query_as("SELECT id, name FROM library ORDER BY created_at LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("a library is seeded");
    assert_eq!(id.to_string(), "01931b6e-0000-7000-8000-000000000001");
    assert_eq!(name, "Default");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_blob_cannot_be_orphaned_by_deleting_it_out_from_under_a_file(pool: sqlx::PgPool) {
    // file.blake3 references blob.blake3. Without the FK a purge could strand a file row
    // pointing at bytes that no longer exist.
    sqlx::query("INSERT INTO blob (blake3, size_bytes, stored_bytes) VALUES ($1, 10, 10)")
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .expect("blob inserts");
    let err = sqlx::query("DELETE FROM blob WHERE blake3 = $1")
        .bind("a".repeat(64))
        .execute(&pool)
        .await;
    assert!(err.is_ok(), "deleting an unreferenced blob is allowed");
}

/// The indexes the STORED guard above exists to make possible.
///
/// `the_search_column_is_stored_not_virtual` has guarded `part.search` since `0002` with a
/// comment about search being "silently unindexable" — and for fourteen migrations nothing
/// indexed it, so every search would have been a sequential scan that looks fine on a
/// developer's 156 parts. This is the other half of that guard.
///
/// Read out of the catalogue rather than asserted about the migration text: a test that
/// grepped the SQL would pass against a migration that was never applied.
#[sqlx::test(migrations = "./migrations")]
async fn search_has_the_three_indexes_it_needs_and_the_right_operator_class(pool: sqlx::PgPool) {
    let found: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT i.relname::text, am.amname::text, op.opcname::text \
           FROM pg_index x \
           JOIN pg_class i ON i.oid = x.indexrelid \
           JOIN pg_am am ON am.oid = i.relam \
           LEFT JOIN pg_opclass op ON op.oid = x.indclass[0] \
          WHERE x.indrelid = 'part'::regclass \
            AND i.relname IN ('part_search_gin', 'part_number_trgm', 'part_name_trgm') \
          ORDER BY i.relname",
    )
    .fetch_all(&pool)
    .await
    .expect("the catalogue reads");

    assert_eq!(
        found,
        vec![
            // The substring half. `gin_trgm_ops` and not `gist_trgm_ops`: GiST buys KNN
            // distance ordering nothing here asks for and pays with slower lookups.
            (
                "part_name_trgm".to_owned(),
                "gin".to_owned(),
                Some("gin_trgm_ops".to_owned())
            ),
            (
                "part_number_trgm".to_owned(),
                "gin".to_owned(),
                Some("gin_trgm_ops".to_owned())
            ),
            // The multi-word half, over the STORED tsvector the test above guards.
            (
                "part_search_gin".to_owned(),
                "gin".to_owned(),
                Some("tsvector_ops".to_owned())
            ),
        ],
        "three indexes, all GIN, and the trigram pair on the trigram operator class — \
         a missing one is a sequential scan that only shows up at corpus size, and the \
         wrong operator class is an index the planner will not use for `ILIKE`"
    );
}
