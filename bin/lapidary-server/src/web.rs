//! The browser app, served by the same process as the API.
//!
//! This was a Caddy container in front of the api until S1 (2026-10-16). Caddy did three things for
//! us and this module does the same three: hand out the files `npm run build` wrote to `web/dist`,
//! answer any path the single-page app owns with `index.html`, and leave `/api/*` alone so an
//! unknown API route stays a 404 rather than becoming an HTML page with a 200.
//!
//! `lapidary-api` stays a library that builds a Router; the files are mounted here, by the binary,
//! because where they sit on disk is a fact about this deployment.

use axum::Router;
use axum::http::{Extensions, HeaderMap, HeaderValue, StatusCode, Uri, Version, header};
use axum::response::{IntoResponse, Response};
use std::path::Path;
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{Predicate, SizeAbove};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

/// Vite names everything under `assets/` by a hash of its contents, so a file there never changes
/// under its name and a browser may keep it for a year.
const IMMUTABLE: HeaderValue = HeaderValue::from_static("public, max-age=31536000, immutable");

/// `index.html` and the other top-level files are not hashed: a browser must ask every time, or a
/// new build would keep loading last week's bundle.
const REVALIDATE: HeaderValue = HeaderValue::from_static("no-cache");

/// Gzip for what is text, and only that (A34): at 20,000 parts the category tree is 1.6 MB of JSON and 0.3 MB
/// gzipped. A model file, a picture or a download is never compressed in flight, so a download stays the bytes on
/// disk with their own length, and a stream of events is never held back to fill a block. Gzip only: flate2 was
/// already in the tree.
fn textual(_: StatusCode, _: Version, headers: &HeaderMap, _: &Extensions) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .is_some_and(|kind| {
            [
                "application/json",
                "text/html",
                "text/css",
                "text/javascript",
            ]
            .iter()
            .any(|text| kind.starts_with(text))
        })
}

/// `app` with the browser app at `dist` behind every route it does not answer itself.
pub fn with_web(app: Router, dist: &Path) -> Router {
    let assets = tower::ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            IMMUTABLE,
        ))
        .service(ServeDir::new(dist.join("assets")));
    let pages = ServeDir::new(dist).fallback(ServeFile::new(dist.join("index.html")));
    app.nest_service("/assets", assets)
        .fallback(move |uri: Uri, request: axum::extract::Request| {
            let pages = pages.clone();
            async move {
                // An API route nobody mounted is the API's 404, not the app's front page.
                if uri.path() == "/api" || uri.path().starts_with("/api/") {
                    return StatusCode::NOT_FOUND.into_response();
                }
                let mut response: Response = match pages.oneshot(request).await {
                    Ok(response) => response.into_response(),
                    Err(never) => match never {},
                };
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, REVALIDATE);
                response
            }
        })
        .layer(CompressionLayer::new().compress_when(SizeAbove::new(1024).and(textual)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;

    async fn get_path(app: &Router, path: &str) -> (StatusCode, Option<String>, String) {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("responds");
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .map(|value| value.to_str().expect("ascii").to_owned());
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        (status, cache, String::from_utf8_lossy(&body).into_owned())
    }

    fn dist() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<!doctype html>lapidary").expect("index");
        std::fs::write(dir.path().join("favicon.svg"), "<svg/>").expect("favicon");
        std::fs::create_dir(dir.path().join("assets")).expect("assets");
        std::fs::write(dir.path().join("assets/index-C8WOGGBF.js"), "export{}").expect("asset");
        dir
    }

    fn app(dist: &Path) -> Router {
        let api = Router::new().route("/api/healthz", get(|| async { "ok" }));
        with_web(api, dist)
    }

    #[tokio::test]
    async fn the_front_page_and_any_app_route_are_the_app() {
        let dist = dist();
        let app = app(dist.path());
        for path in [
            "/",
            "/parts/0193a0c2-7d40-7000-8000-000000000042",
            "/tags/flanges",
        ] {
            let (status, cache, body) = get_path(&app, path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(body, "<!doctype html>lapidary", "{path}");
            assert_eq!(cache.as_deref(), Some("no-cache"), "{path}");
        }
    }

    #[tokio::test]
    async fn a_hashed_asset_is_kept_for_a_year_and_a_top_level_file_is_not() {
        let dist = dist();
        let app = app(dist.path());
        let (status, cache, body) = get_path(&app, "/assets/index-C8WOGGBF.js").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "export{}"));
        assert_eq!(
            cache.as_deref(),
            Some("public, max-age=31536000, immutable")
        );
        let (status, cache, body) = get_path(&app, "/favicon.svg").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "<svg/>"));
        assert_eq!(cache.as_deref(), Some("no-cache"));
    }

    #[tokio::test]
    async fn the_api_keeps_its_routes_and_its_404s() {
        let dist = dist();
        let app = app(dist.path());
        let (status, _, body) = get_path(&app, "/api/healthz").await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
        for path in ["/api/no-such-route", "/api"] {
            let (status, _, body) = get_path(&app, path).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
            assert!(
                !body.contains("doctype"),
                "{path} must not be the app: {body}"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_asset_is_a_404_not_the_front_page() {
        let dist = dist();
        let app = app(dist.path());
        let (status, _, body) = get_path(&app, "/assets/gone-123.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(!body.contains("doctype"));
    }

    /// A34: JSON is gzipped when the browser accepts it; a model file never is, so a download keeps its bytes and
    /// its length.
    #[tokio::test]
    async fn json_is_gzipped_and_a_model_file_is_not() {
        let dist = dist();
        let big = "x".repeat(4096);
        let api = Router::new()
            .route(
                "/api/tree",
                get({
                    let big = big.clone();
                    move || async move { axum::Json(vec![big]) }
                }),
            )
            .route(
                "/api/model",
                get(move || async move {
                    (
                        [(header::CONTENT_TYPE, "model/stl")],
                        big.clone().into_bytes(),
                    )
                }),
            );
        let app = with_web(api, dist.path());
        let encoding = |path: &'static str| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::get(path)
                        .header(header::ACCEPT_ENCODING, "gzip")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("responds")
                .headers()
                .get(header::CONTENT_ENCODING)
                .map(|value| value.to_str().expect("ascii").to_owned())
            }
        };
        assert_eq!(encoding("/api/tree").await.as_deref(), Some("gzip"));
        assert_eq!(encoding("/api/model").await, None);
    }
}
