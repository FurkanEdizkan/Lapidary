//! The three upload routes, driven through this crate's router against a live Postgres.
//!
//! What each of them promises is checked where the promise actually lives: the probe
//! against the `blob` and `part` rows it sorts by, the chunk against the staged file's
//! length, and the commit against the inbox and the `job` table together — the commit's whole
//! point is that bytes reach the inbox *and* a job is queued to index them there (M4), and a test
//! that read only one of the two would pass on half a commit.

use axum::body::Body;
use axum::http::{Request, StatusCode};
mod common;
use common::router;
use lapidary_api::{AppState, Role};
use lapidary_core::{BlobHash, LibraryId, ScanAccepted};
use tower::ServiceExt;

/// Seeded by `crates/lapidary-db/migrations/0002_parts.sql`.
const SEEDED_LIBRARY: &str = "01931b6e-0000-7000-8000-000000000001";
const BRACKET: &[u8] = include_bytes!("../../../fixtures/bracket-lp-1042-03.stl");

/// Two temp roots, because the whole design turns on them being different: a
/// half-transferred file must never sit inside a store whose contract is that everything
/// in it is complete.
struct Server {
    blobs: tempfile::TempDir,
    uploads: tempfile::TempDir,
    inbox: tempfile::TempDir,
    state: AppState,
}

fn server(pool: sqlx::PgPool) -> Server {
    let blobs = tempfile::tempdir().expect("blob root");
    let uploads = tempfile::tempdir().expect("upload dir");
    let inbox = tempfile::tempdir().expect("inbox");
    let state = AppState {
        db: pool,
        blob_root: blobs.path().to_path_buf(),
        upload_dir: uploads.path().to_path_buf(),
        host_storage_root: None,
        ingest_dir: std::path::PathBuf::from("/nonexistent-ingest"),
        inbox_dir: inbox.path().to_path_buf(),
        host_ingest_root: None,
        host_inbox_root: None,
        touches: Default::default(),
    };
    Server {
        blobs,
        uploads,
        inbox,
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
    assert_eq!(
        json["needBytes"],
        serde_json::json!(["brackets/LP-1042-03.stl"]),
        "a library that holds nothing needs the bytes"
    );
    assert!(
        json.get("needRows").is_none(),
        "no stored copy is reused since M4, so there is no third answer: {json}"
    );
}

/// A path is held only with the bytes its part's current revision has. The same path with other bytes is a
/// change to send, and so are the held bytes under another path: there is no stored copy to reuse (M4).
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_probe_holds_a_path_only_with_the_bytes_its_part_has(pool: sqlx::PgPool) {
    let held = BlobHash::from_bytes(*blake3::hash(BRACKET).as_bytes());
    lapidary_db::PgIngest(pool.clone())
        .record(lapidary_db::IngestRequest {
            origin: lapidary_core::RevisionOrigin::Ingest,
            library: library(),
            name: "Bracket, LP-1042-03",
            source_path: "brackets/LP-1042-03.stl",
            folder: None,
            storage_path: Some("libraries/default/lp-1042-03/LP-1042-03.stl"),
            root: lapidary_core::FileRoot::Store,
            mtime_ns: None,
            blob: &lapidary_db::StoredBlobRow {
                hash: held,
                size_bytes: BRACKET.len() as u64,
                stored_bytes: BRACKET.len() as u64,
                zstd_level: 0,
            },
            measurements: &lapidary_core::MeshMeasurements {
                bbox_mm: [80.0, 40.0, 3.0],
                triangle_count: 12_480,
                surface_area_mm2: 7_136.0,
                volume_mm3: Some(9_120.0),
                is_watertight: true,
            },
            provenance: lapidary_core::MeasurementProvenance::TESSELLATED,
            format: "stl",
        })
        .await
        .expect("records the bracket");
    let changed = BlobHash::from_bytes(*blake3::hash(b"the bracket, re-exported").as_bytes());

    let server = server(pool);
    let (status, json) = server
        .probe(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": changed.to_hex() },
            { "path": "copies/LP-1042-03.stl", "blake3": held.to_hex() },
            { "path": "brackets/LP-1042-03.stl", "blake3": held.to_hex() }
        ]))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["have"], serde_json::json!(["brackets/LP-1042-03.stl"]));
    assert_eq!(
        json["needBytes"],
        serde_json::json!(["brackets/LP-1042-03.stl", "copies/LP-1042-03.stl"]),
        "the held path with other bytes is a change to send, and a copy is sent too"
    );
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

/// (e) An upload is written once into the inbox, at the path the browser reported, and queued to be
/// indexed from there. Nothing goes into Lapidary's store.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn committing_writes_the_file_into_the_inbox_and_queues_its_index(pool: sqlx::PgPool) {
    let server = server(pool.clone());
    let hash = server.upload(BRACKET).await;
    assert!(server.staged(&hash).exists(), "the transfer staged a file");

    let (status, json) = server
        .commit(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": hash.to_hex() }
        ]))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    let accepted: ScanAccepted = serde_json::from_value(json).expect("body is a ScanAccepted");
    assert_eq!(accepted.queued, 1);

    // 1. The bytes are in the inbox, byte-identical, under the browser's path.
    let placed = server
        .inbox
        .path()
        .join("uploads/lena.kowalski/brackets/LP-1042-03.stl");
    assert_eq!(std::fs::read(&placed).expect("in the inbox"), BRACKET);

    // 2. Nothing went into the store: no blob row, no file.
    let blobs: i64 = sqlx::query_scalar("SELECT count(*) FROM blob")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(blobs, 0, "an upload records nothing in the store");
    assert_eq!(
        std::fs::read_dir(server.blobs.path())
            .expect("reads")
            .count(),
        0,
        "and writes nothing there"
    );

    // 3. The job the worker picks up: where the file is, and which part it is.
    let (kind, payload): (String, serde_json::Value) =
        sqlx::query_as("SELECT kind, payload FROM job WHERE library_id = $1")
            .bind(library().as_uuid())
            .fetch_one(&pool)
            .await
            .expect("query");
    assert_eq!(kind, "ingest_inbox");
    assert_eq!(
        payload["inbox"],
        "uploads/lena.kowalski/brackets/LP-1042-03.stl"
    );
    assert_eq!(payload["path"], "brackets/LP-1042-03.stl");

    // 4. The staged copy is gone.
    assert!(!server.staged(&hash).exists());

    // 5. The same path again, with other bytes: a new file beside the first, never over it.
    let changed = [BRACKET, b"\n"].concat();
    let again = server.upload(&changed).await;
    let (status, json) = server
        .commit(serde_json::json!([
            { "path": "brackets/LP-1042-03.stl", "blake3": again.to_hex() }
        ]))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    assert_eq!(std::fs::read(&placed).expect("still there"), BRACKET);
    assert_eq!(
        std::fs::read(
            server
                .inbox
                .path()
                .join("uploads/lena.kowalski/brackets/LP-1042-03 (2).stl")
        )
        .expect("beside it"),
        changed
    );
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
    assert!(
        !server
            .inbox
            .path()
            .join("uploads/lena.kowalski/brackets/LP-1042-03.stl")
            .exists(),
        "a refused commit places nothing in the inbox"
    );
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
async fn a_drop_with_no_mesh_in_it_is_refused_before_a_byte_is_transferred(pool: sqlx::PgPool) {
    // The probe is the point: it runs before the transfer, so a folder of STEP assemblies is
    // refused rather than moved and then failed a job at a time. The sentence is the worker's own
    // — one definition, in `lapidary_core::cad_refusal`, so a person who meets both meets the
    // same words.
    //
    // The `README.md` is not decoration. Nothing in `web/src/lib/upload.ts` filters a drop by
    // extension, so a real folder of assemblies arrives with its notes and drawings in the
    // manifest — and a rule that fired only when *every* file was a B-rep file would never fire
    // here, which is the one case it is for.
    let server = server(pool.clone());
    let (status, json) = server
        .probe(serde_json::json!([
            { "path": "fixtures/fixture-plate-lp-9000-00.STEP", "blake3": "a".repeat(64) },
            { "path": "assemblies/gearbox-lp-7700-00.iges", "blake3": "b".repeat(64) },
            { "path": "README.md", "blake3": "c".repeat(64) },
        ]))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = json["message"].as_str().expect("a message");
    assert!(
        message.contains("fixture-plate-lp-9000-00.STEP"),
        "names the file it refused: {message}"
    );
    assert!(
        message.contains("STEP") && message.contains("STL, OBJ or 3MF"),
        "names the format and what this library takes: {message}"
    );
    assert!(
        !message.contains("occt") && !message.contains("--features"),
        "and never a build flag: {message}"
    );

    // And again at the commit, for a client that never probed: nothing is stored.
    let good = server.upload(BRACKET).await;
    let (status, _) = server
        .commit(serde_json::json!([
            { "path": "assemblies/gearbox-lp-7700-00.iges", "blake3": good.to_hex() },
        ]))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let parts: i64 = sqlx::query_scalar("SELECT count(*) FROM part")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(parts, 0, "and no part is indexed");
}

#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn a_drop_with_one_b_rep_file_among_meshes_still_takes_the_meshes(pool: sqlx::PgPool) {
    // There is a mesh to take, so the drop is taken and the `.step` file is refused by its own job
    // — the same answer a scan gives, and the batch carries the same sentence. A whole-manifest
    // refusal here would upload nothing for a hobby corpus of 400 meshes with one `.step` in it.
    let server = server(pool.clone());
    let good = server.upload(BRACKET).await;
    let (status, json) = server
        .commit(serde_json::json!([
            { "path": "brackets/bracket-lp-1042-03.stl", "blake3": good.to_hex() },
            { "path": "fixtures/fixture-plate-lp-9000-00.step", "blake3": good.to_hex() },
        ]))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    assert_eq!(json["queued"], 2, "both files are queued: {json}");
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

/// A drop's files are placed several at a time: two copies of a file under different names share one
/// staged file, each lands in the inbox under its own name, and every file queues its own job.
#[sqlx::test(migrations = "../lapidary-db/migrations")]
async fn committing_many_files_places_each_and_queues_every_file(pool: sqlx::PgPool) {
    let server = server(pool.clone());
    let mut files = Vec::new();
    for n in 0..12u8 {
        let bytes = [BRACKET, &[n][..]].concat();
        let hash = server.upload(&bytes).await;
        files.push(serde_json::json!({
            "path": format!("brackets/LP-1042-{n:02}.stl"),
            "blake3": hash.to_hex(),
        }));
    }
    let copy = files[0]["blake3"].clone();
    files.push(serde_json::json!({ "path": "spares/LP-1042-00-copy.stl", "blake3": copy }));

    let (status, json) = server.commit(serde_json::Value::Array(files)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{json}");
    assert_eq!(json["queued"], 13);
    let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM job WHERE kind = 'ingest_inbox'")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(jobs, 13);
    assert_eq!(
        std::fs::read(server.inbox.path().join("spares/LP-1042-00-copy.stl"))
            .err()
            .map(|e| e.kind()),
        Some(std::io::ErrorKind::NotFound),
        "under uploads/<the uploader>/, not loose in the inbox"
    );
    assert_eq!(
        std::fs::read(
            server
                .inbox
                .path()
                .join("uploads/lena.kowalski/spares/LP-1042-00-copy.stl")
        )
        .expect("the copy"),
        std::fs::read(
            server
                .inbox
                .path()
                .join("uploads/lena.kowalski/brackets/LP-1042-00.stl")
        )
        .expect("the original")
    );
}
