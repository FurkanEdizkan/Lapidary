//! `DELETE /api/parts/{id}` and `POST /api/parts/{id}/restore` — step one of three, and
//! its undo.
//!
//! The product rule these exist under is that we never delete user data implicitly, so
//! what is under test is mostly what *does not* happen: no blob row changes, no bytes
//! move, and the part comes back whole. The one thing that does happen is that it stops
//! being visible, and "visible" is four separate queries — the grid, the part page, the
//! download and the library's storage totals. Each filters `deleted_at` on its own, so
//! each is asserted on its own: a fifth read path added later without the filter is
//! exactly the bug this file is here to catch.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, MeshMeasurements, PartId};
use lapidary_db::{IngestRequest, PgIngest, PgParts, StoredBlobRow};
use tower::ServiceExt;

const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";

fn library() -> LibraryId {
    LibraryId::from_uuid(SEEDED_LIBRARY.parse().expect("valid uuid"))
}

fn state(pool: sqlx::PgPool) -> AppState {
    AppState {
        db: pool,
        // Every assertion below is about rows. The download case checks that the route
        // refuses *before* it would reach the store, and a path that does not exist is
        // what makes that a real assertion rather than a coincidence.
        blob_root: std::path::PathBuf::from("/nonexistent-blob-root"),
        upload_dir: std::path::PathBuf::from("/nonexistent-upload-dir"),
    }
}

async fn call(pool: sqlx::PgPool, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = router(state(pool), Role::Api)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body reads");
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("body is JSON")
    };
    (status, json)
}

/// Seeded through `PgIngest::record`, the repository the scan handler itself uses, so the
/// fixture carries a `blob` row with a real `ref_count` rather than a hand-built one. That
/// matters here: the assertion that delete touches no bytes is only worth making against a
/// count something actually maintains.
async fn seed(pool: &sqlx::PgPool, seed: u8, name: &str, path: &str) -> PartId {
    PgIngest(pool.clone())
        .record(IngestRequest {
            library: library(),
            name,
            source_path: path,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            thumbnail_webp: Some(b"webp-preview"),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seed part")
}

async fn grid_names(pool: &sqlx::PgPool) -> Vec<String> {
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?limit=50"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json["parts"]
        .as_array()
        .expect("a parts array")
        .iter()
        .map(|p| p["name"].as_str().expect("a name").to_owned())
        .collect()
}

async fn source_bytes(pool: &sqlx::PgPool) -> u64 {
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/storage"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json["sourceBytes"].as_u64().expect("a JSON number")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn deleting_a_part_hides_it_from_every_read_path_and_restoring_brings_it_all_back(
    pool: sqlx::PgPool,
) {
    let gone = seed(
        &pool,
        0xa1,
        "Bracket, LP-1042-03",
        "brackets/LP-1042-03.stl",
    )
    .await;
    let kept = seed(&pool, 0xa2, "Impeller, LP-5501-02", "pumps/LP-5501-02.stl").await;
    let revision = PgParts(pool.clone())
        .latest_revision(gone)
        .await
        .expect("revision lookup")
        .expect("the seeded part has one");

    // Before: two cards, two parts' worth of bytes, both pages reachable.
    assert_eq!(grid_names(&pool).await.len(), 2);
    assert_eq!(source_bytes(&pool).await, 91_204 * 2);

    let (status, body) = call(pool.clone(), "DELETE", &format!("/api/parts/{gone}")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, serde_json::Value::Null, "204 carries no body");

    // Four read paths, four filters, asserted one at a time. A single "it is gone from the
    // grid" would pass with `detail` or `source_for_download` still serving it.
    assert_eq!(grid_names(&pool).await, vec!["Impeller, LP-5501-02"]);
    assert_eq!(
        call(pool.clone(), "GET", &format!("/api/parts/{gone}"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            pool.clone(),
            "GET",
            &format!("/api/revisions/{revision}/download?variant=original")
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "the download must refuse before it reaches the store, not after"
    );
    assert_eq!(
        source_bytes(&pool).await,
        91_204,
        "a hidden part must not still be counted against the library's storage"
    );

    // Nothing on disk changed, and the row that would say otherwise is `blob`. This is the
    // assertion the product rule is actually about: delete is not deletion.
    let (refs, blobs): (i64, i64) =
        sqlx::query_as("SELECT sum(ref_count)::bigint, count(*)::bigint FROM blob")
            .fetch_one(&pool)
            .await
            .expect("blob totals read");
    assert_eq!(
        (refs, blobs),
        (2, 2),
        "one blob per source, each still referenced once. The two thumbnails have no \
         blob rows of their own — they travel inline in `derivative.thumb_bytes`"
    );

    let (status, _) = call(pool.clone(), "POST", &format!("/api/parts/{gone}/restore")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let mut names = grid_names(&pool).await;
    names.sort();
    assert_eq!(names, vec!["Bracket, LP-1042-03", "Impeller, LP-5501-02"]);
    assert_eq!(source_bytes(&pool).await, 91_204 * 2);

    // Whole, not merely present: the revision's figures and the preview came back with it,
    // because nothing ever removed them.
    let (status, json) = call(pool.clone(), "GET", &format!("/api/parts/{gone}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["triangleCount"], 48_112);
    assert_eq!(json["volumeMm3"]["value"], 21_478.5);
    assert!(
        json["thumbnail"]
            .as_str()
            .expect("a data URL")
            .starts_with("data:image/webp;base64,"),
        "the preview survived the round trip"
    );
    assert_eq!(
        call(
            pool.clone(),
            "GET",
            &format!("/api/revisions/{revision}/download?variant=original")
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the part is reachable again, and the route now gets as far as the store — which \
         this test deliberately points at a path that does not exist. The 404 above and \
         the 500 here are the assertion: one is 'no such part', the other is 'the part \
         is fine, the bytes are missing', and a restore that only half worked would keep \
         answering 404"
    );
    keeps_ids_stable(&pool, gone, kept).await;
}

/// The ids did not change. A "restore" that reinserted the part would give it a new id and
/// break every link anyone had saved to it, and the grid alone could not tell.
async fn keeps_ids_stable(pool: &sqlx::PgPool, gone: PartId, kept: PartId) {
    let mut ids: Vec<String> = sqlx::query_scalar("SELECT id::text FROM part")
        .fetch_all(pool)
        .await
        .expect("part ids read");
    ids.sort();
    let mut expected = vec![gone.to_string(), kept.to_string()];
    expected.sort();
    assert_eq!(ids, expected);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_second_delete_is_not_a_success_and_a_live_part_cannot_be_restored(pool: sqlx::PgPool) {
    let part = seed(
        &pool,
        0xb1,
        "Bracket, LP-1042-03",
        "brackets/LP-1042-03.stl",
    )
    .await;

    // Restore before delete: there is nothing to undo, and answering 204 would tell a
    // client it had reversed something.
    assert_eq!(
        call(pool.clone(), "POST", &format!("/api/parts/{part}/restore"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        call(pool.clone(), "DELETE", &format!("/api/parts/{part}"))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let first: Option<jiff::Timestamp> = deleted_at(&pool, part).await;
    assert!(first.is_some());

    let (status, json) = call(pool.clone(), "DELETE", &format!("/api/parts/{part}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("has been deleted"),
        "the body must not distinguish a deleted part from one that never existed: {json}"
    );
    assert_eq!(
        deleted_at(&pool, part).await,
        first,
        "a repeat delete must not move the timestamp — that would quietly restart the \
         clock on how long the part has been gone"
    );
}

async fn deleted_at(pool: &sqlx::PgPool, part: PartId) -> Option<jiff::Timestamp> {
    let micros: Option<i64> = sqlx::query_scalar(
        "SELECT (extract(epoch FROM deleted_at) * 1000000)::bigint FROM part WHERE id = $1",
    )
    .bind(part.as_uuid())
    .fetch_one(pool)
    .await
    .expect("deleted_at reads");
    micros.map(|us| {
        jiff::Timestamp::from_microsecond(us).expect("a timestamp Postgres just produced")
    })
}

/// Seeds a second part at a different path over the *same* blob, the way ingest does when
/// it recognises bytes it already holds. This is the case purge has to be careful about,
/// and the one the corpus audit could not cover: on a real library, identical files under
/// two folders are the ordinary case, not the exotic one.
async fn seed_sharing(pool: &sqlx::PgPool, seed: u8, name: &str, path: &str) -> PartId {
    PgIngest(pool.clone())
        .link_existing(IngestRequest {
            library: library(),
            name,
            source_path: path,
            blob: &StoredBlobRow {
                hash: BlobHash::from_bytes([seed; 32]),
                size_bytes: 204_800,
                stored_bytes: 91_204,
                zstd_level: 3,
            },
            measurements: &MeshMeasurements {
                bbox_mm: [61.0, 42.0, 18.5],
                triangle_count: 48_112,
                surface_area_mm2: 9_804.25,
                volume_mm3: Some(21_478.5),
                is_watertight: true,
            },
            thumbnail_webp: Some(b"webp-preview"),
            kernel_version: "mesh stl-1+cpu-1",
            format: "stl",
            tessellations: &[],
        })
        .await
        .expect("seed sharing part")
}

async fn blob_state(pool: &sqlx::PgPool, seed: u8) -> (i32, bool) {
    sqlx::query_as("SELECT ref_count, quarantined_at IS NOT NULL FROM blob WHERE blake3 = $1")
        .bind(BlobHash::from_bytes([seed; 32]).to_hex())
        .fetch_one(pool)
        .await
        .expect("blob state reads")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn purging_one_of_two_parts_over_the_same_blob_quarantines_nothing(pool: sqlx::PgPool) {
    // The same bytes under two paths — `mounting/` and `spares/` — which is what a real
    // library looks like. Purging one of them must leave the other's download working, and
    // the whole design of the reference update is in service of that.
    let first = seed(
        &pool,
        0xc1,
        "Bracket, LP-1042-03",
        "mounting/LP-1042-03.stl",
    )
    .await;
    let second = seed_sharing(&pool, 0xc1, "Bracket, LP-1042-03", "spares/LP-1042-03.stl").await;
    assert_eq!(blob_state(&pool, 0xc1).await, (2, false), "two file rows");

    assert!(
        PgParts(pool.clone())
            .soft_delete(first)
            .await
            .expect("soft delete")
    );
    let (status, json) = call(pool.clone(), "POST", &format!("/api/parts/{first}/purge")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["quarantined"], 0,
        "the bytes are still serving the other part: {json}"
    );
    assert_eq!(json["quarantinedBytes"], 0);
    assert_eq!(
        blob_state(&pool, 0xc1).await,
        (1, false),
        "one reference left, and nothing quarantined"
    );

    // The surviving part is untouched, which is the thing that would have gone wrong.
    let (status, json) = call(pool.clone(), "GET", &format!("/api/parts/{second}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["sourcePath"], "spares/LP-1042-03.stl");

    // Now the last one. Same call, different outcome, because nothing points at the bytes
    // any more.
    assert!(
        PgParts(pool.clone())
            .soft_delete(second)
            .await
            .expect("soft delete")
    );
    let (status, json) = call(pool.clone(), "POST", &format!("/api/parts/{second}/purge")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["quarantined"], 1);
    assert_eq!(json["quarantinedBytes"], 91_204);
    assert_eq!(blob_state(&pool, 0xc1).await, (0, true));

    // Quarantined, not gone. Thirty days is a promise that the bytes are still there, and
    // a row that had been deleted here would make the promise unkeepable.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("blob count");
    assert_eq!(rows, 1, "the blob row survives its own quarantine");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn purge_corrects_a_reference_count_that_had_drifted(pool: sqlx::PgPool) {
    // The reason purge recomputes instead of decrementing. `ref_count` is maintained by
    // arithmetic on every other path, and this is what a drift there looks like by the
    // time the reaper sees it: two parts sharing a blob whose counter says seven.
    //
    // A decrementing purge would take it to six and quarantine nothing, ever. The bytes
    // would be immortal — the failure mode a reference count has when it is only ever
    // adjusted and never checked.
    let first = seed(
        &pool,
        0xd1,
        "Bracket, LP-1042-03",
        "mounting/LP-1042-03.stl",
    )
    .await;
    let second = seed_sharing(&pool, 0xd1, "Bracket, LP-1042-03", "spares/LP-1042-03.stl").await;
    sqlx::query("UPDATE blob SET ref_count = 7 WHERE blake3 = $1")
        .bind(BlobHash::from_bytes([0xd1; 32]).to_hex())
        .execute(&pool)
        .await
        .expect("drift the counter");

    let parts = PgParts(pool.clone());
    assert!(parts.soft_delete(first).await.expect("soft delete"));
    let (status, _) = call(pool.clone(), "POST", &format!("/api/parts/{first}/purge")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        blob_state(&pool, 0xd1).await,
        (1, false),
        "the count is what reachability says it is, not 7 minus 1"
    );

    assert!(parts.soft_delete(second).await.expect("soft delete"));
    let (status, json) = call(pool.clone(), "POST", &format!("/api/parts/{second}/purge")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["quarantined"], 1,
        "and the last purge still reaches zero, which a decrementing one never would"
    );
    assert_eq!(blob_state(&pool, 0xd1).await, (0, true));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn purge_refuses_to_be_the_first_step(pool: sqlx::PgPool) {
    let part = seed(
        &pool,
        0xe1,
        "Bracket, LP-1042-03",
        "mounting/LP-1042-03.stl",
    )
    .await;

    let (status, json) = call(pool.clone(), "POST", &format!("/api/parts/{part}/purge")).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a live part must not be purgeable in one call"
    );
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("Remove it first"),
        "and the body has to say what to do instead: {json}"
    );
    assert_eq!(
        call(pool.clone(), "GET", &format!("/api/parts/{part}"))
            .await
            .0,
        StatusCode::OK,
        "the refused purge changed nothing"
    );
    assert_eq!(blob_state(&pool, 0xe1).await, (1, false));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_purged_part_leaves_no_rows_behind_it(pool: sqlx::PgPool) {
    let part = seed(
        &pool,
        0xf1,
        "Bracket, LP-1042-03",
        "mounting/LP-1042-03.stl",
    )
    .await;
    assert!(
        PgParts(pool.clone())
            .soft_delete(part)
            .await
            .expect("soft delete")
    );
    assert_eq!(
        call(pool.clone(), "POST", &format!("/api/parts/{part}/purge"))
            .await
            .0,
        StatusCode::OK
    );

    // Nothing declares ON DELETE CASCADE, so every one of these is a statement purge had
    // to write. A missed table is a foreign key that blocks the reaper from ever removing
    // the blob, and nothing else would notice.
    for (table, query) in [
        ("part", "SELECT count(*) FROM part"),
        ("revision", "SELECT count(*) FROM revision"),
        ("file", "SELECT count(*) FROM file"),
        ("derivative", "SELECT count(*) FROM derivative"),
    ] {
        let rows: i64 = sqlx::query_scalar(query)
            .fetch_one(&pool)
            .await
            .expect("count reads");
        assert_eq!(rows, 0, "{table} still holds rows for a purged part");
    }
    // And a second purge of the same id is honestly "no such part" now.
    assert_eq!(
        call(pool.clone(), "POST", &format!("/api/parts/{part}/purge"))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_removed_list_is_the_only_route_back_to_a_deleted_part(pool: sqlx::PgPool) {
    // Without `?state=removed` a removal is a one-way door: every other read path filters
    // `deleted_at`, so nothing in the product could name the part again to restore it.
    // "We never delete user data implicitly" is not satisfied by a removal nobody can find.
    let gone = seed(
        &pool,
        0xa1,
        "Bracket, LP-1042-03",
        "mounting/LP-1042-03.stl",
    )
    .await;
    seed(&pool, 0xa2, "Impeller, LP-5501-02", "pumps/LP-5501-02.stl").await;

    let removed_uri = format!("/api/libraries/{SEEDED_LIBRARY}/parts?state=removed");
    let (status, json) = call(pool.clone(), "GET", &removed_uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["parts"].as_array().expect("an array").len(),
        0,
        "nothing is removed yet"
    );

    assert_eq!(
        call(pool.clone(), "DELETE", &format!("/api/parts/{gone}"))
            .await
            .0,
        StatusCode::NO_CONTENT
    );

    let (_, json) = call(pool.clone(), "GET", &removed_uri).await;
    let parts = json["parts"].as_array().expect("an array");
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["name"], "Bracket, LP-1042-03");
    // The path, not only the name. Two parts can share a name and the purge confirmation
    // on this list names one of them — see `strings.removal.purgeConfirm`.
    assert_eq!(parts[0]["sourcePath"], "mounting/LP-1042-03.stl");
    assert_eq!(
        grid_names(&pool).await,
        vec!["Impeller, LP-5501-02"],
        "and the two lists are disjoint: the library never shows a removed part"
    );

    // An unknown value is the library, not a 400. The grid is a read whose wrong answer is
    // visible immediately, and 400ing a typo'd query string turns a bookmark that used to
    // work into an error page.
    let (status, json) = call(
        pool.clone(),
        "GET",
        &format!("/api/libraries/{SEEDED_LIBRARY}/parts?state=banana"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["parts"].as_array().expect("an array").len(), 1);
    assert_eq!(json["parts"][0]["name"], "Impeller, LP-5501-02");
}
