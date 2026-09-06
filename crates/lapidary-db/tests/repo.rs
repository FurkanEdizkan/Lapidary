use lapidary_core::{BlobHash, DerivativeKind, LibraryId, MeshMeasurements, PartId, RevisionId};
use lapidary_db::{
    DbError, DerivativeBytes, IngestRequest, PartRepository, PgBlobs, PgIngest, PgParts,
    StoredBlobRow, TessellationRow,
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bearing block, 608ZZ",
            source_path: "bearing-block-608zz.stl",
            blob: &blob,
            measurements: &watertight(),
            thumbnail_webp: Some(b"webp bytes"),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Cable clip, LP-3300-01",
            source_path: "cable-clip-lp-3300-01.stl",
            blob: &blob,
            measurements: &open_mesh(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Spacer, LP-2001-00",
            source_path: "spacer-lp-2001-00.stl",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Vee block, LP-3072-02",
            source_path: "vee-block-lp-3072-02.stl",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
        })
        .await
        .expect("records");

    assert!(
        blobs.exists(&blob.hash).await.expect("query"),
        "the bytes are held — globally"
    );
    assert!(
        blobs
            .library_holds(library(), "vee-block-lp-3072-02.stl", &blob.hash)
            .await
            .expect("query"),
        "the library that was scanned into holds the part"
    );
    assert!(
        !blobs
            .library_holds(other, "vee-block-lp-3072-02.stl", &blob.hash)
            .await
            .expect("query"),
        "a different library does not hold it, however well known the hash is"
    );
    assert!(
        !blobs
            .library_holds(library(), "copies/vee-block-lp-3072-02.stl", &blob.hash)
            .await
            .expect("query"),
        "a different path is a different part, even byte for byte -- and since slice 6a \
         the same NAME at a different path is too"
    );
    assert!(
        !blobs
            .library_holds(library(), "vee-block-lp-3072-02.stl", &blob_row(0x44).hash)
            .await
            .expect("query"),
        "a hash nothing has ingested is held by no library"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_same_source_path_in_two_libraries_is_not_refused(pool: sqlx::PgPool) {
    // Negative control for `part_source_path_unique_per_library`
    // (tests/migrations.rs::two_parts_at_one_source_path_in_one_library_are_refused).
    // That test alone can't tell a correctly-scoped `UNIQUE (library_id, source_path)`
    // from an accidentally-global `UNIQUE (source_path)` — it never inserts into a second
    // library, and being scoped per library is the entire point (spec §3.5). This is the
    // other half: the same path in a *different* library must succeed, not just the same
    // path in the same library must fail.
    //
    // It matters more since slice 6a than it did on the name, because two libraries
    // mounted at two roots routinely hold the same relative path.
    let other = second_library(&pool).await;

    let insert = |library: uuid::Uuid| {
        sqlx::query(
            "INSERT INTO part (id, library_id, name, source_path) \
             VALUES (gen_random_uuid(), $1, $2, $3)",
        )
        .bind(library)
        .bind("bracket-lp-1042-03")
        .bind("brackets/bracket-lp-1042-03.stl")
        .execute(&pool)
    };

    insert(library().as_uuid())
        .await
        .expect("the first library's part inserts");
    insert(other.as_uuid())
        .await
        .expect("the same path in a different library must not be refused");
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
        folder: None,
        storage_path: None,
        library: library(),
        name,
        source_path: name,
        blob: &blob,
        measurements: &measurements,
        kernel_version: "mesh stl-1+cpu-1",
        format: "stl",
        tessellations: &[],
        thumbnail_webp: Some(b"webp"),
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
                folder: None,
                storage_path: None,
                library: library(),
                name,
                source_path: name,
                blob: &blob_row(0x30 + i as u8),
                measurements: &watertight(),
                kernel_version: "mesh stl-1+cpu-1",
                format: "stl",
                tessellations: &[],
                thumbnail_webp: Some(b"webp"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0x40),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0x51),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"older-thumbnail"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0x60),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"first-thumbnail"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0x61),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"the-thumbnail"),
        })
        .await
        .expect("records");

    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, thumb_bytes, kernel_version, params_json, created_at) SELECT gen_random_uuid(), revision_id, 'tessellation_l0', $2, kernel_version, params_json, created_at + interval '1 hour' FROM derivative WHERE revision_id = (SELECT id FROM revision WHERE part_id = $1)",
    )
    .bind(id.as_uuid())
    .bind(b"tessellation-l0-bytes".as_slice())
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
        "the thumbnail derivative, not the tessellation_l0 one, is what the grid card shows"
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0x70),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
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
            folder: None,
            storage_path: None,
            library: library(),
            name: "Implausible mesh",
            source_path: "implausible-mesh.stl",
            blob: &blob_row(0x71),
            measurements: &oversized,
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
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

/// Seeds one part and returns its id, so a test can reach the revision for a direct
/// INSERT. The derivative constraints below are about what the *database* refuses, which
/// `PgIngest` cannot express — it only ever writes rows that are already valid.
async fn seeded_part(pool: &sqlx::PgPool, seed: u8) -> lapidary_core::PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(seed),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"the-thumbnail"),
        })
        .await
        .expect("records")
}

/// INSERT a derivative directly, with whichever storage columns the caller wants.
async fn insert_derivative(
    pool: &sqlx::PgPool,
    part: lapidary_core::PartId,
    kind: &str,
    blake3: Option<String>,
    thumb_bytes: Option<&[u8]>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO derivative (id, revision_id, kind, blake3, thumb_bytes, kernel_version, params_json) \
         SELECT gen_random_uuid(), id, $2, $3, $4, 'mesh stl-1+glb-1+cpu-1', '{}'::jsonb \
         FROM revision WHERE part_id = $1",
    )
    .bind(part.as_uuid())
    .bind(kind)
    .bind(blake3)
    .bind(thumb_bytes)
    .execute(pool)
    .await
}

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_stored_both_inline_and_by_hash_is_rejected(pool: sqlx::PgPool) {
    let part = seeded_part(&pool, 0x71).await;
    // The hash is one `record` really wrote, so the foreign key is satisfied and the
    // exclusivity CHECK is unambiguously what refuses this.
    let existing = blob_row(0x71).hash.to_hex();

    let err = insert_derivative(
        &pool,
        part,
        "tessellation_l0",
        Some(existing),
        Some(b"and-also-inline"),
    )
    .await
    .expect_err("a derivative may not claim both storages");

    assert!(
        err.to_string().contains("derivative_storage_is_exclusive"),
        "expected the named constraint to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_stored_neither_way_is_rejected(pool: sqlx::PgPool) {
    // The case that has been legal since 0002 and is the reason this constraint exists: a
    // row describing a derivative that cannot be served, because nothing holds its bytes.
    let part = seeded_part(&pool, 0x72).await;

    let err = insert_derivative(&pool, part, "tessellation_l0", None, None)
        .await
        .expect_err("a derivative must be stored somewhere");

    assert!(
        err.to_string().contains("derivative_storage_is_exclusive"),
        "expected the named constraint to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_naming_a_blob_that_does_not_exist_is_rejected(pool: sqlx::PgPool) {
    let part = seeded_part(&pool, 0x73).await;
    let absent = BlobHash::from_bytes([0xff; 32]).to_hex();

    let err = insert_derivative(&pool, part, "tessellation_l0", Some(absent), None)
        .await
        .expect_err("a derivative may not reference a blob that was never stored");

    assert!(
        err.to_string()
            .contains("derivative_blake3_references_blob"),
        "expected the foreign key to refuse it, got: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_derivative_stored_by_hash_against_a_real_blob_is_accepted(pool: sqlx::PgPool) {
    // Not filler. A CHECK written with `and` instead of `<>` refuses everything, and would
    // pass all three negative cases above while making the LOD ladder unwritable.
    let part = seeded_part(&pool, 0x74).await;
    let existing = blob_row(0x74).hash.to_hex();

    insert_derivative(&pool, part, "tessellation_l0", Some(existing), None)
        .await
        .expect("a rung stored by hash against a real blob is exactly what slice 3 writes");
}

fn rung(kind: &str, seed: u8, grid: Option<u32>) -> TessellationRow<'_> {
    TessellationRow {
        kind,
        blob: StoredBlobRow {
            hash: BlobHash::from_bytes([seed; 32]),
            size_bytes: 40_960,
            // Derivatives are never compressed, so these two are equal and the level is
            // unset. `insert_part_chain` writes them that way regardless of what is here.
            stored_bytes: 40_960,
            zstd_level: 0,
        },
        grid,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn three_tessellations_and_a_thumbnail_coexist_on_one_revision(pool: sqlx::PgPool) {
    let rungs = [
        rung("tessellation_l0", 0x81, Some(32)),
        rung("tessellation_l1", 0x82, Some(96)),
        rung("tessellation_l2", 0x83, None),
    ];
    PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0x80),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &rungs,
            thumbnail_webp: Some(b"the-thumbnail"),
        })
        .await
        .expect("records");

    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM derivative d JOIN revision r ON r.id = d.revision_id \
         JOIN part p ON p.id = r.part_id WHERE p.library_id = $1 ORDER BY kind",
    )
    .bind(library().as_uuid())
    .fetch_all(&pool)
    .await
    .expect("kinds");
    assert_eq!(
        kinds,
        vec![
            "tessellation_l0",
            "tessellation_l1",
            "tessellation_l2",
            "thumbnail"
        ]
    );

    // The rungs go by hash and the thumbnail inline, which is the split migration 0004's
    // CHECK enforces. A rung large enough to need the blob store is the reason that
    // column exists at all.
    let by_hash: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM derivative d JOIN revision r ON r.id = d.revision_id \
         JOIN part p ON p.id = r.part_id \
         WHERE p.library_id = $1 AND d.blake3 IS NOT NULL AND d.thumb_bytes IS NULL",
    )
    .bind(library().as_uuid())
    .fetch_all(&pool)
    .await
    .map(|rows: Vec<i64>| rows[0])
    .expect("count");
    assert_eq!(by_hash, 3);

    // params_json carries the grid, so kernel_version and params_json together say how to
    // regenerate these exact bytes -- the property that lets a derivative be evicted.
    let grids: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT params_json FROM derivative d JOIN revision r ON r.id = d.revision_id \
         JOIN part p ON p.id = r.part_id \
         WHERE p.library_id = $1 AND d.kind LIKE 'tessellation%' ORDER BY d.kind",
    )
    .bind(library().as_uuid())
    .fetch_all(&pool)
    .await
    .expect("params");
    assert_eq!(
        grids,
        vec![
            serde_json::json!({ "grid": 32 }),
            serde_json::json!({ "grid": 96 }),
            serde_json::json!({ "grid": null }),
        ]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_rung_shared_between_two_revisions_is_one_blob_with_ref_count_two(pool: sqlx::PgPool) {
    // Two different parts whose L0 clusters to identical bytes -- the ordinary case for
    // anything under the L0 budget, where every rung is the source mesh itself.
    let shared = 0x91;
    for (seed, name) in [(0x90, "Bracket, LP-1042-03"), (0x92, "Spacer, LP-2001-00")] {
        PgIngest(pool.clone())
            .record(IngestRequest {
                folder: None,
                storage_path: None,
                library: library(),
                name,
                source_path: name,
                blob: &blob_row(seed),
                measurements: &watertight(),
                kernel_version: "mesh stl-1+glb-1+cpu-1",
                format: "stl",
                tessellations: &[rung("tessellation_l0", shared, Some(32))],
                thumbnail_webp: Some(b"the-thumbnail"),
            })
            .await
            .expect("records");
    }

    let hex = BlobHash::from_bytes([shared; 32]).to_hex();
    let (rows, ref_count): (i64, i32) =
        sqlx::query_as("SELECT count(*), max(ref_count) FROM blob WHERE blake3 = $1")
            .bind(&hex)
            .fetch_one(&pool)
            .await
            .expect("blob");
    assert_eq!(rows, 1, "identical rung bytes are stored once, not twice");
    assert_eq!(
        ref_count, 2,
        "each derivative pointing at these bytes holds a reference, or the reap frees \
         bytes another revision is still serving"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_file_row_records_the_format_it_was_given(pool: sqlx::PgPool) {
    PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Idler Bracket, LP-2210-01",
            source_path: "idler-bracket-lp-2210-01.obj",
            blob: &blob_row(0xa0),
            measurements: &open_mesh(),
            kernel_version: "mesh obj-1+glb-1+cpu-1",
            format: "obj",
            tessellations: &[],
            thumbnail_webp: Some(b"the-thumbnail"),
        })
        .await
        .expect("records");

    let format: String = sqlx::query_scalar(
        "SELECT f.format FROM file f JOIN revision r ON r.id = f.revision_id \
         JOIN part p ON p.id = r.part_id WHERE p.library_id = $1",
    )
    .bind(library().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("format");
    // Was the SQL literal 'stl'. An OBJ recorded as an STL is a lie that survives into
    // every later read of the row.
    assert_eq!(format, "obj");
}

/// The one revision `insert_part_chain` writes for a part. Read straight out of the
/// table rather than through `PgParts::latest_revision`, so that a test of the write side
/// cannot fail for a reason that lives in the resolver.
async fn only_revision(pool: &sqlx::PgPool, part: PartId) -> RevisionId {
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM revision WHERE part_id = $1")
        .bind(part.as_uuid())
        .fetch_one(pool)
        .await
        .expect("the ingested revision");
    RevisionId::from_uuid(id)
}

/// How many rows currently claim these bytes. The number eviction will one day trust.
async fn ref_count_of(pool: &sqlx::PgPool, hash: &BlobHash) -> i32 {
    sqlx::query_scalar("SELECT ref_count FROM blob WHERE blake3 = $1")
        .bind(hash.to_hex())
        .fetch_one(pool)
        .await
        .expect("blob row")
}

/// One part in `library`, with or without a preview. The sweep fixture below needs five
/// of them and cares only about the thumbnail.
async fn seed_part(
    ingest: &PgIngest,
    library: LibraryId,
    name: &str,
    blob: u8,
    thumbnail: Option<&[u8]>,
) -> PartId {
    ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library,
            name,
            source_path: name,
            blob: &blob_row(blob),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: thumbnail,
        })
        .await
        .expect("records")
}

fn rung_blob(seed: u8) -> StoredBlobRow {
    StoredBlobRow {
        hash: BlobHash::from_bytes([seed; 32]),
        size_bytes: 40_960,
        stored_bytes: 40_960,
        zstd_level: 0,
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_part_ingested_without_a_thumbnail_still_appears_in_the_grid(pool: sqlx::PgPool) {
    // The one that matters: a library with `auto_thumbnail = false` ingests parts with no
    // preview at all, and every one of them must still be in the grid. The `LEFT JOIN
    // LATERAL` is what guarantees it, so it is asserted rather than assumed.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0xb0),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");

    let page = PgParts(pool.clone())
        .page(library(), None, 10)
        .await
        .expect("page");
    assert_eq!(page.len(), 1, "a part with no preview is still a part");
    assert_eq!(page[0].summary.name, "Bracket, LP-1042-03");
    assert_eq!(page[0].summary.triangle_count, Some(48_112));
    assert_eq!(
        page[0].thumbnail_webp, None,
        "no bytes at all, which the grid renders as \"no preview yet\""
    );

    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM derivative d JOIN revision r ON r.id = d.revision_id \
         WHERE r.part_id = $1",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(
        rows, 0,
        "no thumbnail means no row: an empty bytea is not NULL, so it would pass \
         derivative_storage_is_exclusive, read back as Some(vec![]) and reach the grid \
         as a broken image"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_grid_reports_what_a_part_costs_on_disk(pool: sqlx::PgPool) {
    // Two source blobs that differ the way the ingest policy makes them differ: an STL
    // goes through zstd (`Compression::for_source_format`), a 3MF is already a zip and
    // is stored `AsIs`. Written out here rather than through `blob_row`, because the
    // whole assertion is that these two rows read back differently — a fixture pair
    // that happened to carry the same sizes would pass whatever the query reported.
    let stl = StoredBlobRow {
        hash: BlobHash::from_bytes([0xc1; 32]),
        size_bytes: 204_800,
        stored_bytes: 91_204,
        zstd_level: 3,
    };
    let three_mf = StoredBlobRow {
        hash: BlobHash::from_bytes([0xc2; 32]),
        size_bytes: 61_294,
        stored_bytes: 61_294,
        zstd_level: 0,
    };
    assert!(
        stl.stored_bytes < stl.size_bytes,
        "the compressed fixture has to actually compress, or reporting size_bytes for \
         both columns would look correct"
    );

    let ingest = PgIngest(pool.clone());
    let bracket = ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &stl,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
        })
        .await
        .expect("records the compressed source");
    let impeller = ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Impeller, LP-5501-02",
            source_path: "impeller-lp-5501-02.3mf",
            blob: &three_mf,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "3mf",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
        })
        .await
        .expect("records the AsIs source");

    let page = PgParts(pool.clone())
        .page(library(), None, 10)
        .await
        .expect("page");
    assert_eq!(page.len(), 2);
    let row = |id: PartId| {
        page.iter()
            .find(|row| row.summary.id == id)
            .map(|row| &row.summary)
            .expect("the part is in the page")
    };

    let compressed = row(bracket);
    assert_eq!(compressed.source_bytes, Some(204_800));
    assert_eq!(
        compressed.stored_bytes,
        Some(91_204),
        "the size on disk, not the size ingested — the whole point of showing both"
    );
    assert_eq!(compressed.compressed, Some(true));
    assert_eq!(compressed.source_hash, Some(stl.hash));
    assert_eq!(
        compressed.revision,
        only_revision(&pool, bracket).await,
        "the card names the revision its numbers came from, which is what a download \
         link is built out of"
    );

    let as_is = row(impeller);
    assert_eq!(as_is.source_bytes, Some(61_294));
    assert_eq!(
        as_is.stored_bytes,
        Some(61_294),
        "a 3MF is stored as it arrived, so the two figures agree"
    );
    assert_eq!(as_is.compressed, Some(false));
    assert_eq!(as_is.source_hash, Some(three_mf.hash));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_card_and_the_download_name_the_same_source_file(pool: sqlx::PgPool) {
    // `PgParts::page` and `PgParts::source_for_download` both claim, in their own
    // comments, to resolve the same `file` row: same `role = 'source'` filter, same
    // `created_at DESC, id DESC`. `file` carries no unique constraint on
    // `(revision_id, role)`, so that agreement is a choice rather than something the
    // schema enforces — and one revision with one file row cannot tell whether either
    // query still makes it. Three rows on one revision can. Stop filtering on the role
    // and the card advertises the render's 777 bytes while the button serves half a
    // megabyte of STL; reverse either ordering and the card describes the file the
    // re-upload replaced.
    let ingest = PgIngest(pool.clone());
    let id = seed_part(
        &ingest,
        library(),
        "Manifold block, LP-2210-04",
        0xe1,
        Some(b"webp"),
    )
    .await;
    let revision = only_revision(&pool, id).await;
    let reupload = BlobHash::from_bytes([0xe2; 32]);
    let render = BlobHash::from_bytes([0xe3; 32]);

    // Sizes deliberately unlike `blob_row`'s and unlike each other: a card that reads
    // the wrong row has to read visibly wrong numbers, not the same ones twice.
    sqlx::query(
        "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
         VALUES ($1, 512000, 218640, 3, 1), ($2, 777, 777, 0, 1)",
    )
    .bind(reupload.to_hex())
    .bind(render.to_hex())
    .execute(&pool)
    .await
    .expect("a blob for the re-uploaded STL and one for the render");
    // Explicit timestamps rather than three statements racing `now()`: the ordering is
    // the whole assertion, so it is written down instead of inferred from insert order.
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, created_at) \
         VALUES (gen_random_uuid(), $1, 'source', 'stl', $2, 512000, now() + interval '1 minute'), \
         (gen_random_uuid(), $1, 'render', 'png', $3, 777, now() + interval '2 minutes')",
    )
    .bind(revision.as_uuid())
    .bind(reupload.to_hex())
    .bind(render.to_hex())
    .execute(&pool)
    .await
    .expect("a newer source row and a newer row of another role");

    let parts = PgParts(pool.clone());
    let page = parts.page(library(), None, 10).await.expect("page");
    assert_eq!(page.len(), 1, "three file rows are still one part");
    let card = &page[0].summary;
    let download = parts
        .source_for_download(revision)
        .await
        .expect("query")
        .expect("a live part has something to download");

    assert_eq!(
        card.source_hash,
        Some(download.hash),
        "the figures on the card and the bytes behind its download link have to come \
         off one `file` row, or the card is advertising a file the button will not serve"
    );
    assert_eq!(
        card.source_hash,
        Some(reupload),
        "and that row is the newest source one, not the render and not the original"
    );
    assert_eq!(card.source_bytes, Some(512_000));
    assert_eq!(card.stored_bytes, Some(218_640));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_source_blob_whose_level_nobody_recorded_reads_as_uncompressed(pool: sqlx::PgPool) {
    // Built through the path that actually produces this state rather than by an UPDATE
    // over `blob`, because whether it is reachable at all is half of what is being
    // asserted. `insert_part_chain` writes every tessellation blob with `zstd_level
    // NULL`, and the ingest handler routes bytes it already holds to `link_existing`,
    // which leaves that row exactly as it found it. So a file whose bytes are
    // byte-identical to an existing rung lands as a `role = 'source'` row over a
    // NULL-level blob. Contrived under an STL-only scan; not unreachable.
    let ingest = PgIngest(pool.clone());
    let rungs = [rung("tessellation_l0", 0xe6, Some(32))];
    ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0xe5),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &rungs,
            thumbnail_webp: Some(b"webp-bracket"),
        })
        .await
        .expect("records the part whose rung the next part's bytes are identical to");

    // Exactly what the handler passes on that branch: `bytes.len()` for both sizes and a
    // level it does not get to choose, because the `blob` row already exists.
    let duplicate = StoredBlobRow {
        hash: BlobHash::from_bytes([0xe6; 32]),
        size_bytes: 40_960,
        stored_bytes: 40_960,
        zstd_level: 0,
    };
    let clip = ingest
        .link_existing(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Cable clip, LP-3300-01",
            source_path: "cable-clip-lp-3300-01.stl",
            blob: &duplicate,
            measurements: &open_mesh(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp-clip"),
        })
        .await
        .expect("links the part onto bytes the database already holds");

    let level: Option<i16> = sqlx::query_scalar("SELECT zstd_level FROM blob WHERE blake3 = $1")
        .bind(duplicate.hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("the rung's blob row");
    assert_eq!(
        level, None,
        "link_existing must have left the rung's blob row alone, or this fixture is not \
         the state it claims to be"
    );

    let parts = PgParts(pool.clone());
    let page = parts.page(library(), None, 10).await.expect("page");
    let card = page
        .iter()
        .find(|row| row.summary.id == clip)
        .map(|row| &row.summary)
        .expect("the linked part is in the page");
    assert_eq!(
        card.source_hash,
        Some(duplicate.hash),
        "there is a source row, so the field below is about its level and not its absence"
    );
    assert_eq!(
        card.compressed,
        Some(false),
        "`false`, never `None`: `None` on this field means no source row at all, which \
         is a different fact and is asserted next door"
    );

    // The other half of that decision. The card declines to be where a data error
    // surfaces; the download route is where it surfaces, and refuses to serve the bytes
    // rather than reading them raw (spec §2.5.1). Asserted here because this fixture is
    // the proof that the 500 is reachable from an ordinary ingest.
    let download = parts
        .source_for_download(only_revision(&pool, clip).await)
        .await
        .expect("query")
        .expect("a live part has something to download");
    assert_eq!(
        download.zstd_level, None,
        "the unrecorded level the download route answers 500 for, reached without \
         corrupting a single row by hand"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_negative_size_in_the_column_is_reported_not_reinterpreted(pool: sqlx::PgPool) {
    // The last of `bytes_column`'s four sibling guards without a test, and the one whose
    // absence is easiest to justify wrongly: neither `blob.size_bytes` nor
    // `blob.stored_bytes` carries a CHECK constraint, so a negative row is representable
    // by anything else with write access, and `as u64` would put 18 exabytes on a card
    // instead of saying the row is wrong. Same shape as its triangle-count neighbour.
    let ingest = PgIngest(pool.clone());
    seed_part(
        &ingest,
        library(),
        "Impeller, LP-5501-02",
        0xe8,
        Some(b"webp"),
    )
    .await;
    sqlx::query("UPDATE blob SET stored_bytes = -1 WHERE blake3 = $1")
        .bind(BlobHash::from_bytes([0xe8; 32]).to_hex())
        .execute(&pool)
        .await
        .expect("corrupt the column directly");

    let err = PgParts(pool)
        .page(library(), None, 10)
        .await
        .expect_err("a negative size must be reported, not reinterpreted");
    match err {
        DbError::NegativeByteCount { column, value } => {
            assert_eq!(column, "blob.stored_bytes");
            assert_eq!(value, -1);
        }
        other => panic!("expected NegativeByteCount, got {other:?}"),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_revision_with_no_source_file_still_appears_in_the_grid(pool: sqlx::PgPool) {
    // The source LATERAL has to be a LEFT one, and nothing else proves it: ingest always
    // writes a `file` row, so `a_part_ingested_without_a_thumbnail_still_appears_in_the
    // _grid` would stay green with an inner join here. A part whose source row is gone is
    // a part its owner most needs to see — to delete it, or to re-scan it — and hiding it
    // is how a half-repaired database becomes an invisible one.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Cable clip, LP-3300-01",
            source_path: "cable-clip-lp-3300-01.stl",
            blob: &blob_row(0xc3),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp"),
        })
        .await
        .expect("records");
    sqlx::query(
        "DELETE FROM file f USING revision r WHERE f.revision_id = r.id AND r.part_id = $1",
    )
    .bind(id.as_uuid())
    .execute(&pool)
    .await
    .expect("removes the source file row");

    let page = PgParts(pool.clone())
        .page(library(), None, 10)
        .await
        .expect("page");
    assert_eq!(page.len(), 1, "the part is still in the grid");
    assert_eq!(page[0].summary.source_hash, None);
    assert_eq!(page[0].summary.source_bytes, None);
    assert_eq!(page[0].summary.stored_bytes, None);
    assert_eq!(
        page[0].summary.compressed, None,
        "absent, not false: there is no source row to be uncompressed"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn upserting_a_thumbnail_twice_leaves_one_row_holding_the_second_bytes(pool: sqlx::PgPool) {
    let ingest = PgIngest(pool.clone());
    let id = ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Spacer, LP-2001-00",
            source_path: "spacer-lp-2001-00.stl",
            blob: &blob_row(0xb1),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");
    let revision = only_revision(&pool, id).await;

    ingest
        .upsert_derivative(
            revision,
            DerivativeKind::Thumbnail,
            DerivativeBytes::Inline(b"first render"),
            "mesh stl-1+cpu-1",
        )
        .await
        .expect("the first render writes a row");
    ingest
        .upsert_derivative(
            revision,
            DerivativeKind::Thumbnail,
            DerivativeBytes::Inline(b"second render"),
            "mesh stl-2+cpu-1",
        )
        .await
        .expect("a re-render must replace it, not collide with it");

    let rows: Vec<(Option<Vec<u8>>, Option<String>, String)> = sqlx::query_as(
        "SELECT thumb_bytes, blake3, kernel_version FROM derivative \
         WHERE revision_id = $1 AND kind = 'thumbnail'",
    )
    .bind(revision.as_uuid())
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(rows.len(), 1, "one row per (revision, kind), never two");
    assert_eq!(
        rows[0].0.as_deref(),
        Some(b"second render".as_slice()),
        "the second render's bytes win"
    );
    assert_eq!(rows[0].1, None, "an inline thumbnail names no blob");
    assert_eq!(
        rows[0].2, "mesh stl-2+cpu-1",
        "kernel_version is refreshed too, or the row claims a version that did not \
         produce these bytes and can never be regenerated from it"
    );

    let page = PgParts(pool).page(library(), None, 10).await.expect("page");
    assert_eq!(
        page[0].thumbnail_webp.as_deref(),
        Some(b"second render".as_slice()),
        "and the grid reads the upserted row, not a second one beside it"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn upserting_over_the_other_storage_shape_moves_the_reference(pool: sqlx::PgPool) {
    // The kind is held constant on purpose: `derivative_storage_is_exclusive` is a
    // per-row check, and ON CONFLICT (revision_id, kind) is the only arm that can rewrite
    // a row from one storage shape to the other. Writing just `thumb_bytes` on the way in
    // would leave the old `blake3` beside it and trip the check.
    let ingest = PgIngest(pool.clone());
    let id = ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Cable clip, LP-3300-01",
            source_path: "cable-clip-lp-3300-01.stl",
            blob: &blob_row(0xc0),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");
    let revision = only_revision(&pool, id).await;
    let first = rung_blob(0xc1);
    let second = rung_blob(0xc2);

    ingest
        .upsert_derivative(
            revision,
            DerivativeKind::TessellationL1,
            DerivativeBytes::Hashed {
                blob: &first,
                grid: Some(96),
            },
            "mesh stl-1+glb-1+cpu-1",
        )
        .await
        .expect("a hash-addressed rung writes its blob row and its derivative");
    assert_eq!(
        ref_count_of(&pool, &first.hash).await,
        1,
        "a derivative pointing at these bytes holds a reference, exactly as a file row does"
    );

    ingest
        .upsert_derivative(
            revision,
            DerivativeKind::TessellationL1,
            DerivativeBytes::Hashed {
                blob: &second,
                grid: Some(32),
            },
            "mesh stl-1+glb-2+cpu-1",
        )
        .await
        .expect("a re-render at a different grid replaces the rung");

    let rungs: Vec<(Option<Vec<u8>>, Option<String>, serde_json::Value)> = sqlx::query_as(
        "SELECT thumb_bytes, blake3, params_json FROM derivative \
         WHERE revision_id = $1 AND kind = 'tessellation_l1'",
    )
    .bind(revision.as_uuid())
    .fetch_all(&pool)
    .await
    .expect("rows");
    assert_eq!(rungs.len(), 1, "still one row");
    assert_eq!(
        rungs[0].0, None,
        "a hash-addressed rung holds no inline bytes"
    );
    assert_eq!(rungs[0].1.as_deref(), Some(second.hash.to_hex().as_str()));
    assert_eq!(
        rungs[0].2,
        serde_json::json!({ "grid": 32 }),
        "params_json is refreshed, or the row cannot be regenerated into the bytes it holds"
    );
    assert_eq!(
        ref_count_of(&pool, &first.hash).await,
        0,
        "the bytes the row no longer names lose their reference, or eviction can never \
         free them"
    );
    assert_eq!(ref_count_of(&pool, &second.hash).await, 1);

    // Inline over hash-addressed: the crossing that trips the exclusivity check when only
    // one of the two storage columns is written from `excluded`.
    ingest
        .upsert_derivative(
            revision,
            DerivativeKind::TessellationL1,
            DerivativeBytes::Inline(b"inline over a rung"),
            "mesh stl-1+glb-2+cpu-1",
        )
        .await
        .expect("inline over hash-addressed must not trip derivative_storage_is_exclusive");

    let (thumb, blake3, exclusive): (Option<Vec<u8>>, Option<String>, bool) = sqlx::query_as(
        "SELECT thumb_bytes, blake3, (blake3 IS NULL) <> (thumb_bytes IS NULL) \
         FROM derivative WHERE revision_id = $1 AND kind = 'tessellation_l1'",
    )
    .bind(revision.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("one row");
    assert_eq!(thumb.as_deref(), Some(b"inline over a rung".as_slice()));
    assert_eq!(
        blake3, None,
        "the hash it used to name is cleared, not kept beside the bytes"
    );
    assert!(
        exclusive,
        "exactly one of the two storage columns is non-null"
    );
    assert_eq!(
        ref_count_of(&pool, &second.hash).await,
        0,
        "a row that stopped naming these bytes stopped referencing them"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn revisions_missing_returns_only_the_revisions_lacking_that_generated_kind(
    pool: sqlx::PgPool,
) {
    let ingest = PgIngest(pool.clone());
    // Has a thumbnail: not missing one.
    seed_part(
        &ingest,
        library(),
        "Bracket, LP-1042-03",
        0xd0,
        Some(b"webp"),
    )
    .await;
    // Has a derivative, but of another kind. This is the part that fails if the query
    // stops filtering on `kind` and asks only whether *any* derivative exists.
    let other_kind = seed_part(&ingest, library(), "Spacer, LP-2001-00", 0xd1, None).await;
    let other_kind = only_revision(&pool, other_kind).await;
    let rung = rung_blob(0xd5);
    ingest
        .upsert_derivative(
            other_kind,
            DerivativeKind::TessellationL0,
            DerivativeBytes::Hashed {
                blob: &rung,
                grid: Some(32),
            },
            "mesh stl-1+glb-1+cpu-1",
        )
        .await
        .expect("writes a rung and no thumbnail");
    // Has nothing at all.
    let bare = seed_part(&ingest, library(), "Cable clip, LP-3300-01", 0xd2, None).await;
    let bare = only_revision(&pool, bare).await;
    // Soft-deleted: hidden from the grid, so rendering for it is work nothing displays.
    let deleted = seed_part(&ingest, library(), "Vee block, LP-3072-02", 0xd3, None).await;
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(deleted.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");
    // A second library's thumbnail-less part. The sweep is per library, and this is what
    // fails if the library filter is dropped.
    let other_library = second_library(&pool).await;
    seed_part(
        &ingest,
        other_library,
        "Fixture plate, LP-4000-00",
        0xd4,
        None,
    )
    .await;

    // Explicit timestamps rather than the four `now()` values the seeds happened to get:
    // the assertion below pins the order, and revisions written inside the same
    // microsecond would fall back to a `Uuid::now_v7` tie-break that is not guaranteed
    // monotonic within one millisecond.
    sqlx::query("UPDATE revision SET created_at = now() - interval '1 hour' WHERE id = $1")
        .bind(other_kind.as_uuid())
        .execute(&pool)
        .await
        .expect("age the rung-only revision");

    let missing = PgParts(pool.clone())
        .revisions_missing(library(), DerivativeKind::Thumbnail)
        .await
        .expect("query");
    let got: Vec<uuid::Uuid> = missing.iter().map(|r| r.as_uuid()).collect();
    assert_eq!(
        got,
        vec![bare.as_uuid(), other_kind.as_uuid()],
        "exactly the live revisions of this library with no thumbnail row, newest first — \
         not the one that has a thumbnail, not the soft-deleted one, not the other \
         library's"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn revision_source_returns_the_source_files_hash_and_format(pool: sqlx::PgPool) {
    let blob = blob_row(0xe0);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Idler Bracket, LP-2210-01",
            source_path: "idler-bracket-lp-2210-01.3mf",
            blob: &blob,
            measurements: &open_mesh(),
            kernel_version: "mesh 3mf-1+cpu-1",
            format: "3mf",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");
    let revision = only_revision(&pool, id).await;
    // A newer, non-source `file` row on the same revision. `file` carries no unique
    // constraint on (revision_id, role), so this is representable — and without it the
    // `role = 'source'` filter is untested, because the only row in the table would be
    // the right answer either way. It reuses the same blake3 so the foreign key is
    // satisfied without inventing a second blob.
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, created_at) \
         VALUES (gen_random_uuid(), $1, 'export', 'glb', $2, 4096, now() + interval '1 hour')",
    )
    .bind(revision.as_uuid())
    .bind(blob.hash.to_hex())
    .execute(&pool)
    .await
    .expect("a later export row");

    let other_library = second_library(&pool).await;

    let parts = PgParts(pool);
    let source = parts
        .revision_source(library(), revision)
        .await
        .expect("query")
        .expect("the revision has a source file");
    assert_eq!(source.hash.to_hex(), blob.hash.to_hex());
    assert_eq!(
        source.format, "3mf",
        "the source file's format, not the newer export's — the derive arm reopens the \
         source with Compression::for_source_format(format)"
    );

    assert!(
        parts
            .revision_source(library(), RevisionId::new())
            .await
            .expect("query")
            .is_none(),
        "a revision that does not exist has no source, and that is not an error"
    );

    // The tenant guard, at the layer that owns it. A revision id is a uuid a caller might
    // hold from anywhere, so the scope has to be structural rather than a check the
    // caller remembers: without `p.library_id = $2` this returns another library's source
    // bytes and the derive arm renders onto that library's revision.
    assert!(
        parts
            .revision_source(other_library, revision)
            .await
            .expect("query")
            .is_none(),
        "a library that does not reach this revision must be told nothing about it — not \
         its hash, not its format, and not that it exists"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_part_has_nothing_to_download_and_a_live_one_answers_in_full(pool: sqlx::PgPool) {
    // The download route reads exactly this row and nothing else, so every column it
    // needs is asserted here rather than at the route, where a wrong one shows up as a
    // file named after the wrong part or a zstd frame handed over as an STL.
    let blob = blob_row(0xd1);
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Spindle housing, LP-4180-02",
            source_path: "spindle-housing-lp-4180-02.3mf",
            blob: &blob,
            measurements: &watertight(),
            kernel_version: "mesh 3mf-1+cpu-1",
            format: "3mf",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records");
    let revision = only_revision(&pool, id).await;
    // A newer non-source row on the same revision, as `revision_source`'s test seeds:
    // without one the `role = 'source'` filter is untested here, because the only row in
    // the table would be the right answer either way. Same blake3, so the foreign key is
    // satisfied without inventing a second blob — and a second blob would also hide a
    // wrong `role` behind a matching `zstd_level`.
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, created_at) \
         VALUES (gen_random_uuid(), $1, 'export', 'glb', $2, 4096, now() + interval '1 hour')",
    )
    .bind(revision.as_uuid())
    .bind(blob.hash.to_hex())
    .execute(&pool)
    .await
    .expect("a later export row");

    let parts = PgParts(pool.clone());
    let source = parts
        .source_for_download(revision)
        .await
        .expect("query")
        .expect("a live part's revision has a source file");
    assert_eq!(source.hash.to_hex(), blob.hash.to_hex());
    assert_eq!(
        source.format, "3mf",
        "the source file's format, not the newer export's — the route synthesizes the \
         download's extension from it"
    );
    assert_eq!(source.part_name, "Spindle housing, LP-4180-02");
    assert_eq!(
        source.zstd_level,
        Some(3),
        "the level the bytes were actually written at, read off `blob` rather than \
         re-derived from the format"
    );

    // Ruling T1-A, as corrected. `zstd_level` is nullable and NULL is real — every
    // derivative blob is written that way — so a source blob with no level means nobody
    // recorded how those bytes were stored, and that unknown must reach the route intact.
    // Not because a `COALESCE` would serve a zstd frame as the file: it would not,
    // `SourceReader::get` reads `None` and `Some(0)` identically. Because the route can
    // only refuse an unrecorded level with a message naming it (spec §2.5.1) if the
    // unknown survives the query. Every fixture in this file writes level 3, so without
    // this leg the assertion above passes just as well against the COALESCE.
    sqlx::query("UPDATE blob SET zstd_level = NULL WHERE blake3 = $1")
        .bind(blob.hash.to_hex())
        .execute(&pool)
        .await
        .expect("clear the recorded level");
    assert_eq!(
        parts
            .source_for_download(revision)
            .await
            .expect("query")
            .expect("still a live part")
            .zstd_level,
        None,
        "an unrecorded compression state must stay unrecorded, not become level 0"
    );

    // Delete is soft, and a download URL is held by whoever was last shown the grid. A
    // part the user deleted is not browsable, so a link minted before the delete must
    // stop serving bytes rather than outliving it. `library_holds` omits this same filter
    // on purpose — a re-scan must not resurrect — which is the opposite question.
    sqlx::query("UPDATE part SET deleted_at = now() WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&pool)
        .await
        .expect("soft delete");
    assert!(
        parts
            .source_for_download(revision)
            .await
            .expect("query")
            .is_none(),
        "a soft-deleted part's revision has nothing to download, and that is not an error"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn latest_revision_names_the_revision_the_grid_shows(pool: sqlx::PgPool) {
    // Two resolutions of "which revision is current" that can disagree is the bug §3.7
    // exists to prevent: a derive job enqueued against the revision the grid is not
    // showing renders a picture nobody looks at and reports success doing it. So this
    // pins both halves against one fixture — two revisions an hour apart, which is the
    // smallest shape where an ASC ordering picks the other one.
    let id = PgIngest(pool.clone())
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &blob_row(0xf0),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"older-thumbnail"),
        })
        .await
        .expect("records");
    let newer: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO revision (id, part_id, rev_label, origin, created_at, triangle_count, \
         is_watertight) SELECT gen_random_uuid(), part_id, '2', 'ingest', \
         created_at + interval '1 hour', 99999, true FROM revision WHERE part_id = $1 RETURNING id",
    )
    .bind(id.as_uuid())
    .fetch_one(&pool)
    .await
    .expect("insert a strictly newer revision");

    let latest = PgParts(pool.clone())
        .latest_revision(id)
        .await
        .expect("query")
        .expect("the part has revisions");
    assert_eq!(
        latest.as_uuid(),
        newer.0,
        "the newer revision, not the one the part was ingested with"
    );

    let page = PgParts(pool).page(library(), None, 10).await.expect("page");
    assert_eq!(page.len(), 1, "still one part");
    assert_eq!(
        page[0].summary.triangle_count,
        Some(99999),
        "and the grid resolves the same revision the resolver just named — these two \
         orderings must never be able to disagree"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn touching_a_blob_leaves_every_other_blob_alone(pool: sqlx::PgPool) {
    let read = blob_row(0x51);
    let never_read = blob_row(0x52);
    for blob in [&read, &never_read] {
        sqlx::query("INSERT INTO blob (blake3, size_bytes, stored_bytes) VALUES ($1, $2, $3)")
            .bind(blob.hash.to_hex())
            .bind(blob.size_bytes as i64)
            .bind(blob.stored_bytes as i64)
            .execute(&pool)
            .await
            .expect("inserts a blob row");
    }

    PgBlobs(pool.clone()).touch_blob(&read.hash).await;

    // Which rows, not how many: an UPDATE that lost its WHERE clause would mark the whole
    // table recently used, and every age-based decision downstream reads this column to
    // tell blobs apart. A touch that cannot discriminate is worse than no touch at all.
    let touched: Vec<String> =
        sqlx::query_scalar("SELECT blake3 FROM blob WHERE last_accessed_at IS NOT NULL")
            .fetch_all(&pool)
            .await
            .expect("queries the touched rows");
    assert_eq!(
        touched,
        vec![read.hash.to_hex()],
        "exactly the blob that was read carries a timestamp"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_hash_addressed_thumbnail_is_refused_rather_than_written(pool: sqlx::PgPool) {
    // The row this refuses is valid, invisible and unhealable: `page` reads a thumbnail
    // only out of `thumb_bytes`, so the grid says "no preview yet" for a part that has
    // one — and `revisions_missing` then excludes that revision, because a row exists, so
    // the sweep never fills it in either. There is no reader for a hash-addressed
    // thumbnail until the viewer lands, so the honest answer is to refuse the shape.
    let ingest = PgIngest(pool.clone());
    let id = seed_part(&ingest, library(), "Bracket, LP-1042-03", 0xa1, None).await;
    let revision = only_revision(&pool, id).await;
    let rung = rung_blob(0xa2);

    let err = ingest
        .upsert_derivative(
            revision,
            DerivativeKind::Thumbnail,
            DerivativeBytes::Hashed {
                blob: &rung,
                grid: None,
            },
            "mesh stl-1+glb-1+cpu-1",
        )
        .await
        .expect_err("nothing can read a hash-addressed thumbnail back");
    match err {
        DbError::ThumbnailNotInline { revision: named } => assert_eq!(named, revision),
        other => panic!("expected ThumbnailNotInline, got {other:?}"),
    }

    // Refused before the transaction opens, so it leaves nothing behind — not the
    // derivative row, and not the `blob` row the Hashed arm would otherwise insert first.
    let rows: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM derivative WHERE revision_id = $1), \
                (SELECT count(*) FROM blob WHERE blake3 = $2)",
    )
    .bind(revision.as_uuid())
    .bind(rung.hash.to_hex())
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!(
        rows,
        (0, 0),
        "a refused write must write nothing at all, or it leaves a blob row nothing points at"
    );

    // And the revision is still one the sweep will offer to fill, which is what a row
    // would have taken away.
    let missing = PgParts(pool)
        .revisions_missing(library(), DerivativeKind::Thumbnail)
        .await
        .expect("query");
    assert_eq!(
        missing.iter().map(|r| r.as_uuid()).collect::<Vec<_>>(),
        vec![revision.as_uuid()],
        "the revision must stay in the sweep's list, or nothing ever heals it"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn an_empty_inline_derivative_is_refused_rather_than_written(pool: sqlx::PgPool) {
    // `DerivativeBytes` closed "both columns" and "neither"; it did not close *empty*. An
    // empty bytea is not NULL, so it satisfies `derivative_storage_is_exclusive`, reads
    // back as `Some(vec![])` and reaches the grid as `data:image/webp;base64,` — the
    // broken image `insert_part_chain` already refuses to write.
    let ingest = PgIngest(pool.clone());
    let id = seed_part(&ingest, library(), "Spacer, LP-2001-00", 0xa3, None).await;
    let revision = only_revision(&pool, id).await;

    let err = ingest
        .upsert_derivative(
            revision,
            DerivativeKind::Thumbnail,
            DerivativeBytes::Inline(b""),
            "mesh stl-1+cpu-1",
        )
        .await
        .expect_err("zero bytes are not a thumbnail");
    match err {
        DbError::EmptyDerivative {
            kind,
            revision: named,
        } => {
            assert_eq!(kind, "thumbnail");
            assert_eq!(named, revision);
        }
        other => panic!("expected EmptyDerivative, got {other:?}"),
    }

    let page = PgParts(pool).page(library(), None, 10).await.expect("page");
    assert_eq!(
        page[0].thumbnail_webp, None,
        "the grid must still read \"no preview yet\", not zero bytes it will render as a \
         broken image"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_source_hash_that_is_not_a_digest_is_reported_with_what_to_do(pool: sqlx::PgPool) {
    // `file.blake3` is plain `text` with a foreign key and no format check, so a row
    // holding something that is not a digest is representable — and `revision_source` is
    // where it surfaces. Nothing outside `src/` pinned this message before, so its
    // wording was free to drift back into the speculation CLAUDE.md forbids.
    const NOT_A_DIGEST: &str = "0xdeadbeef-written-by-hand";
    let ingest = PgIngest(pool.clone());
    let id = seed_part(&ingest, library(), "Cable clip, LP-3300-01", 0xa4, None).await;
    let revision = only_revision(&pool, id).await;
    sqlx::query("INSERT INTO blob (blake3, size_bytes, stored_bytes) VALUES ($1, 204800, 91204)")
        .bind(NOT_A_DIGEST)
        .execute(&pool)
        .await
        .expect("a blob row the corrupt file row can reference");
    sqlx::query("UPDATE file SET blake3 = $1 WHERE revision_id = $2 AND role = 'source'")
        .bind(NOT_A_DIGEST)
        .bind(revision.as_uuid())
        .execute(&pool)
        .await
        .expect("corrupt the column directly");

    let parts = PgParts(pool);
    let err = parts
        .revision_source(library(), revision)
        .await
        .expect_err("a hash that is not a hash must be reported, not parsed into one");
    // The download route reads the same column through its own query, so it reports the
    // same thing rather than parsing the garbage into a hash and 404ing on the blob store.
    assert!(
        matches!(
            parts.source_for_download(revision).await,
            Err(DbError::CorruptBlobHash {
                column: "file.blake3",
                ..
            })
        ),
        "source_for_download reads the same column and must refuse it the same way"
    );
    match &err {
        DbError::CorruptBlobHash { column, value } => {
            assert_eq!(*column, "file.blake3");
            assert_eq!(value, NOT_A_DIGEST);
        }
        other => panic!("expected CorruptBlobHash, got {other:?}"),
    }
    let message = err.to_string();
    assert!(
        message.contains("Check what else has write access to this database")
            && message.contains("re-scan the part"),
        "the message must say what to do about it (CLAUDE.md), got: {message}"
    );
    assert!(
        !message.contains("probably written by something other than lapidary-db"),
        "speculating about who wrote the row is not an instruction, got: {message}"
    );
    assert_eq!(
        err.client_message(),
        message,
        "this variant's text is crafted, safe and actionable, so the client sees it \
         verbatim rather than being sent to the server logs"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn two_parts_holding_the_same_bytes_are_two_files_in_the_total(pool: sqlx::PgPool) {
    // The regression this pins is an under-report, which is the direction that looks fine:
    // the panel reads a plausible number and the volume fills up anyway. Source bytes stop
    // being deduplicated the moment ingest writes one file per model (spec §0), so a total
    // summed over `blob` rows reports one copy of bytes that are on disk twice.
    //
    // Shaped exactly as ingest writes them today, unlike the fixture below it: each part
    // carries its own `storage_path`, and the blob is uncompressed, so `size_bytes` is
    // what the file occupies.
    let shared = StoredBlobRow {
        hash: BlobHash::from_bytes([0xe4; 32]),
        size_bytes: 204_800,
        stored_bytes: 204_800,
        zstd_level: 0,
    };
    let ingest = PgIngest(pool.clone());
    ingest
        .record(IngestRequest {
            folder: None,
            storage_path: Some("libraries/default/Terrain/cliff/cliff.stl"),
            library: library(),
            name: "cliff",
            source_path: "Terrain/cliff.stl",
            blob: &shared,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("records the first part");
    ingest
        .link_existing(IngestRequest {
            folder: None,
            storage_path: Some("libraries/default/Bases/cliff/cliff.stl"),
            library: library(),
            name: "cliff",
            source_path: "Bases/cliff.stl",
            blob: &shared,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: None,
        })
        .await
        .expect("the same bytes under a second part, in its own directory");

    let totals = PgParts(pool.clone())
        .storage_totals(library())
        .await
        .expect("totals")
        .expect("the seeded library exists");
    assert_eq!(
        totals.source_bytes,
        204_800 * 2,
        "two files on disk, two files in the total — the blob-shaped sum this replaced \
         reported 204,800 for a library holding 409,600, and the gap widens with every \
         duplicate a corpus carries"
    );
    assert_eq!(
        totals.derivative_bytes, 0,
        "no rungs and no previews in this fixture: the source half is what is under test"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_library_total_shares_derivative_bytes_and_counts_inline_previews_at_all(
    pool: sqlx::PgPool,
) {
    // Three things this fixture is built to catch, and no single-part library shows any
    // of them: derivative bytes two parts share counted twice, inline previews left out
    // of the derivative total entirely, and another library's bytes swept into this
    // one's.
    let shared = StoredBlobRow {
        hash: BlobHash::from_bytes([0xd1; 32]),
        size_bytes: 204_800,
        stored_bytes: 91_204,
        zstd_level: 3,
    };
    let ingest = PgIngest(pool.clone());
    let rungs = [rung("tessellation_l0", 0xd5, Some(32))];
    ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "bracket-lp-1042-03.stl",
            blob: &shared,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+glb-1+cpu-1",
            format: "stl",
            tessellations: &rungs,
            thumbnail_webp: Some(b"webp-bracket"),
        })
        .await
        .expect("records the first part");
    // The same bytes under a second part — a duplicate STL scanned from another folder,
    // which is the ordinary case `link_existing` exists for. Two files on disk since the
    // store became a folder tree: one blob ROW, two model directories.
    ingest
        .link_existing(IngestRequest {
            folder: None,
            storage_path: None,
            library: library(),
            name: "Bracket, LP-1042-03 (spare)",
            source_path: "bracket-lp-1042-03-spare.stl",
            blob: &shared,
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
            thumbnail_webp: Some(b"webp-spare"),
        })
        .await
        .expect("links the shared blob to a second part");
    // Another tenant, whose bytes must not appear in this library's total.
    let other = second_library(&pool).await;
    ingest
        .record(IngestRequest {
            folder: None,
            storage_path: None,
            library: other,
            name: "Impeller, LP-5501-02",
            source_path: "impeller-lp-5501-02.3mf",
            blob: &blob_row(0xd9),
            measurements: &watertight(),
            kernel_version: "mesh stl-1+cpu-1",
            format: "3mf",
            tessellations: &[],
            thumbnail_webp: Some(b"webp-impeller"),
        })
        .await
        .expect("records another library's part");

    let totals = PgParts(pool.clone())
        .storage_totals(library())
        .await
        .expect("totals")
        .expect("the seeded library exists");
    assert_eq!(
        totals.source_bytes,
        204_800 * 2,
        "one blob row, two parts, two files: source bytes are path-addressed now, so the \
         old blob-shaped sum would report one part's worth for a library holding two"
    );
    let inline = i64::try_from("webp-bracket".len() + "webp-spare".len()).expect("fits");
    assert_eq!(
        totals.derivative_bytes,
        40_960 + u64::try_from(inline).expect("fits"),
        "the rung on disk plus both inline previews: thumbnails live in Postgres by \
         DATA.md §1.5's deliberate exception, which is where they are stored and not an \
         exemption from being counted"
    );
    assert!(
        totals.derivative_bytes > 0,
        "a derivative total that omitted the inline half would read 0 for a library \
         whose every part has a preview"
    );

    // And the other tenant's own total, read back the same way, is the proof the filter
    // is a filter rather than a coincidence of ordering.
    let theirs = PgParts(pool.clone())
        .storage_totals(other)
        .await
        .expect("totals")
        .expect("the second library exists");
    // `size_bytes`, not `stored_bytes`: their one part is one file on disk, at the size
    // the file occupies.
    assert_eq!(theirs.source_bytes, blob_row(0xd9).size_bytes);
    assert_eq!(
        theirs.derivative_bytes,
        u64::try_from("webp-impeller".len()).expect("fits")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_file_row_of_another_role_is_not_part_of_the_source_total(pool: sqlx::PgPool) {
    let ingest = PgIngest(pool.clone());
    let id = seed_part(
        &ingest,
        library(),
        "Flange, DN40 PN16, LP-3310-02",
        0xf1,
        Some(b"webp-flange"),
    )
    .await;
    let revision = only_revision(&pool, id).await;

    let before = PgParts(pool.clone())
        .storage_totals(library())
        .await
        .expect("totals")
        .expect("the seeded library exists");

    // A converted export beside the source it came from: the shape `variant=3mf` will
    // write when format negotiation lands, and the first row this library holds whose
    // role is not `source`. Size deliberately unlike the source's, so a total that
    // counted it has to report a visibly different number.
    let export = BlobHash::from_bytes([0xf2; 32]);
    sqlx::query(
        "INSERT INTO blob (blake3, size_bytes, stored_bytes, zstd_level, ref_count) \
         VALUES ($1, 8642, 4321, 3, 1)",
    )
    .bind(export.to_hex())
    .execute(&pool)
    .await
    .expect("a blob for the export");
    sqlx::query(
        "INSERT INTO file (id, revision_id, role, format, blake3, size_bytes, created_at) \
         VALUES (gen_random_uuid(), $1, 'export', '3mf', $2, 8642, now())",
    )
    .bind(revision.as_uuid())
    .bind(export.to_hex())
    .execute(&pool)
    .await
    .expect("an export row on the same revision");

    let after = PgParts(pool.clone())
        .storage_totals(library())
        .await
        .expect("totals")
        .expect("the seeded library exists");
    assert_eq!(
        after.source_bytes, before.source_bytes,
        "the source total counts source rows: `page`'s own LATERAL filters `role` and \
         these two queries have to mean the same set of rows, or a card's figures and \
         the library total stop describing the same library"
    );
    assert_eq!(
        after.source_bytes,
        blob_row(0xf1).size_bytes,
        "and it is still the source file's bytes, not zero and not the sum of both"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn an_empty_library_costs_nothing_and_an_absent_one_has_no_answer(pool: sqlx::PgPool) {
    let parts = PgParts(pool.clone());
    let empty = parts
        .storage_totals(library())
        .await
        .expect("totals")
        .expect("the seeded library exists even with nothing in it");
    assert_eq!((empty.source_bytes, empty.derivative_bytes), (0, 0));

    // `None`, not zeroes. A library that does not exist and one holding nothing are
    // different facts, and only the caller knows what to do about the first — the same
    // distinction `auto_thumbnail` draws, and the reason the route can answer 404.
    assert!(
        parts
            .storage_totals(LibraryId::new())
            .await
            .expect("totals")
            .is_none()
    );
}
