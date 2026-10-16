//! The schema constraints slice 2 adds are load-bearing, not decoration: one of them is
//! what makes at-least-once job delivery safe. Each is tested by trying to violate it.

use lapidary_core::{BatchId, LibraryId};
use lapidary_db::PgJobs;
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

    // `run_to(5)`, not `run`: 0054 drops `auto_thumbnail` again, and this case is about what
    // 0005 does to rows that pre-date it.
    migrator.run_to(5, &pool).await.expect("0005 applies");

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

/// `0035` copies each part's latest revision's figures onto its row for the grid's order: not an older
/// revision's, the longest side as the largest extent, and no volume where the mesh had none.
#[sqlx::test(migrations = false)]
async fn the_sort_figures_are_copied_from_each_parts_latest_revision(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(34, &pool)
        .await
        .expect("migrations up to 0034 apply");
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let (vee, duct) = (Uuid::now_v7(), Uuid::now_v7());
    for (part, path) in [
        (vee, "vee-block-lp-3072-02.stl"),
        (duct, "fan-duct-lp-3311-01.stl"),
    ] {
        sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $3)")
            .bind(part)
            .bind(library)
            .bind(path)
            .execute(&pool)
            .await
            .expect("part inserts");
    }
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin, created_at, volume, volume_source, \
         surface_area, surface_area_source, bbox_x, bbox_y, bbox_z, bbox_source, triangle_count) VALUES \
         ($1, $4, '1', 'ingest', now() - interval '1 hour', 900.0, 'tessellated', 100.0, 'tessellated', \
          20.0, 5.0, 5.0, 'tessellated', 3000), \
         ($2, $4, '2', 'ingest', now(), 21478.5, 'tessellated', 9804.25, 'tessellated', \
          42.0, 61.0, 18.5, 'tessellated', 48112), \
         ($3, $5, '1', 'ingest', now(), NULL, NULL, 3310.0, 'tessellated', \
          88.0, 34.0, 12.0, 'tessellated', 12940)",
    )
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(Uuid::now_v7())
    .bind(vee)
    .bind(duct)
    .execute(&pool)
    .await
    .expect("revisions insert");

    migrator.run(&pool).await.expect("0035 applies");

    let copied = "SELECT latest_volume, latest_surface_area, latest_longest_side, latest_triangle_count \
                  FROM part WHERE id = $1";
    let vee: (Option<f64>, Option<f64>, Option<f64>, Option<i32>) = sqlx::query_as(copied)
        .bind(vee)
        .fetch_one(&pool)
        .await
        .expect("reads the vee block");
    assert_eq!(
        vee,
        (Some(21_478.5), Some(9_804.25), Some(61.0), Some(48_112)),
        "the latest revision's figures, not the first's"
    );
    let duct: (Option<f64>, Option<f64>, Option<f64>, Option<i32>) = sqlx::query_as(copied)
        .bind(duct)
        .fetch_one(&pool)
        .await
        .expect("reads the fan duct");
    assert_eq!(duct, (None, Some(3_310.0), Some(88.0), Some(12_940)));
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

    // `run_to(7)`, not `run`: this test is about `0007` and applying everything after it
    // was incidental. It stopped being harmless when `0015` arrived — the fixture above is a
    // database that never ran `migrate_storage`, so its file row has no `storage_path`, and
    // `0015` refuses exactly that. Which is the guard working, not a conflict to route
    // around: an operator with this database is told to drain the job first.
    migrator.run_to(7, &pool).await.expect("0007 applies");

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
    // This test seeds the table the way 6a leaves it, runs 0009's backfill by hand against
    // the already-migrated pool, and asserts the tree. Because sqlx has already run the
    // migration on an empty database, the rows are inserted first and the backfill's
    // statement is re-executed here.
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    for (name, path) in [
        ("bracket-lp-1042-03", "bracket-lp-1042-03.stl"),
        ("rock", "Terrain/rock.stl"),
        // The "?" is deliberate: these directories already exist on disk under that exact
        // name, so the folder this backfills must carry it unslugged — see the assertion
        // below.
        ("cliff", "Terrain/Rocks?/cliff.stl"),
        ("spire", "Terrain/Rocks?/Cliffs/spire.stl"),
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

    sqlx::query(include_str!("../backfill/0009_backfill.sql"))
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
            "Terrain/Rocks?",
            "Terrain/Rocks?/Cliffs"
        ],
        "Terrain/Rocks? and Bases/Rocks are two folders, not one"
    );

    // The backfill must not slugify: "Rocks?" is a directory name the store already has
    // on disk, and turning it into anything else would rename a directory the store is
    // about to be told to find. `folder.slug` is meant to be the filesystem-safe name —
    // it just is not computed by this backfill, on purpose, because these names did not
    // come through a path where computing one was safe.
    let rocks_slug: String =
        sqlx::query_scalar("SELECT slug FROM folder WHERE library_id = $1 AND name = 'Rocks?'")
            .bind(library)
            .fetch_one(&pool)
            .await
            .expect("reads the Rocks? folder's slug");
    assert_eq!(
        rocks_slug, "Rocks?",
        "the slug must stay unslugged, character for character"
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

/// Beyond the brief: the test above seeds parts into a database `0009` has already
/// migrated and re-runs the backfill statement by hand. It never exercises the copy of
/// that same statement appended to `0009_folders.sql` itself -- every `sqlx::test` above
/// migrates an *empty* database, so on that path `_dirs` is empty, `maxlvl` is null, the
/// loop never runs, and the inline copy is a no-op in every test in this file.
///
/// The real upgrade path is a database that already has parts with nested source_paths
/// (written by slice 6a's recursive scan) when `0009` runs against it. This drives that
/// path directly: migrate up to `0007`, seed the parts, then run `0009` for real and
/// check the tree it leaves behind matches the hand-run backfill above.
#[sqlx::test(migrations = false)]
async fn migration_0009_backfills_a_database_that_already_has_parts(pool: PgPool) {
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
            .expect("seeds a part before 0009 runs");
    }

    migrator
        .run(&pool)
        .await
        .expect("0009 applies against an already-populated database");

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

/// Task 8b's own opening line: an operator could only start a `migrate_storage` job by
/// hand-writing `INSERT INTO job` -- and nothing stopped them from writing it twice for
/// the same library. `0011_migrate_storage_startup_guard.sql` added a unique index that a
/// database in exactly that state would otherwise have failed to create at all
/// (`CREATE UNIQUE INDEX` refuses to build over an existing duplicate), so it
/// de-duplicated first, keeping the oldest pending row per library.
///
/// Fix round 3: the index itself is gone (`0012_drop_migrate_storage_pending_index.sql`)
/// -- it only ever constrained `pending` rows, never the concurrent *execution* it was
/// believed to guard, and the guards it forced into `release_leases` and `reschedule`
/// cost more correctness than the deduplication bought. The de-duplication in `0011`
/// still ran and is still worth pinning; the index it built is not, and must be absent
/// once `0012` has applied too.
#[sqlx::test(migrations = false)]
async fn migration_0011_de_duplicates_pending_migrations_and_0012_drops_its_index(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(9, &pool)
        .await
        .expect("migrations up to 0010 apply");

    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let mut oldest = None;
    for offset_seconds in [2_i64, 1, 0] {
        let id = Uuid::now_v7();
        if oldest.is_none() {
            oldest = Some(id);
        }
        sqlx::query(
            "INSERT INTO job (id, batch_id, library_id, kind, payload, state, created_at) \
             VALUES ($1, $2, $3, 'migrate_storage', '{}', 'pending', \
                     now() - make_interval(secs => $4))",
        )
        .bind(id)
        .bind(Uuid::now_v7())
        .bind(library)
        .bind(offset_seconds)
        .execute(&pool)
        .await
        .expect("hand-writes a pending migrate_storage row, as an operator would have");
    }
    // Three hand-written rows, none of them created by this test in `created_at` order --
    // the oldest is the FIRST one inserted (offset 2s ago), not the one this loop
    // happened to insert last, so this also pins that the migration keeps the oldest
    // rather than an arbitrary survivor.
    let count_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job WHERE kind = 'migrate_storage' AND library_id = $1",
    )
    .bind(library)
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(
        count_before, 3,
        "three hand-written rows exist before 0011 runs"
    );

    // Runs every remaining migration, 0011 and 0012 alike -- this is the same
    // round-trip point 4 of the round-3 brief asks for, just entered from a database
    // that already holds duplicates rather than an empty one.
    migrator
        .run(&pool)
        .await
        .expect("0011 must not fail against a database already holding duplicates, and 0012 must not fail after it");

    let survivors: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM job WHERE kind = 'migrate_storage' AND library_id = $1 \
          AND state = 'pending'",
    )
    .bind(library)
    .fetch_all(&pool)
    .await
    .expect("reads back");
    assert_eq!(
        survivors,
        vec![oldest.expect("set in the loop above")],
        "0011's de-duplication still ran: exactly the oldest pending row survives"
    );

    let index_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_indexes \
          WHERE tablename = 'job' AND indexname = 'job_migrate_storage_pending_per_library')",
    )
    .fetch_one(&pool)
    .await
    .expect("checks pg_indexes");
    assert!(
        !index_exists,
        "0012 must have dropped the index 0011 created"
    );
}

/// `backfill/0009_backfill.sql` and the copy appended inside `migrations/0009_folders.sql`
/// exist for two different reasons — the standalone file is what this test file re-runs by
/// hand against an already-migrated database, the appended copy is what upgrades a database
/// that already has parts when `0009` itself runs — and nothing stops the two drifting
/// apart. This is not a live-database test: it reads both files as text and checks the
/// migration contains the standalone file's contents verbatim, so an edit to one that is
/// not carried to the other fails here instead of shipping two silently different rebuilds
/// of the same tree.
#[test]
fn the_standalone_backfill_file_and_the_copy_inside_0009_agree() {
    let standalone = include_str!("../backfill/0009_backfill.sql");
    let migration = include_str!("../migrations/0009_folders.sql");
    assert!(
        migration.contains(standalone),
        "migrations/0009_folders.sql must contain backfill/0009_backfill.sql verbatim, or \
         the two have drifted apart"
    );
}

/// `0056`: sharing between installations is gone, and its ten tables with it. Nothing else may have
/// gone: a pulled part was an ordinary part, so the core tables it lived in must all still be there.
#[sqlx::test(migrations = "./migrations")]
async fn sharing_leaves_no_table_behind_and_takes_no_core_one(pool: PgPool) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_all(&pool)
    .await
    .expect("reads the table list");
    for gone in [
        "part_provenance",
        "pull",
        "share_grant",
        "share_member",
        "share",
        "peer_share_member",
        "peer_share_part",
        "peer_share",
        "peer",
        "peer_identity",
    ] {
        assert!(!tables.iter().any(|t| t == gone), "{gone} is still there");
    }
    for kept in [
        "part",
        "library",
        "folder",
        "revision",
        "file",
        "blob",
        "job",
        "part_image",
        "part_source",
    ] {
        assert!(tables.iter().any(|t| t == kept), "{kept} went with sharing");
    }
}

/// `0054`: production stops. The queue loses the jobs no handler can run, the similarity tables
/// go, the thumbnail setting goes — and **finished history keeps its word**.
///
/// Run by hand up to `0052` so the rows genuinely pre-date `0054` rather than being inserted into
/// a database that has already dropped the tables they live in. `0053` is reserved for another
/// goal and does not exist yet, so `0052` is the version before this one.
#[sqlx::test(migrations = false)]
async fn production_stops_and_takes_only_the_jobs_nothing_can_run(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(52, &pool)
        .await
        .expect("migrations up to 0052 apply");
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let part = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(part)
        .bind(library)
        .bind("Bracket, LP-1042-03")
        .bind("brackets/bracket-lp-1042-03.stl")
        .execute(&pool)
        .await
        .expect("a part");
    let revision = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision)
    .bind(part)
    .execute(&pool)
    .await
    .expect("a revision");

    // One batch holding the whole mixture, so "its batch still totals correctly" is a real
    // assertion about one batch rather than about three.
    let batch = Uuid::now_v7();
    let queued = async |state: &'static str, kind: &'static str, produce: Option<&'static str>| {
        let id = Uuid::now_v7();
        let payload = match produce {
            Some(produce) => serde_json::json!({ "revision": revision, "produce": produce }),
            None => serde_json::json!({ "revision": revision }),
        };
        sqlx::query(
            "INSERT INTO job (id, batch_id, library_id, kind, payload, state, outcome, \
                              last_error, leased_by, lease_expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, \
                     CASE WHEN $6 = 'done' THEN $7 END, \
                     CASE WHEN $6 = 'failed' THEN 'the reason this job gave' END, \
                     CASE WHEN $6 = 'running' THEN 'worker-a' END, \
                     CASE WHEN $6 = 'running' THEN now() + interval '60 seconds' END)",
        )
        .bind(id)
        .bind(batch)
        .bind(library)
        .bind(kind)
        .bind(&payload)
        .bind(state)
        .bind(if kind == "profile_shape" {
            "profiled"
        } else {
            "rendered"
        })
        .execute(&pool)
        .await
        .expect("a job row");
        id
    };

    let pending_profile = queued("pending", "profile_shape", None).await;
    // **`running`, not only pending.** `dequeue` reclaims a running row whose lease expires, so
    // one left behind is claimed by the next worker and fails on a payload it cannot parse.
    let running_rung = queued("running", "derive", Some("tessellation_l0")).await;
    let pending_thumb = queued("pending", "derive", Some("thumbnail")).await;
    let done_profile = queued("done", "profile_shape", None).await;
    let done_rung = queued("done", "derive", Some("tessellation_l2")).await;
    // **Failed rows of a dead kind stay**, deliberately: deleting them would change the totals of
    // a batch somebody already read. `PgJobs::retry` is what must not put them back — see below.
    let failed_profile = queued("failed", "profile_shape", None).await;
    let failed_thumb = queued("failed", "derive", Some("thumbnail")).await;
    // Live work, and it must survive: the exports are still built on demand.
    let pending_export = queued("pending", "derive", Some("export_stl")).await;
    let failed_export = queued("failed", "derive", Some("export_3mf")).await;
    let pending_ingest = queued("pending", "ingest_file", None).await;

    sqlx::query(
        "INSERT INTO part_shape (part_id, library_id, revision_id, l0_blake3, version, size_mm, descriptor) \
         VALUES ($1, $2, $3, repeat('a', 64), 1, 12.5, array_fill(0.1::real, ARRAY[35]))",
    )
    .bind(part)
    .bind(library)
    .bind(revision)
    .execute(&pool)
    .await
    .expect("a shape profile");
    let other = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(other)
        .bind(library)
        .bind("Bracket, LP-1042-04")
        .bind("brackets/bracket-lp-1042-04.stl")
        .execute(&pool)
        .await
        .expect("a second part");
    sqlx::query(
        "INSERT INTO part_link (part_id, other_id, library_id, kind) \
         VALUES (least($1, $2), greatest($1, $2), $3, 'variant')",
    )
    .bind(part)
    .bind(other)
    .bind(library)
    .execute(&pool)
    .await
    .expect("a decision about a pair");

    migrator.run(&pool).await.expect("0054 applies");

    let surviving: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM job ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("reads the queue");
    for (id, what) in [
        (pending_profile, "a pending profile_shape"),
        (running_rung, "a running rung rebuild"),
        (pending_thumb, "a pending thumbnail"),
    ] {
        assert!(
            !surviving.contains(&id),
            "{what} names work no handler can run and must be gone"
        );
    }
    for (id, what) in [
        (done_profile, "a finished profile_shape"),
        (done_rung, "a finished rung rebuild"),
        (failed_profile, "a failed profile_shape"),
        (failed_thumb, "a failed thumbnail rebuild"),
        (pending_export, "a pending export"),
        (failed_export, "a failed export"),
        (pending_ingest, "a pending ingest"),
    ] {
        assert!(surviving.contains(&id), "{what} must survive");
    }

    // **Finished history keeps its word.** `0047` added `profiled` to `job_outcome_known` and a
    // done row still holds it; `job_done_has_outcome` requires a finished job to say how it
    // finished, so dropping the value would make those rows unwritable and the constraint a lie
    // about the table's own contents.
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM job WHERE id = $1")
        .bind(done_profile)
        .fetch_one(&pool)
        .await
        .expect("reads it back");
    assert_eq!(outcome, "profiled", "a finished job keeps what it said");
    sqlx::query("UPDATE job SET outcome = 'profiled' WHERE id = $1")
        .bind(done_rung)
        .execute(&pool)
        .await
        .expect("`profiled` is still a value `job_outcome_known` admits");
    // Put back, because the counts below are about this row: writing `profiled` over its
    // `rendered` is a probe of the constraint, not a fact about the batch.
    sqlx::query("UPDATE job SET outcome = 'rendered' WHERE id = $1")
        .bind(done_rung)
        .execute(&pool)
        .await
        .expect("restores what the row said");

    // **And the batch still adds up, read through `PgJobs::batch_status` itself** rather than
    // through a copy of its SQL — a copy would pass just as happily if the real aggregate were
    // broken. Seven rows left: two finished, two pending, three failed.
    let library = LibraryId::from_uuid(library);
    let status = PgJobs(pool.clone())
        .batch_status(library, BatchId::from_uuid(batch))
        .await
        .expect("the batch reads")
        .expect("the batch is there");
    assert_eq!(status.total, 7);
    assert_eq!(status.pending, 2);
    assert_eq!(status.failed_total, 3);
    assert_eq!(
        status.rendered, 1,
        "the finished rung rebuild still counts as a render"
    );
    // Every failure is listed, including the two of kinds this build cannot name: `failures` is
    // pure SQL and never parses a payload, so an old row is a row and not a 500.
    assert_eq!(status.failed.len(), 3);

    // **And `retry` leaves the dead ones where they are.** Retrying one would put it back in the
    // queue for a handler that cannot parse its payload, and hand the person who pressed Retry
    // "this job's payload is malformed" about work that was removed from the product.
    let retried = PgJobs(pool.clone())
        .retry(library, BatchId::from_uuid(batch), None)
        .await
        .expect("retries");
    assert_eq!(retried, 1, "the failed export, and nothing else");
    let back: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM job WHERE state = 'pending' ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("reads the queue");
    assert!(
        back.contains(&failed_export),
        "the export is runnable again"
    );
    for (id, what) in [
        (failed_profile, "a failed profile_shape"),
        (failed_thumb, "a failed thumbnail rebuild"),
    ] {
        assert!(
            !back.contains(&id),
            "{what} must stay failed: nothing can run it"
        );
    }

    for table in ["part_shape", "part_link"] {
        let exists: bool = sqlx::query_scalar(
            "SELECT exists (SELECT 1 FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = $1)",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("the catalogue reads");
        assert!(!exists, "{table} is gone with shape similarity");
    }
    let has_setting: bool = sqlx::query_scalar(
        "SELECT exists (SELECT 1 FROM information_schema.columns \
         WHERE table_name = 'library' AND column_name = 'auto_thumbnail')",
    )
    .fetch_one(&pool)
    .await
    .expect("the catalogue reads");
    assert!(
        !has_setting,
        "there is no thumbnail left for a library to decide about"
    );

    // The parts themselves are untouched. "We never delete user data implicitly" — the rows this
    // migration removes are ours, not theirs.
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("counts");
    assert_eq!(parts, 2, "both parts are still here");
}

/// A part and a revision of it, for `0055`'s seeds. Returns the revision.
async fn seeded_revision(pool: &PgPool, name: &str, path: &str) -> Uuid {
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");
    let part = Uuid::now_v7();
    sqlx::query("INSERT INTO part (id, library_id, name, source_path) VALUES ($1, $2, $3, $4)")
        .bind(part)
        .bind(library)
        .bind(name)
        .bind(path)
        .execute(pool)
        .await
        .expect("a part");
    let revision = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO revision (id, part_id, rev_label, origin) VALUES ($1, $2, '1', 'ingest')",
    )
    .bind(revision)
    .bind(part)
    .execute(pool)
    .await
    .expect("a revision");
    revision
}

/// A `derivative` row of any kind, in raw SQL because `DerivativeKind` has two variants and both
/// are exports — it cannot name a single one of the kinds `0055` removes.
///
/// `None` for the hash is the inline shape: the bytes are the row, and there is no blob to
/// settle. `derivative_storage_is_exclusive` (`0004`) is what makes it one or the other.
async fn seeded_derivative(pool: &PgPool, revision: Uuid, kind: &str, hash: Option<&str>) {
    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, blake3, thumb_bytes, kernel_version, \
                                 params_json) \
         VALUES ($1, $2, $3, $4, \
                 CASE WHEN $4::text IS NULL THEN '\\x524946468c'::bytea END, \
                 'occt-7.8+glb-1+cpu-1', '{}'::jsonb)",
    )
    .bind(Uuid::now_v7())
    .bind(revision)
    .bind(kind)
    .bind(hash)
    .execute(pool)
    .await
    .expect("a derivative row");
}

/// `ref_count` and whether the clock is running, which is the whole of what `0055` writes.
async fn blob_state(pool: &PgPool, hash: &str) -> (i32, bool) {
    sqlx::query_as("SELECT ref_count, quarantined_at IS NOT NULL FROM blob WHERE blake3 = $1")
        .bind(hash)
        .fetch_one(pool)
        .await
        .expect("the blob row is still there")
}

/// `0055`: the rendered derivatives leave the table and the reaper gets their blobs.
///
/// Four blob stories, because the predicate has four outcomes and only one case can reach each:
/// an orphan the sweep must collect, a blob a **`file`** row still needs, a blob a surviving
/// **export** still names, and a blob the migration must never look at at all. Plus the inline
/// thumbnail, which has no blob because its bytes are the row.
///
/// Run by hand up to `0054` so the rows genuinely pre-date `0055` rather than being written into
/// a database where no build can name their kinds.
#[sqlx::test(migrations = false)]
async fn the_renders_leave_the_table_and_only_their_own_blobs_are_quarantined(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(54, &pool)
        .await
        .expect("migrations up to 0054 apply");

    // Four hashes, named for the story each one tells.
    let orphan = "a0".repeat(32);
    let shared_with_source = "b0".repeat(32);
    let shared_with_export = "c0".repeat(32);
    let export_only = "d0".repeat(32);

    let bracket = seeded_revision(
        &pool,
        "Bracket, LP-1042-03",
        "brackets/bracket-lp-1042-03.stl",
    )
    .await;
    let spacer = seeded_revision(
        &pool,
        "Hex spacer, M4×20, LP-2145-01",
        "fasteners/hex-spacer-m4x20-lp-2145-01.stl",
    )
    .await;

    for (hash, stored, refs) in [
        (&orphan, 184_320_i64, 4_i32),
        (&shared_with_source, 2_097_152, 2),
        (&shared_with_export, 61_440, 2),
        (&export_only, 48_210, 1),
    ] {
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, $2, $2, NULL, $3)",
        )
        .bind(hash)
        .bind(stored)
        .bind(refs)
        .execute(&pool)
        .await
        .expect("a blob row");
    }

    // The orphan carries four of the seven dead kinds, which is the ordinary case: a part's
    // rungs and its CAD reads were several rows naming one encode.
    for kind in ["tessellation_l0", "tessellation_l1", "entities", "pmi"] {
        seeded_derivative(&pool, bracket, kind, Some(&orphan)).await;
    }
    // **The blob a `file` row still references.** Content addressing makes the overlap possible
    // however it arose, and this is the case the `file` half of `recount`'s predicate exists
    // for: lose that half and this migration quarantines a live model file.
    seeded_derivative(&pool, bracket, "structure", Some(&shared_with_source)).await;
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, stored_bytes) \
         VALUES ($1, $2, 'source', 'stl', $3, 2097152, 2097152)",
    )
    .bind(Uuid::now_v7())
    .bind(bracket)
    .bind(&shared_with_source)
    .execute(&pool)
    .await
    .expect("the part's own model file");
    // **The blob a surviving export still names.** The `derivative` half.
    seeded_derivative(&pool, spacer, "tessellation_l2", Some(&shared_with_export)).await;
    seeded_derivative(&pool, spacer, "export_3mf", Some(&shared_with_export)).await;
    // A blob no dead row ever named: its hash must never reach the recount, so the drifted
    // `ref_count` it arrived with is left exactly as it is rather than corrected in passing.
    seeded_derivative(&pool, spacer, "export_stl", Some(&export_only)).await;
    // The inline thumbnail: `blake3 IS NULL`, its bytes in the row, no blob to settle.
    seeded_derivative(&pool, spacer, "thumbnail", None).await;
    // A kind no build has ever named. `NOT IN (the two exports)` is what settles it, and that is
    // the direction this migration's predicate is written in: what survives is what something
    // can still build, not what a list of seven happened to mention.
    seeded_derivative(&pool, bracket, "preview", None).await;

    migrator.run(&pool).await.expect("0055 applies");

    let left: Vec<String> = sqlx::query_scalar("SELECT kind FROM derivative ORDER BY kind")
        .fetch_all(&pool)
        .await
        .expect("reads the table");
    assert_eq!(
        left,
        ["export_3mf", "export_stl"],
        "the table holds only what something can still build"
    );
    let inline: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(octet_length(thumb_bytes)), 0)::bigint FROM derivative",
    )
    .fetch_one(&pool)
    .await
    .expect("counts the inline bytes");
    assert_eq!(
        inline, 0,
        "no inline thumbnail is left — though the space itself waits on `VACUUM FULL derivative`"
    );

    assert_eq!(
        blob_state(&pool, &orphan).await,
        (0, true),
        "nothing points at it any more, so its thirty days start now — and `PgBlobs::reap` is \
         what unlinks it, because a migration cannot unlink a file at all"
    );
    assert_eq!(
        blob_state(&pool, &shared_with_source).await,
        (1, false),
        "**the model file's bytes are untouched.** The `file` half of the predicate is the whole \
         guard, and this is the row it guards"
    );
    assert_eq!(
        blob_state(&pool, &shared_with_export).await,
        (1, false),
        "a surviving export still names these bytes, so they are counted down to one and not \
         quarantined"
    );
    assert_eq!(
        blob_state(&pool, &export_only).await,
        (1, false),
        "no dead row ever named it, so the migration never looked at it"
    );

    // The user's data, which is the half of the store this migration must not reach.
    let (parts, files): (i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM part), (SELECT count(*) FROM file)")
            .fetch_one(&pool)
            .await
            .expect("counts");
    assert_eq!((parts, files), (2, 1), "both parts and the model file stay");
}

/// `0061`: every removed part and folder is deleted for good, rows only, and `deleted_at` goes.
///
/// A removed bracket with an export, a live spacer filed in a removed category, and a live
/// washer that must come through untouched. The bracket's model file row goes and its source
/// blob is **not** recounted (that is what used to hand the file to the reaper); its export's
/// blob is, and enters quarantine.
#[sqlx::test(migrations = false)]
async fn removed_parts_and_folders_are_deleted_and_the_column_goes(pool: PgPool) {
    let migrator = sqlx::migrate!("./migrations");
    migrator
        .run_to(60, &pool)
        .await
        .expect("migrations up to 0060 apply");
    let library = Uuid::parse_str(SEEDED_LIBRARY).expect("seeded library id parses");

    let fixtures = Uuid::now_v7();
    let jigs = Uuid::now_v7();
    for (id, parent, name) in [(fixtures, None, "Fixtures"), (jigs, Some(fixtures), "Jigs")] {
        sqlx::query(
            "INSERT INTO folder (id, library_id, parent_id, name, slug, deleted_at) \
             VALUES ($1, $2, $3, $4, lower($4), CASE WHEN $3::uuid IS NULL THEN now() END)",
        )
        .bind(id)
        .bind(library)
        .bind(parent)
        .bind(name)
        .execute(&pool)
        .await
        .expect("a folder");
    }

    let bracket = seeded_revision(&pool, "Bracket, LP-1042-03", "bracket-lp-1042-03.stl").await;
    let spacer = seeded_revision(&pool, "Hex spacer, LP-2145-01", "jigs/hex-spacer.stl").await;
    seeded_revision(&pool, "Washer, M8, LP-3001-02", "washer-m8.stl").await;
    sqlx::query(
        "UPDATE part SET deleted_at = now() \
         WHERE id = (SELECT part_id FROM revision WHERE id = $1)",
    )
    .bind(bracket)
    .execute(&pool)
    .await
    .expect("the bracket was removed");
    sqlx::query(
        "UPDATE part SET folder_id = $2 WHERE id = (SELECT part_id FROM revision WHERE id = $1)",
    )
    .bind(spacer)
    .bind(jigs)
    .execute(&pool)
    .await
    .expect("the spacer is filed under Jigs");

    let source = "e0".repeat(32);
    let export = "f0".repeat(32);
    for hash in [&source, &export] {
        sqlx::query(
            "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
             VALUES ($1, 4096, 4096, 0, 1)",
        )
        .bind(hash)
        .execute(&pool)
        .await
        .expect("a blob row");
    }
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, stored_bytes, \
                           storage_path) \
         VALUES ($1, $2, 'source', 'stl', $3, 4096, 4096, \
                 'libraries/default/bracket-lp-1042-03/bracket-lp-1042-03.stl')",
    )
    .bind(Uuid::now_v7())
    .bind(bracket)
    .bind(&source)
    .execute(&pool)
    .await
    .expect("the bracket's model file");
    seeded_derivative(&pool, bracket, "export_3mf", Some(&export)).await;

    migrator.run(&pool).await.expect("0061 applies");

    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM part ORDER BY name")
        .fetch_all(&pool)
        .await
        .expect("reads parts");
    assert_eq!(
        names,
        ["Washer, M8, LP-3001-02"],
        "only the live part outside the removed tree stays"
    );
    let left: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM folder), (SELECT count(*) FROM revision), \
                (SELECT count(*) FROM file), (SELECT count(*) FROM derivative), \
                (SELECT count(*) FROM quarantined_file)",
    )
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(
        left,
        (0, 1, 0, 0, 0),
        "rows only, and no model file is queued for removal"
    );

    assert_eq!(
        blob_state(&pool, &source).await,
        (1, false),
        "the model file's blob is not recounted, so nothing ever schedules it for removal"
    );
    assert_eq!(
        blob_state(&pool, &export).await,
        (0, true),
        "the export nothing points at any more enters the cache quarantine"
    );

    let columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_name IN ('part', 'folder') AND column_name = 'deleted_at'",
    )
    .fetch_one(&pool)
    .await
    .expect("reads the catalogue");
    assert_eq!(columns, 0, "nothing can read `deleted_at` any more");
}
