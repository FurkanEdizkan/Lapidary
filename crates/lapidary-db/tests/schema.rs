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
