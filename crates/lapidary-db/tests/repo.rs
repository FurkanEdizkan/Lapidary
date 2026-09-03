use lapidary_core::{BlobHash, LibraryId, MeshMeasurements};
use lapidary_db::{
    DbError, IngestRequest, PartRepository, PgBlobs, PgIngest, PgParts, StoredBlobRow,
};

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn blob_row(seed: u8) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 204_800,
        stored_bytes: 91_204,
        zstd_level: 3,
    }
}

fn watertight() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [61.0, 42.0, 18.5],
        triangle_count: 48_112,
        surface_area_mm2: 9_804.25,
        volume_mm3: Some(21_478.5),
        is_watertight: true,
    }
}

fn open_mesh() -> MeshMeasurements {
    MeshMeasurements {
        bbox_mm: [88.0, 34.0, 12.0],
        triangle_count: 12_940,
        surface_area_mm2: 15_320.5,
        volume_mm3: None,
        is_watertight: false,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn recording_an_ingest_creates_a_part_a_revision_a_file_and_a_thumbnail(pool: sqlx::PgPool) {
    let blob = blob_row(0xab);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bearing block, 608ZZ",
            blob: &blob,
            measurements: &watertight(),
            thumbnail_webp: b"webp bytes",
            kernel_version: "mesh stl-1+cpu-1",
        })
        .await
        .expect("records");

    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM part WHERE id = $1),
                (SELECT count(*) FROM revision r WHERE r.part_id = $1),
                (SELECT count(*) FROM file f JOIN revision r ON f.revision_id = r.id WHERE r.part_id = $1),
                (SELECT count(*) FROM derivative d JOIN revision r ON d.revision_id = r.id WHERE r.part_id = $1 AND d.kind = 'thumbnail')",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(counts, (1, 1, 1, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn every_measurement_is_written_as_tessellated(pool: sqlx::PgPool) {
    let blob = blob_row(0xcd);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    let (vs, bs): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT volume_source, bbox_source FROM revision WHERE part_id = $1")
            .bind(id.as_uuid())
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(vs.as_deref(), Some("tessellated"));
    assert_eq!(bs.as_deref(), Some("tessellated"));
}

#[sqlx::test(migrations = "./migrations")]
async fn an_open_mesh_stores_a_null_volume_but_still_stores_its_bbox(pool: sqlx::PgPool) {
    let blob = blob_row(0xef);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Cable clip, LP-3300-01",
            blob: &blob,
            measurements: &open_mesh(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    let (volume, vs, bx, watertight): (Option<f64>, Option<String>, Option<f64>, Option<bool>) =
        sqlx::query_as(
            "SELECT volume, volume_source, bbox_x, is_watertight FROM revision WHERE part_id = $1",
        )
        .bind(id.as_uuid())
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(volume, None, "an open mesh must store no volume");
    assert_eq!(vs, None, "no volume means no provenance for one");
    assert_eq!(
        bx,
        Some(88.0),
        "the bbox is still measurable and still stored"
    );
    assert_eq!(watertight, Some(false));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_known_hash_is_reported_as_existing(pool: sqlx::PgPool) {
    let blob = blob_row(0x11);
    let blobs = PgBlobs(pool.clone());
    assert!(!blobs.exists(&blob.hash).await.expect("query"));
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Spacer, LP-2001-00",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    assert!(blobs.exists(&blob.hash).await.expect("query"));
}

/// A second library to scan the same bytes into. Migration `0002_parts.sql` seeds only
/// one, and slice 1 has no library-creation route, so the row is inserted directly.
async fn second_library(pool: &sqlx::PgPool) -> LibraryId {
    let id: uuid::Uuid = "01931b6e-0000-7000-8000-0000000000a2"
        .parse()
        .expect("valid uuid");
    sqlx::query("INSERT INTO library (id, name) VALUES ($1, 'Fixture jigs')")
        .bind(id)
        .execute(pool)
        .await
        .expect("seeds a second library");
    LibraryId::from_uuid(id)
}

#[sqlx::test(migrations = "./migrations")]
async fn a_hash_another_library_holds_is_not_held_by_this_one(pool: sqlx::PgPool) {
    // The distinction `PgBlobs::exists` cannot make, and the reason ingest must not
    // short-circuit on it: knowing the bytes is not the same as holding the part.
    // Scanning into an empty second library used to report six files skipped and leave
    // the library empty, because these two questions were answered by one query.
    let blob = blob_row(0x33);
    let blobs = PgBlobs(pool.clone());
    let other = second_library(&pool).await;
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Vee block, LP-3072-02",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");

    assert!(
        blobs.exists(&blob.hash).await.expect("query"),
        "the bytes are held — globally"
    );
    assert!(
        blobs
            .library_holds(library(), "Vee block, LP-3072-02", &blob.hash)
            .await
            .expect("query"),
        "the library that was scanned into holds the part"
    );
    assert!(
        !blobs
            .library_holds(other, "Vee block, LP-3072-02", &blob.hash)
            .await
            .expect("query"),
        "a different library does not hold it, however well known the hash is"
    );
    assert!(
        !blobs
            .library_holds(library(), "Vee block copy, LP-3072-02", &blob.hash)
            .await
            .expect("query"),
        "a different name is a different part, even byte for byte"
    );
    assert!(
        !blobs
            .library_holds(library(), "Vee block, LP-3072-02", &blob_row(0x44).hash)
            .await
            .expect("query"),
        "a hash nothing has ingested is held by no library"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_same_part_name_in_two_libraries_is_not_refused(pool: sqlx::PgPool) {
    // Negative control for `part_name_unique_per_library`
    // (tests/migrations.rs::two_parts_with_one_name_in_one_library_are_refused). That
    // test alone can't tell a correctly-scoped `UNIQUE (library_id, name)` from an
    // accidentally-global `UNIQUE (name)` — it never inserts into a second library, and
    // being scoped per library is the entire point (spec §3.5). This is the other half:
    // the same name in a *different* library must succeed, not just the same name in
    // the same library must fail.
    let other = second_library(&pool).await;

    sqlx::query("INSERT INTO part (id, library_id, name) VALUES (gen_random_uuid(), $1, $2)")
        .bind(library().as_uuid())
        .bind("bracket-lp-1042-03")
        .execute(&pool)
        .await
        .expect("the first library's part inserts");

    sqlx::query("INSERT INTO part (id, library_id, name) VALUES (gen_random_uuid(), $1, $2)")
        .bind(other.as_uuid())
        .bind("bracket-lp-1042-03")
        .execute(&pool)
        .await
        .expect("the same name in a different library must not be refused");
}

#[sqlx::test(migrations = "./migrations")]
async fn linking_an_existing_blob_adds_a_part_without_touching_ref_count_twice(pool: sqlx::PgPool) {
    let blob = blob_row(0x22);
    let ingest = PgIngest(pool.clone());
    // Bound once, outside the closure: `measurements: &watertight()` inside the closure
    // body borrows a temporary that does not outlive the returned `IngestRequest`
    // (E0515) — the brief's original listing does not compile.
    let measurements = watertight();
    let req = |name: &'static str| IngestRequest {
        library: library(),
        name,
        blob: &blob,
        measurements: &measurements,
        kernel_version: "mesh stl-1+cpu-1",
        thumbnail_webp: b"webp",
    };
    ingest
        .record(req("Bracket, LP-1042-03"))
        .await
        .expect("first");
    ingest
        .link_existing(req("Bracket copy, LP-1042-03"))
        .await
        .expect("second");

    let ref_count: i32 = sqlx::query_scalar("SELECT ref_count FROM blob WHERE blake3 = $1")
        .bind(blob.hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(ref_count, 2, "each file referencing the blob counts once");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_grid_page_returns_newest_first_with_a_thumbnail_hash(pool: sqlx::PgPool) {
    let ingest = PgIngest(pool.clone());
    for (i, name) in [
        "Bracket, LP-1042-03",
        "Spacer, LP-2001-00",
        "Cable clip, LP-3300-01",
    ]
    .iter()
    .enumerate()
    {
        ingest
            .record(IngestRequest {
                library: library(),
                name,
                blob: &blob_row(0x30 + i as u8),
                measurements: &watertight(),
                kernel_version: "mesh stl-1+cpu-1",
                thumbnail_webp: b"webp",
            })
            .await
            .expect("records");
    }

    let page = PgParts(pool.clone())
        .page(library(), None, 2)
        .await
        .expect("page");
    assert_eq!(page.len(), 2, "limit is honoured");
    assert_eq!(
        page[0].summary.name, "Cable clip, LP-3300-01",
        "newest first"
    );
    assert!(
        page[0].summary.approximate,
        "every mesh-derived part is approximate"
    );
    assert_eq!(page[0].summary.triangle_count, Some(48_112));
    assert_eq!(
        page[0].thumbnail_webp.as_deref(),
        Some(b"webp".as_slice()),
        "the inline thumbnail bytes travel with the row, not just a hash"
    );

    let next = PgParts(pool.clone())
        .page(library(), Some(page[1].summary.id), 2)
        .await
        .expect("second page");
    assert_eq!(
        next.len(),
        1,
        "keyset pagination continues after the last id"
    );
    assert_eq!(next[0].summary.name, "Bracket, LP-1042-03");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_soft_deleted_part_never_appears_in_the_grid(pool: sqlx::PgPool) {
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x40),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");

    let page = PgParts(pool).page(library(), None, 50).await.expect("page");
    assert!(
        page.is_empty(),
        "delete is soft, but soft-deleted parts are still hidden"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_grid_shows_the_newer_revisions_numbers_not_the_older_ones(pool: sqlx::PgPool) {
    // F2: `PgParts::page`'s revision LATERAL orders by `created_at DESC` (with an
    // `id DESC` tie-break for a tie in that same instant — see the tie-break tests'
    // notes on why that half stays unpinned). Flipping DESC to ASC on the primary key
    // leaves the whole workspace green otherwise, because nothing else exercises a
    // part with more than one revision: `insert_part_chain` is the only writer today
    // and it always creates a brand-new part, so this seeds the second revision
    // directly. "Measurement must not lie" — this is the query deciding which
    // revision's numbers the grid shows, so a regression here is a stale figure shown
    // as current, not a crash.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x51),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"older-thumbnail",
        })
        .await
        .expect("records");

    let newer_revision: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO revision (id, part_id, rev_label, origin, created_at, triangle_count, is_watertight) SELECT gen_random_uuid(), part_id, '2', 'ingest', created_at + interval '1 hour', 99999, true FROM revision WHERE part_id = $1 RETURNING id",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("insert a strictly newer revision");
    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json) VALUES (gen_random_uuid(), $1, 'thumbnail', $2, 'mesh stl-1+cpu-1', '{}')",
    )
    .bind(newer_revision.0)
    .bind(b"newer-thumbnail".as_slice())
    .execute(&pool)
    .await
    .expect("insert the newer revision's own derivative");

    let page = PgParts(pool).page(library(), None, 10).await.expect("page");
    assert_eq!(page.len(), 1, "still one part");
    assert_eq!(
        page[0].summary.triangle_count,
        Some(99999),
        "the newer revision's triangle count, not the one it was ingested with"
    );
    assert_eq!(
        page[0].thumbnail_webp.as_deref(),
        Some(b"newer-thumbnail".as_slice()),
        "the newer revision's own thumbnail, not the original ingest's"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_thumbnail_on_one_revision_is_refused_by_the_schema(pool: sqlx::PgPool) {
    // F6, superseded: this used to seed a second `kind = 'thumbnail'` row directly to
    // prove the grid query's LATERAL join wouldn't fan out on it. Migration 0003 (slice
    // 1 ledger item S3) closed that hole a layer earlier: `derivative_kind_unique_per_revision`
    // now makes a second row of the same kind on one revision unrepresentable at all, so
    // the old fixture can no longer be constructed — the INSERT itself is refused before
    // the grid query ever runs. What's left to assert is that refusal.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x60),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"first-thumbnail",
        })
        .await
        .expect("records");

    let err = sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json, created_at) SELECT gen_random_uuid(), revision_id, 'thumbnail', $2, kernel_version, params_json, created_at + interval '1 hour' FROM derivative WHERE revision_id = (SELECT id FROM revision WHERE part_id = $1)",
    )
    .bind(id.as_uuid())
    .bind(b"second-thumbnail".as_slice())
    .execute(&pool)
    .await
    .expect_err("a second thumbnail derivative for the same revision must be refused");

    assert!(
        err.to_string()
            .contains("derivative_kind_unique_per_revision"),
        "expected the named constraint to be what refused it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_of_a_different_kind_does_not_duplicate_the_grid_row(pool: sqlx::PgPool) {
    // Fan-out pin: `derivative_kind_unique_per_revision` is scoped to (revision_id,
    // kind), not to revision_id alone — `kind` is a plain `text` column with no CHECK,
    // so nothing stops a revision from legitimately carrying several derivatives of
    // *different* kinds. Slice 3's LOD ladder does exactly that. `PgParts::page`'s
    // derivative LATERAL is what keeps that from fanning the grid out into duplicate
    // cards; this seeds a second kind and pins that it still doesn't, independent of
    // the unique constraint (a different kind never touches it) and independent of the
    // WHERE kind = 'thumbnail' filter inside the LATERAL not regressing later.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x61),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"the-thumbnail",
        })
        .await
        .expect("records");

    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json, created_at) SELECT gen_random_uuid(), revision_id, 'lod0', $2, kernel_version, params_json, created_at + interval '1 hour' FROM derivative WHERE revision_id = (SELECT id FROM revision WHERE part_id = $1)",
    )
    .bind(id.as_uuid())
    .bind(b"lod0-mesh-bytes".as_slice())
    .execute(&pool)
    .await
    .expect("insert a same-revision derivative of a different kind");

    let page = PgParts(pool).page(library(), None, 10).await.expect("page");
    assert_eq!(
        page.len(),
        1,
        "one part must still be one grid row, however many derivative kinds its latest revision has"
    );
    assert_eq!(
        page[0].thumbnail_webp.as_deref(),
        Some(b"the-thumbnail".as_slice()),
        "the thumbnail derivative, not the lod0 one, is what the grid card shows"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_negative_triangle_count_in_the_column_is_reported_not_reinterpreted(pool: sqlx::PgPool) {
    // F5 (read side): `as u32` on a negative i32 column wraps to a huge positive
    // number (-7 becomes 4_294_967_289) instead of failing, the same silent-wraparound
    // shape TimestampOutOfRange already refuses to allow for a corrupt timestamp.
    // insert_part_chain cannot write a negative value itself (it stores a u32
    // unconditionally), so this is only reachable via a row written by something else
    // — exactly what the error message says.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Bracket, LP-1042-03",
            blob: &blob_row(0x70),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect("records");
    sqlx::query("UPDATE revision SET triangle_count = -7 WHERE part_id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .expect("corrupt the column directly");

    let err = PgParts(pool)
        .page(library(), None, 10)
        .await
        .expect_err("a negative triangle count must be reported, not reinterpreted");
    match err {
        DbError::NegativeTriangleCount { column, value } => {
            assert_eq!(column, "revision.triangle_count");
            assert_eq!(value, -7);
        }
        other => panic!("expected NegativeTriangleCount, got {other:?}"),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_triangle_count_too_large_for_the_column_is_rejected_on_write(pool: sqlx::PgPool) {
    // F5 (write side): `m.triangle_count as i32` silently wrapped a count above
    // i32::MAX into a negative number instead of failing — 3_000_000_000 stored as
    // -1_294_967_296, and the read path's (former) `as u32` would have round-tripped
    // it straight back to 3_000_000_000, making the API look correct while the column
    // itself was wrong for every SQL-level consumer that isn't this endpoint.
    let oversized = MeshMeasurements {
        bbox_mm: [10.0, 10.0, 10.0],
        triangle_count: 3_000_000_000,
        surface_area_mm2: 100.0,
        volume_mm3: Some(50.0),
        is_watertight: true,
    };
    let err = PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name: "Implausible mesh",
            blob: &blob_row(0x71),
            measurements: &oversized,
            kernel_version: "mesh stl-1+cpu-1",
            thumbnail_webp: b"webp",
        })
        .await
        .expect_err("a triangle count this large must be rejected, not wrapped");
    match err {
        DbError::TriangleCountTooLarge { column, value } => {
            assert_eq!(column, "revision.triangle_count");
            assert_eq!(value, 3_000_000_000);
        }
        other => panic!("expected TriangleCountTooLarge, got {other:?}"),
    }

    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(
        parts, 0,
        "a rejected triangle count must leave no partial part/revision row behind"
    );
}
