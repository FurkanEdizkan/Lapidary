//! The three sites, served from a fixture server in the shapes they answer.
//!
//! Printables' answers are trimmed from the live API (model 482910, 2026-10-16). Thingiverse's and
//! MyMiniFactory's could not be fetched from the development machine — both sat behind a Cloudflare
//! challenge — so theirs follow responses other importers recorded (manyfold's cassettes: thing
//! 4049220, object 2052) and MyMiniFactory's own OpenAPI file.

use axum::Json;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use lapidary_sites::{Site, SiteError, Sites};
use serde_json::{Value, json};
use std::net::Ipv4Addr;

const STL: &[u8] = b"solid go-cruise\nendsolid go-cruise\n";
const PICTURE: &[u8] = b"RIFF\x24\x00\x00\x00WEBPVP8 fixture";

/// The fixture server: every site under its own prefix, as `Sites::local` expects.
async fn serve() -> String {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("a port");
    let base = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let b = base.clone();
    let app = axum::Router::new()
        .route(
            "/printables/graphql/",
            post(move |Json(body): Json<Value>| {
                let b = b.clone();
                async move { Json(printables(&b, &body)) }
            }),
        )
        .route("/printables-media/{*path}", get(|| async { picture() }))
        .route("/files/{*path}", get(|| async { STL }))
        .route("/thingiverse/users/me", get(|h: HeaderMap| async move { keyed(&h, json!({"name": "floppy_uk"})) }))
        .route(
            "/thingiverse/things/{id}",
            get({
                let b = base.clone();
                move |Path(id): Path<String>, h: HeaderMap| {
                    let b = b.clone();
                    async move {
                        if id != "4049220" {
                            return (StatusCode::NOT_FOUND, Json(json!({"error": format!("Thing {id} does not exist")}))).into_response();
                        }
                        keyed(&h, json!({
                            "id": 4049220, "name": "Slatwall support", "public_url": "https://www.thingiverse.com/thing:4049220",
                            "license": "Creative Commons - Attribution - Share Alike",
                            "creator": {"name": "floppy_uk"}, "description": "A bracket for slatwall.",
                            "tags": [{"name": "slatwall"}, {"name": "bracket"}],
                            "default_image": {"url": format!("{b}/files/default.jpg")}
                        }))
                    }
                }
            }),
        )
        .route(
            "/thingiverse/things/{id}/files",
            get({
                let b = base.clone();
                move |h: HeaderMap| {
                    let b = b.clone();
                    async move {
                        keyed(&h, json!([
                            {"id": 1, "name": "slatwall-support.stl", "size": 35, "direct_url": format!("{b}/files/slatwall-support.stl"), "download_url": format!("{b}/thingiverse/files/1/download")},
                            {"id": 2, "name": "slatwall-support.scad", "size": 900, "direct_url": null, "download_url": format!("{b}/thingiverse/files/2/download")}
                        ]))
                    }
                }
            }),
        )
        .route(
            "/thingiverse/things/{id}/images",
            get({
                let b = base.clone();
                move |h: HeaderMap| {
                    let b = b.clone();
                    async move {
                        keyed(&h, json!([{"id": 9, "sizes": [
                            {"type": "thumb", "size": "large", "url": format!("{b}/files/thumb.jpg")},
                            {"type": "display", "size": "large", "url": format!("{b}/files/display.jpg")}
                        ]}]))
                    }
                }
            }),
        )
        .route("/thingiverse/files/{id}/download", get(|h: HeaderMap| async move {
            if bearer(&h) == Some("tv-token") { STL.into_response() } else { StatusCode::UNAUTHORIZED.into_response() }
        }))
        .route("/mmf/api/v2/user", get(|h: HeaderMap| async move { keyed(&h, json!({"username": "scantheworld"})) }))
        .route("/mmf/api/v2/categories", get(|uri: Uri| async move { mmf_key(&uri, json!({"total_count": 1, "items": []})) }))
        .route(
            "/mmf/api/v2/objects/{id}",
            get({
                let b = base.clone();
                move |h: HeaderMap, uri: Uri| {
                    let b = b.clone();
                    async move {
                        // An API key reads the object as `?key=`, with no download link (A14).
                        let api_key = uri.query().is_some_and(|q| q.contains("key=mmf-api-key"));
                        // The download link is only there for a signed-in account, as the spec says.
                        let link = if bearer(&h).is_some() { json!(format!("{b}/files/david.stl")) } else { Value::Null };
                        let object = json!({
                            "id": 2052, "url": "https://www.myminifactory.com/object/3d-print-michelangelo-s-david-in-florence-italy-2052",
                            "name": "David", "description": "When all was finished.",
                            "license": "Download - Share - Maintain License - Credit - Remix - Commercial - Exclusive",
                            "tags": ["david", "sculpture", "Marble"],
                            "categories": {"total_count": 2, "items": [{"id": 1, "name": "Art"}, {"id": 2, "name": "Scan The World"}]},
                            "designer": {"id": 20099, "username": "scantheworld", "name": "Scan The World"},
                            "images": [
                                {"id": 1, "is_primary": false, "standard": {"url": format!("{b}/files/side.jpg")}},
                                {"id": 2, "is_primary": true, "large": {"url": format!("{b}/files/front.jpg")}, "standard": {"url": format!("{b}/files/front-720.jpg")}}
                            ],
                            "files": {"total_count": 1, "items": [{"id": 141114, "filename": "scan-the-world-michelangelo-s-david.stl", "size": "35", "download_url": link}]}
                        });
                        if api_key { Json(object).into_response() } else { keyed(&h, object) }
                    }
                }
            }),
        )
        .route("/cf/{*path}", get(|| async {
            (StatusCode::FORBIDDEN, [("cf-mitigated", "challenge"), (header::CONTENT_TYPE.as_str(), "text/html")], "<!DOCTYPE html><title>Just a moment...</title>").into_response()
        }));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
}

fn picture() -> Response {
    ([(header::CONTENT_TYPE, "image/webp")], PICTURE).into_response()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Thingiverse and MyMiniFactory answer 401 without their key; these fixtures take `tv-token` and
/// `mmf-token` and nothing else.
fn keyed(headers: &HeaderMap, body: Value) -> Response {
    match bearer(headers) {
        Some("tv-token" | "mmf-token") => Json(body).into_response(),
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized", "code": 401, "type": "NO_TOKEN_PROVIDED"})),
        )
            .into_response(),
    }
}

/// MyMiniFactory's API key goes in the query, and only `mmf-api-key` is one.
fn mmf_key(uri: &Uri, body: Value) -> Response {
    if uri.query().is_some_and(|q| q.contains("key=mmf-api-key")) {
        Json(body).into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "access_denied"})),
        )
            .into_response()
    }
}

fn printables(base: &str, body: &Value) -> Value {
    let query = body["query"].as_str().unwrap_or_default();
    if query.contains("getDownloadLink") {
        return if body["variables"]["id"] == "2080405" {
            json!({"data": {"getDownloadLink": {"ok": true, "errors": null, "output": {"link": format!("{base}/files/go-cruise.stl"), "ttl": 86400}}}})
        } else {
            json!({"data": {"getDownloadLink": {"ok": false, "errors": [{"field": "non_field_error", "messages": ["files_cannot_be_downloaded"]}], "output": null}}})
        };
    }
    if query.contains("__typename") {
        return json!({"data": {"__typename": "Query"}});
    }
    if body["variables"]["id"] != "482910" {
        return json!({"data": {"print": null}});
    }
    json!({"data": {"print": {
        "id": "482910", "name": "Motorcycle Throttle Stop (Cruise Control)", "slug": "motorcycle-throttle-stop-cruise-control",
        "summary": "Modeled in F360 after the original Go-Cruise.", "description": "<p>Modeled in F360 …</p>",
        "license": {"name": "Creative Commons — Attribution", "abbreviation": "CC-BY"},
        "user": {"publicUsername": "3Diocrity", "handle": "JordanK_858807"},
        "tags": [{"name": "motorcycles"}, {"name": "cruisecontrol"}],
        "images": [{"filePath": "media/prints/482910/images/3945197_061ead02/go-cruise-on-bike.jpg"}],
        "stls": [{"id": "2080405", "name": "Go-Cruise.stl", "fileSize": 98184}, {"id": "2080406", "name": "Go-Cruise.f3d", "fileSize": 757829}]
    }}})
}

#[tokio::test]
async fn a_printables_model_reads_whole_and_its_free_file_downloads_with_no_key() {
    let sites = Sites::local(&serve().await);
    let model = sites
        .model(Site::Printables, "482910", None)
        .await
        .expect("reads");
    assert_eq!(model.name, "Motorcycle Throttle Stop (Cruise Control)");
    assert_eq!(model.creator.as_deref(), Some("3Diocrity"));
    assert_eq!(
        model.license.as_deref(),
        Some("Creative Commons — Attribution")
    );
    assert_eq!(model.tags, ["motorcycles", "cruisecontrol"]);
    assert_eq!(
        model.url,
        "https://www.printables.com/model/482910-motorcycle-throttle-stop-cruise-control"
    );
    assert!(model.images[0].ends_with(
        "/printables-media/media/prints/482910/images/3945197_061ead02/go-cruise-on-bike.jpg"
    ));
    assert_eq!(
        sites.image(&model.images[0]).await.expect("a picture"),
        PICTURE
    );
    let names: Vec<&str> = model.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Go-Cruise.stl", "Go-Cruise.f3d"]);
    assert_eq!(
        sites
            .download(&model, &model.files[0], None)
            .await
            .expect("downloads"),
        STL
    );
    // A file Printables will not hand out — paid, or a club model — says so.
    assert!(matches!(
        sites.download(&model, &model.files[1], None).await,
        Err(SiteError::NotFree {
            site: Site::Printables
        })
    ));
    assert!(matches!(
        sites.model(Site::Printables, "1", None).await,
        Err(SiteError::NotFound {
            site: Site::Printables,
            ..
        })
    ));
    assert!(sites.check(Site::Printables, None).await.is_ok());
}

#[tokio::test]
async fn a_thingiverse_thing_needs_its_token_and_reads_whole_with_it() {
    let sites = Sites::local(&serve().await);
    assert!(matches!(
        sites.model(Site::Thingiverse, "4049220", None).await,
        Err(SiteError::NoKey {
            site: Site::Thingiverse
        })
    ));
    assert!(matches!(
        sites
            .model(Site::Thingiverse, "4049220", Some("wrong"))
            .await,
        Err(SiteError::BadKey {
            site: Site::Thingiverse
        })
    ));
    assert_eq!(
        sites
            .check(Site::Thingiverse, Some("tv-token"))
            .await
            .expect("a good token"),
        "floppy_uk"
    );
    assert!(matches!(
        sites.check(Site::Thingiverse, Some("wrong")).await,
        Err(SiteError::BadKey { .. })
    ));

    let model = sites
        .model(Site::Thingiverse, "4049220", Some("tv-token"))
        .await
        .expect("reads");
    assert_eq!(model.name, "Slatwall support");
    assert_eq!(model.creator.as_deref(), Some("floppy_uk"));
    assert_eq!(model.tags, ["slatwall", "bracket"]);
    assert!(
        model.images[0].ends_with("/files/display.jpg"),
        "the large display picture: {:?}",
        model.images
    );
    // A CDN file needs no token; the API's own download link takes it.
    for file in &model.files {
        assert_eq!(
            sites
                .download(&model, file, Some("tv-token"))
                .await
                .expect(&file.name),
            STL
        );
    }
    assert!(matches!(
        sites
            .model(Site::Thingiverse, "7075185", Some("tv-token"))
            .await,
        Err(SiteError::NotFound { .. })
    ));
}

#[tokio::test]
async fn a_myminifactory_object_downloads_only_with_an_account_token() {
    let sites = Sites::local(&serve().await);
    assert_eq!(
        sites
            .check(Site::MyMiniFactory, Some("mmf-token"))
            .await
            .expect("a good token"),
        "scantheworld"
    );
    let model = sites
        .model(Site::MyMiniFactory, "2052", Some("mmf-token"))
        .await
        .expect("reads");
    assert_eq!(model.name, "David");
    assert_eq!(model.creator.as_deref(), Some("Scan The World"));
    assert_eq!(model.tags, ["david", "sculpture", "Marble"]);
    assert!(
        model.images[0].ends_with("/files/front.jpg"),
        "the primary, at its largest: {:?}",
        model.images
    );
    assert_eq!(
        sites
            .download(&model, &model.files[0], Some("mmf-token"))
            .await
            .expect("downloads"),
        STL
    );
}

#[tokio::test]
async fn a_myminifactory_api_key_reads_the_object_but_cannot_download() {
    let sites = Sites::local(&serve().await);
    let key = Some("mmf-api-key");
    assert!(
        sites
            .check(Site::MyMiniFactory, key)
            .await
            .expect("a good API key")
            .contains("API key"),
        "the check says what an API key can do"
    );
    let model = sites
        .model(Site::MyMiniFactory, "2052", key)
        .await
        .expect("reads");
    assert_eq!(model.name, "David");
    assert_eq!(model.tags, ["david", "sculpture", "Marble"]);
    assert_eq!(model.images.len(), 2);
    assert_eq!(
        model.files[0].size,
        Some(35),
        "a size sent as a string is still read"
    );
    assert!(matches!(
        sites.download(&model, &model.files[0], key).await,
        Err(SiteError::NotFree { .. })
    ));
    assert!(matches!(
        sites.check(Site::MyMiniFactory, Some("not-a-key")).await,
        Err(SiteError::BadKey { .. })
    ));
}

#[tokio::test]
async fn a_cloudflare_challenge_is_named_as_one_and_never_as_a_bad_key() {
    // Every site answered from `/cf/…`, which is a Cloudflare challenge whatever is asked.
    let sites = Sites::local(&format!("{}/cf", serve().await));
    for site in [Site::Thingiverse, Site::MyMiniFactory] {
        match sites.check(site, Some("tv-token")).await {
            Err(SiteError::Cloudflare { site: named }) => assert_eq!(named, site),
            other => panic!("{site:?}: {other:?}"),
        }
    }
}
