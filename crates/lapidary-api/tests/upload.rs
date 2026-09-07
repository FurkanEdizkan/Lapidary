//! The three upload routes, driven through this crate's router against a live Postgres.
//!
//! What each of them promises is checked where the promise actually lives: the probe
//! against the `blob` and `part` rows it sorts by, the chunk against the staged file's
//! length, and the commit against the blob store and the `job` table together — the
//! commit's whole point is that bytes reach the store *and* a row is written to point at
//! them, and a test that read only one of the two would pass on half a commit.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use lapidary_api::{AppState, Role, router};
use lapidary_core::{BlobHash, LibraryId, ScanAccepted};
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

/// Two temp roots, because the whole design turns on them being different: a
/// half-transferred file must never sit inside a store whose contract is that everything
/// in it is complete.
struct Server {
    _blobs: tempfile::TempDir,
    uploads: tempfile::TempDir,
    state: AppState,
}

fn server(pool: sqlx::PgPool) -> Server {
    let blobs = tempfile::tempdir().expect("blob root");
    let uploads = tempfile::tempdir().expect("upload dir");
    let state = AppState {
        db: pool,
        blob_root: blobs.path().to_path_buf(),
        upload_dir: uploads.path().to_path_buf(),
        host_storage_root: None,
    };
    Server {
        _blobs: blobs,
        uploads,
        state,
    }
}

impl Server {
    async fn send(&self, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = router(self.state.clone(), Role::Api)
            .oneshot(request)
            .await
            .expect("router responds");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("body reads");
        // Not `expect("body is JSON")`. axum's own rejections -- a body over the limit
        // is the one this file cares about -- answer a line of plain text, and a parse
        // panic there reports a serde error instead of the status that explains it.
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(
                |_| serde_json::json!({ "message": String::from_utf8_lossy(&bytes) }),
            )
        };
        (status, json)
    }

    async fn post(&self, uri: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        self.send(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request builds"),
        )
        .await
    }

    async fn probe(&self, files: serde_json::Value) -> (StatusCode, serde_json::Value) {
        self.post(
            &format!("/api/libraries/{SEEDED_LIBRARY}/uploads/probe"),
            serde_json::json!({ "files": files }),
        )
        .await
    }

    async fn commit(&self, files: serde_json::Value) -> (StatusCode, serde_json::Value) {
        self.post(
            &format!("/api/libraries/{SEEDED_LIBRARY}/uploads/commit"),
            serde_json::json!({ "files": files }),
        )
        .await
    }

    async fn chunk(
        &self,
        hash: &BlobHash,
        offset: u64,
        bytes: &[u8],
    ) -> (StatusCode, serde_json::Value) {
        self.send(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/libraries/{SEEDED_LIBRARY}/uploads/{}?offset={offset}",
                    hash.to_hex()
                ))
                .body(Body::from(bytes.to_vec()))
                .expect("request builds"),
        )
        .await
    }

    /// The whole transfer in two chunks, so every test that needs staged bytes also
    /// exercises the resume path rather than a single append.
    async fn upload(&self, bytes: &[u8]) -> BlobHash {
        let hash = BlobHash::from_bytes(*blake3::hash(bytes).as_bytes());
        let split = bytes.len() / 2;
        let (first, second) = bytes.split_at(split);
        let (status, _) = self.chunk(&hash, 0, first).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = self.chunk(&hash, split as u64, second).await;
        assert_eq!(status, StatusCode::OK);
        hash
    }

    fn staged(&self, hash: &BlobHash) -> std::path::PathBuf {
        self.uploads
            .path()
            .join(SEEDED_LIBRARY)
            .join(format!("{}.part", hash.to_hex()))
    }
}

fn library() -> LibraryId {
    SEEDED_LIBRARY.parse().expect("seeded library id parses")
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_probe_of_an_empty_library_needs_every_file_and_its_bytes(pool: sqlx::PgPool) {
    let server = server(pool);
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET).as_bytes()).to_hex();
    let (status, json) = server
        .probe(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": hash }
        ]))
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["have"], serde_json::json!([]));
    assert_eq!(json["needRows"], serde_json::json!([]));
    assert_eq!(
        json["needBytes"],
        serde_json::json!(["brackets/LP-1042-03.stl"]),
        "a library that holds nothing needs both the rows and the bytes"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_probe_answers_need_rows_for_bytes_the_store_already_holds(pool: sqlx::PgPool) {
    // The larger of the two dedup wins, and the one `DATA.md`'s two-list contract misses:
    // these bytes are in the store under some other library's part, so the transfer is
    // skipped entirely and only the rows are missing.
    let server = server(pool.clone());
    let hash = server.upload(BRACKET).await;
    let (status, _) = server
        .commit(serde_json::json!([{ "path": "a/LP-1042-03.stl", "blake3": hash.to_hex() }]))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, json) = server
        .probe(serde_json::json!([
            { "path": "b/LP-1042-03.stl", "blake3": hash.to_hex() }
        ]))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["needRows"],
        serde_json::json!(["b/LP-1042-03.stl"]),
        "the bytes are held, so only the rows are missing"
    );
    assert_eq!(json["needBytes"], serde_json::json!([]));
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_chunk_at_the_wrong_offset_is_refused_and_says_where_to_resume(pool: sqlx::PgPool) {
    let server = server(pool);
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET).as_bytes());

    let (status, json) = server.chunk(&hash, 0, &BRACKET[..100]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["received"], 100);

    // A client that lost track — retrying a chunk that in fact landed. Appending it
    // again would corrupt the file into something that can never verify, so the length
    // the server actually holds is the answer, in the same field a successful chunk uses.
    let (status, json) = server.chunk(&hash, 0, &BRACKET[..100]).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        json["received"], 100,
        "a resuming client reads one field whether the chunk landed or not"
    );

    // And a gap is refused for the same reason from the other direction.
    let (status, json) = server.chunk(&hash, 500, &BRACKET[500..600]).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(json["received"], 100);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn committing_stores_the_bytes_records_the_blob_and_queues_the_mesh(pool: sqlx::PgPool) {
    let server = server(pool.clone());
    let hash = server.upload(BRACKET).await;
    assert!(server.staged(&hash).exists(), "the transfer staged a file");

    let (status, json) = server
        .commit(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": hash.to_hex() }
        ]))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let accepted: ScanAccepted = serde_json::from_value(json).expect("body is a ScanAccepted");
    assert_eq!(accepted.queued, 1);

    // 1. The bytes are in the blob store, byte-identical.
    let stored = lapidary_db::PgBlobs(pool.clone())
        .blob(&hash)
        .await
        .expect("query")
        .expect("the commit recorded a blob row");
    let read = lapidary_storage::SourceReader::open(&server.state.blob_root)
        .get(&hash, Some(stored.zstd_level))
        .expect("the blob is in the store");
    assert_eq!(read, BRACKET);

    // 2. The row exists with a zero count. That is the whole point of §4.2: the worker
    //    has not run, so nothing references these bytes, and a blob with no row at all
    //    would be invisible to slice 7's reaper forever.
    let ref_count: i32 = sqlx::query_scalar("SELECT ref_count FROM blob WHERE blake3 = $1")
        .bind(hash.to_hex())
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(
        ref_count, 0,
        "an uploaded blob is a known orphan until the worker runs"
    );

    // 3. The job the worker will pick up, with both facts it needs.
    let (kind, payload): (String, serde_json::Value) =
        sqlx::query_as("SELECT kind, payload FROM job WHERE library_id = $1")
            .bind(library().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("query");
    assert_eq!(kind, "ingest_blob");
    assert_eq!(payload["blake3"], hash.to_hex());
    assert_eq!(payload["path"], "brackets/LP-1042-03.stl");

    // 4. The staged copy is gone. Wasted disk otherwise, and nothing else would ever
    //    look at it.
    assert!(!server.staged(&hash).exists());
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn committing_bytes_that_do_not_match_their_claim_stores_nothing(pool: sqlx::PgPool) {
    // The rule `DATA.md` §5.2 states and the reason it states it: the hash is the dedup
    // key for the whole store, so accepting a wrong one attaches this library's part to
    // whatever else lives at that hash.
    let server = server(pool.clone());
    let lie = BlobHash::from_bytes(*blake3::hash(b"not the bracket").as_bytes());
    let (status, _) = server.chunk(&lie, 0, BRACKET).await;
    assert_eq!(status, StatusCode::OK, "the server cannot know yet");

    let (status, json) = server
        .commit(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": lie.to_hex() }
        ]))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("hash"),
        "the message must say what was wrong with the upload, got: {}",
        json["message"]
    );

    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM job")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(queued, 0, "a refused commit queues nothing");
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM blob WHERE blake3 = $1")
        .bind(lie.to_hex())
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(blobs, 0, "a refused commit records nothing");
    assert!(
        !server.staged(&lie).exists(),
        "known-bad bytes must not be left for a resume to append to"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn committing_without_transferring_says_to_send_the_bytes(pool: sqlx::PgPool) {
    let server = server(pool.clone());
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET).as_bytes());
    let (status, json) = server
        .commit(serde_json::json!([{ "path": "a.stl", "blake3": hash.to_hex() }]))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("a.stl"),
        "the message must name the file, got: {}",
        json["message"]
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_path_that_escapes_is_refused_before_anything_is_written(pool: sqlx::PgPool) {
    // `source_path` never reaches a filesystem on this route — it goes into the `part`
    // column and from there into a Content-Disposition filename — but the refusal is the
    // same one ingest makes, and it lands before the first blob so a manifest with one
    // bad path in it does not leave the user working out which files landed.
    let server = server(pool.clone());
    let good = server.upload(BRACKET).await;
    let (status, json) = server
        .commit(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": good.to_hex() },
            { "path": "../../etc/passwd", "blake3": good.to_hex() },
        ]))
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .expect("a message")
            .contains("etc/passwd"),
        "the message must name the path it refused, got: {}",
        json["message"]
    );
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(
        blobs, 0,
        "the file before the bad one must not have been committed"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_hash_that_is_not_a_hash_never_reaches_the_filesystem(pool: sqlx::PgPool) {
    let server = server(pool);
    let (status, json) = server
        .send(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/libraries/{SEEDED_LIBRARY}/uploads/..%2f..%2fetc%2fpasswd?offset=0"
                ))
                .body(Body::from("x"))
                .expect("request builds"),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"].as_str().expect("a message").contains("64"),
        "the message must say what a hash is, got: {}",
        json["message"]
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn uploading_into_a_library_that_does_not_exist_is_a_404(pool: sqlx::PgPool) {
    // The same probe `scan.rs` makes, for the same reason: without it a manifest against
    // a mistyped id writes blobs, fails the enqueue on a foreign key, and reports a
    // database error for one wrong character in a URL.
    let server = server(pool);
    let hash = BlobHash::from_bytes(*blake3::hash(BRACKET).as_bytes()).to_hex();
    let missing = "01931b6e-0000-7000-8000-00000000dead";
    for route in ["probe", "commit"] {
        let (status, _) = server
            .post(
                &format!("/api/libraries/{missing}/uploads/{route}"),
                serde_json::json!({ "files": [{ "path": "a.stl", "blake3": hash }] }),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{route} must answer 404");
    }
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn the_worker_role_serves_no_upload_route(pool: sqlx::PgPool) {
    // Not `shared`: one binary builds both routers, so a route mounted unconditionally is
    // a route the worker serves too — and the worker mounts no upload volume at all.
    let server = server(pool);
    let response = router(server.state.clone(), Role::Worker)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/libraries/{SEEDED_LIBRARY}/uploads/probe"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"files":[]}"#))
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_realistic_chunk_is_not_refused_as_too_large(pool: sqlx::PgPool) {
    // axum's default body limit is 2 MB, and the client sends 8 MiB chunks. Every test
    // above sends a few kilobytes, so all of them passed against a router that would
    // have answered 413 to every real chunk the browser produced. This is that gap.
    let server = server(pool);
    let big = vec![0x2eu8; 8 * 1024 * 1024];
    let hash = BlobHash::from_bytes(*blake3::hash(&big).as_bytes());

    let (status, json) = server.chunk(&hash, 0, &big).await;
    assert_eq!(status, StatusCode::OK, "answered: {json}");
    assert_eq!(json["received"], 8 * 1024 * 1024);
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_chunk_past_the_limit_is_refused_in_words_rather_than_by_the_framework(
    pool: sqlx::PgPool,
) {
    // Comfortably past the limit, not one byte past it. The version this replaces sent
    // exactly `limit + 1`, which was the only size at which the handler's own length
    // check could fire — every genuinely oversized chunk got axum's "Failed to buffer the
    // request body: length limit exceeded" instead, which names our framework rather than
    // the caller's mistake. A live 35 MB PUT is what showed it; the test did not, because
    // it was measuring the one-byte window the check still owned.
    let server = server(pool);
    let far_too_big = vec![0x2eu8; 40 * 1024 * 1024];
    let hash = BlobHash::from_bytes(*blake3::hash(&far_too_big).as_bytes());

    let (status, json) = server.chunk(&hash, 0, &far_too_big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let message = json["message"].as_str().expect("a message");
    assert!(
        message.contains("smaller chunks"),
        "must say what to do, got: {message}"
    );
    assert!(
        !message.contains("buffer"),
        "must not be axum's own rejection text, got: {message}"
    );
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn committing_nothing_is_a_success_with_no_batch_to_watch(pool: sqlx::PgPool) {
    // Re-dropping a folder this library already holds. The probe puts every file in
    // `have`, so the client commits an empty manifest — and this must be `202` with
    // `queued: 0`, the same "success with nothing to poll" a thumbnail sweep answers when
    // nothing is missing. A 500 or a 400 here turns an unchanged re-drop into a failure
    // banner, and the copy that says "every file is already in this library" would never
    // be reachable.
    let server = server(pool);
    let (status, json) = server.commit(serde_json::json!([])).await;

    assert_eq!(status, StatusCode::ACCEPTED);
    let accepted: ScanAccepted = serde_json::from_value(json).expect("body is a ScanAccepted");
    assert_eq!(accepted.queued, 0);
}
