//! The schema constraints slice 2 adds are load-bearing, not decoration: one of them is
//! what makes at-least-once job delivery safe. Each is tested by trying to violate it.

use sqlx::PgPool;
use uuid::Uuid;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

#[sqlx::test(migrations = "./migrations")]
async fn two_parts_at_one_source_path_in_one_library_are_refused(pool: PgPool) {
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let insert = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)",
            )
            .bind(id)
            .bind(library)
            .bind("bracket-lp-1042-03")
            .bind("brackets/bracket-lp-1042-03.stl")
            .execute(&pool)
            .await
        }
    };

    insert(Uuid::now_v7())
        .await
        .expect("the first part inserts");
    let second = insert(Uuid::now_v7()).await;

    let err = second.expect_err("a second part at the same path must be refused");
    assert!(
        err.to_string()
            .contains("part_source_path_unique_per_library"),
        "expected the named constraint to be what refused it, got: {err}"
    );
}

/// The regression migration `0007` exists to prevent, and the reason the constraint moved
/// off the name.
///
/// Once the scan descends, two folders may each hold a `bracket.stl`. They are two parts
/// with one name. Under `part_name_unique_per_library` the second insert raised a unique
/// violation, `classify_write` mapped that violation to `Outcome::Skipped`, and the file
/// was reported as already here and never indexed — silently, for as many files as the
/// corpus had duplicate basenames.
#[sqlx::test(migrations = "./migrations")]
async fn two_parts_with_one_name_at_different_paths_are_allowed(pool: PgPool) {
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let insert = |path: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)",
            )
            .bind(Uuid::now_v7())
            .bind(library)
            // One name. This is the whole point: the stem is a label, not an identity.
            .bind("bracket-lp-1042-03")
            .bind(path)
            .execute(&pool)
            .await
        }
    };

    insert("brackets/bracket-lp-1042-03.stl")
        .await
        .expect("the first part inserts");
    insert("plates/bracket-lp-1042-03.stl")
        .await
        .expect("the same name in a different folder is a second part, not a duplicate");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_that_claims_done_without_an_outcome_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'done')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("done without an outcome must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_that_claims_failed_without_a_reason_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'failed')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("failed without a reason must be refused");

    assert!(
        err.to_string().contains("job_failed_has_reason"),
        "expected job_failed_has_reason to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_job_that_carries_its_last_error_is_accepted(pool: PgPool) {
    // job_failed_has_reason is an implication (`state <> 'failed' or last_error is not
    // null`), not an equivalence: only the 'failed' direction is mandatory. Task 5's
    // reschedule sets state back to 'pending' while keeping last_error, so a retrying
    // job can say what went wrong last time without waiting for it to exhaust its
    // attempts. If this constraint ever regresses to a biconditional, this is the row
    // that starts getting refused, and nothing else in this file would catch it.
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, last_error) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'pending', 'the database was unreachable')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect("a retrying job may carry its last error while pending");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_done_job_that_claims_it_rendered_something_is_accepted(pool: PgPool) {
    // The positive case for 0005's re-added job_outcome_known: without this, a CHECK
    // written inverted would pass every negative case below and still be wrong.
    sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'render_thumbnail', '{}'::jsonb, 'done', 'rendered')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect("a done job may report that it rendered something");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_job_that_claims_it_rendered_something_is_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'render_thumbnail', '{}'::jsonb, 'pending', 'rendered')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("an outcome on a non-terminal row must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_job_claiming_an_unknown_outcome_is_still_refused(pool: PgPool) {
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'render_thumbnail', '{}'::jsonb, 'done', 'polished')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("an outcome outside the known set must be refused");

    assert!(
        err.to_string().contains("job_outcome_known"),
        "expected job_outcome_known to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = false)]
async fn pre_existing_tessellation_rungs_survive_the_auto_thumbnail_migration(pool: PgPool) {
    // 0005 touches `library` and `job`, not `derivative` -- this is "we never delete user
    // data implicitly" made testable for the LOD ladder slice 3 wrote. Migrations are run
    // by hand here rather than via the attribute, so the rows genuinely pre-exist 0005
    // rather than being inserted into an already-migrated database.
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(4, &pool)
        .await
        .expect("migrations up to 0004 apply");

    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let part = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
        .bind(part)
        .bind(library)
        .bind("bracket-lp-1042-04")
        .execute(&pool)
        .await
        .expect("part inserts");

    let revision = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision)
    .bind(part)
    .execute(&pool)
    .await
    .expect("revision inserts");

    for kind in ["tessellation_l1", "tessellation_l2"] {
        sqlx::query(
            "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) \
             VALUES ($1, $2, $3, $4, 'mesh stl-1+cpu-1', '{}')",
        )
        .bind(Uuid::now_v7())
        .bind(revision)
        .bind(kind)
        .bind(b"lod-rung".as_slice())
        .execute(&pool)
        .await
        .expect("tessellation rung inserts");
    }

    migrator.run(&pool).await.expect("0005 applies");

    let has_column: bool = sqlx::query_scalar(
        "SELECT exists (SELECT 1 FROM information_schema.columns \
         WHERE table_name = 'library' AND column_name = 'auto_thumbnail')",
    )
    .fetch_one(&pool)
    .await
    .expect("column check runs");
    assert!(has_column, "0005 must actually have run");

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM derivative WHERE revision_id = $1 \
         AND kind IN ('tessellation_l1', 'tessellation_l2')",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .expect("count runs");

    assert_eq!(
        count, 2,
        "pre-existing tessellation rungs must survive migration 0005"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_job_that_claims_an_outcome_is_refused(pool: PgPool) {
    // The converse of `a_job_that_claims_done_without_an_outcome_is_refused`:
    // job_done_has_outcome is a genuine biconditional, so an outcome on a row that
    // isn't 'done' must be refused too, not just a 'done' row with no outcome.
    let err = sqlx::query(
        "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome) \
         VALUES ($1, $2, $3, 'ingest_file', '{}'::jsonb, 'pending', 'ingested')",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses"))
    .execute(&pool)
    .await
    .expect_err("a pending job claiming an outcome must be refused");

    assert!(
        err.to_string().contains("job_done_has_outcome"),
        "expected job_done_has_outcome to refuse it, got: {err}"
    );
}

/// Migration `0007` reconstructs `source_path` for rows that predate the column, and a
/// backfill that quietly writes the wrong value is worse than one that fails: it becomes
/// the identity every later scan compares against. Every pre-6a row is flat by
/// construction — the scan that created it could not descend — so the original filename is
/// exactly the stem plus the source format.
#[sqlx::test(migrations = false)]
async fn the_source_path_backfill_reconstructs_the_filename_a_flat_scan_used(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(6, &pool)
        .await
        .expect("migrations up to 0006 apply, so `part` has no source_path yet");

    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    // Two parts: one with a source `file` row to reconstruct from, one without. The
    // second is the case the COALESCE exists for — a live part with no revision is a
    // state this schema permits, and NOT NULL would strand it.
    let with_file = Uuid::now_v7();
    let orphan = Uuid::now_v7();
    for (id, name) in [
        (with_file, "idler-pulley-lp-4820-00"),
        (orphan, "flange-dn40-lp-3310-02"),
    ] {
        sqlx::query("INSERT INTO part (id, library_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(library)
            .bind(name)
            .execute(&pool)
            .await
            .expect("part inserts");
    }

    let revision = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision)
    .bind(with_file)
    .execute(&pool)
    .await
    .expect("revision inserts");

    let hash = "b".repeat(64);
    sqlx::query("INSERT INTO blob (blake3, size_bytes, stored_bytes) VALUES ($1, 4096, 2048)")
        .bind(&hash)
        .execute(&pool)
        .await
        .expect("blob inserts");
    // `obj`, not `stl`: a backfill that hardcoded an extension would pass on `stl` alone.
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes) \
         VALUES ($1, $2, 'source', 'obj', $3, 4096)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(&hash)
    .execute(&pool)
    .await
    .expect("file inserts");

    migrator.run(&pool).await.expect("0007 applies");

    let backfilled: Vec<(String, String)> =
        sqlx::query_as("SELECT name, source_path FROM part WHERE library_id = $1 ORDER BY name")
            .bind(library)
            .fetch_all(&pool)
            .await
            .expect("reads the backfilled rows");

    assert_eq!(
        backfilled,
        vec![
            (
                "flange-dn40-lp-3310-02".to_owned(),
                "flange-dn40-lp-3310-02".to_owned()
            ),
            (
                "idler-pulley-lp-4820-00".to_owned(),
                "idler-pulley-lp-4820-00.obj".to_owned()
            ),
        ],
        "the stem plus the recorded format where there is a source file, and the bare \
         name where there is none — never NULL, which NOT NULL would have refused"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn two_root_folders_with_one_name_are_refused(pool: PgPool) {
    // NULLs are distinct in a unique constraint by default, so a plain
    // unique(library_id, parent_id, name) would silently allow this — and a corpus scan
    // produces it on the first two top-level directories.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let insert = |id: Uuid, name: &'static str, slug: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO folder (id, library_id, parent_id, name, slug) \
                 VALUES ($1, $2, NULL, $3, $4)",
            )
            .bind(id)
            .bind(library)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await
        }
    };
    insert(Uuid::now_v7(), "Terrain", "Terrain")
        .await
        .expect("the first inserts");
    let err = insert(Uuid::now_v7(), "Terrain", "Terrain")
        .await
        .expect_err("the second must not");
    assert_eq!(
        err.as_database_error().and_then(|e| e.constraint()),
        Some("folder_name_unique_per_parent")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn two_distinct_names_that_slug_alike_are_refused(pool: PgPool) {
    // "Rocks?" and "Rocks*" are different names and the same directory.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let insert = |id: Uuid, name: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO folder (id, library_id, parent_id, name, slug) \
                 VALUES ($1, $2, NULL, $3, 'Rocks-')",
            )
            .bind(id)
            .bind(library)
            .bind(name)
            .execute(&pool)
            .await
        }
    };
    insert(Uuid::now_v7(), "Rocks?")
        .await
        .expect("the first inserts");
    let err = insert(Uuid::now_v7(), "Rocks*")
        .await
        .expect_err("the second must not");
    assert_eq!(
        err.as_database_error().and_then(|e| e.constraint()),
        Some("folder_slug_unique_per_parent")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_backfill_rebuilds_the_tree_from_nested_source_paths(pool: PgPool) {
    // Slice 6a made the scan recursive, so parts ingested since carry nested source_paths.
    // Without this backfill they are stranded flat forever: folders are only created for
    // files that actually ingest, and a re-scan settles every one of them as Skipped.
    //
    // This test seeds the table the way 6a leaves it, runs 0008's backfill by hand against
    // the already-migrated pool, and asserts the tree. Because sqlx has already run the
    // migration on an empty database, the rows are inserted first and the backfill's
    // statement is re-executed here.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    for (name, path) in [
        ("bracket-lp-1042-03", "bracket-lp-1042-03.stl"),
        ("rock", "Terrain/rock.stl"),
        ("cliff", "Terrain/Rocks/cliff.stl"),
        ("spire", "Terrain/Rocks/Cliffs/spire.stl"),
        ("round-32mm", "Bases/round-32mm.stl"),
        ("base-rock", "Bases/Rocks/base-rock.stl"),
    ] {
        sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1,$2,$3,$4)")
            .bind(Uuid::now_v7())
            .bind(library)
            .bind(name)
            .bind(path)
            .execute(&pool)
            .await
            .expect("seeds a part");
    }

    sqlx::query(include_str!("../backfill/0008_backfill.sql"))
        .execute(&pool)
        .await
        .expect("the backfill runs");

    let paths: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE t AS (
           SELECT id, name::text AS path FROM folder WHERE parent_id IS NULL
           UNION ALL SELECT f.id, t.path||'/'||f.name FROM folder f JOIN t ON f.parent_id = t.id)
         SELECT path FROM t ORDER BY path",
    )
    .fetch_all(&pool)
    .await
    .expect("reads the tree");

    assert_eq!(
        paths,
        vec![
            "Bases",
            "Bases/Rocks",
            "Terrain",
            "Terrain/Rocks",
            "Terrain/Rocks/Cliffs"
        ],
        "Terrain/Rocks and Bases/Rocks are two folders, not one"
    );

    let root_parts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM part WHERE folder_id IS NULL AND source_path NOT LIKE '%/%'",
    )
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(root_parts, 1, "the flat part stays at the library root");

    // Beyond the brief: the tree existing is not proof every nested part actually got
    // filed into it. Dropping the backfill's final UPDATE leaves this tree intact and
    // every nested part unfiled -- the two assertions above would still pass.
    let unfiled: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE folder_id IS NULL")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(
        unfiled, 1,
        "only the flat part stays unfiled; all five nested parts must get a folder_id"
    );
}

/// Beyond the brief: the test above seeds parts into a database `0008` has already
/// migrated and re-runs the backfill statement by hand. It never exercises the copy of
/// that same statement appended to `0008_folders.sql` itself -- every `sqlx::test` above
/// migrates an *empty* database, so on that path `_dirs` is empty, `maxlvl` is null, the
/// loop never runs, and the inline copy is a no-op in every test in this file.
///
/// The real upgrade path is a database that already has parts with nested source_paths
/// (written by slice 6a's recursive scan) when `0008` runs against it. This drives that
/// path directly: migrate up to `0007`, seed the parts, then run `0008` for real and
/// check the tree it leaves behind matches the hand-run backfill above.
#[sqlx::test(migrations = false)]
async fn migration_0008_backfills_a_database_that_already_has_parts(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(7, &pool)
        .await
        .expect("migrations up to 0007 apply");

    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    for (name, path) in [
        ("bracket-lp-1042-03", "bracket-lp-1042-03.stl"),
        ("rock", "Terrain/rock.stl"),
        ("cliff", "Terrain/Rocks/cliff.stl"),
        ("spire", "Terrain/Rocks/Cliffs/spire.stl"),
        ("round-32mm", "Bases/round-32mm.stl"),
        ("base-rock", "Bases/Rocks/base-rock.stl"),
    ] {
        sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1,$2,$3,$4)")
            .bind(Uuid::now_v7())
            .bind(library)
            .bind(name)
            .bind(path)
            .execute(&pool)
            .await
            .expect("seeds a part before 0008 runs");
    }

    migrator
        .run(&pool)
        .await
        .expect("0008 applies against an already-populated database");

    let paths: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE t AS (SELECT id, name::text AS path FROM folder WHERE parent_id IS NULL \
         UNION ALL SELECT f.id, t.path||'/'||f.name FROM folder f JOIN t ON f.parent_id = t.id) \
         SELECT path FROM t ORDER BY path",
    )
    .fetch_all(&pool)
    .await
    .expect("reads the tree");
    assert_eq!(
        paths,
        vec![
            "Bases",
            "Bases/Rocks",
            "Terrain",
            "Terrain/Rocks",
            "Terrain/Rocks/Cliffs"
        ],
        "the real migration backfills a pre-populated database the same way"
    );

    let unfiled: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE folder_id IS NULL")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(unfiled, 1, "only the flat part stays unfiled");
}
