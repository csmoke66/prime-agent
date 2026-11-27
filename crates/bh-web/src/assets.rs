//! Embedded frontend bundle (`crates/bh-web/assets`, built from `web/` by `make web`) with single-page-app fallback.

use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Bundle;

const NOT_BUILT: &str = "<!doctype html><title>Beehive</title><body style=\"font-family:sans-serif;background:#050506;color:#f4f4f5;padding:2rem\"><h1>Frontend not built</h1><p>Run <code>make web</code> and rebuild, or use <code>npm run dev</code> in <code>web/</code>.</p>";

fn content_type(path: &str) -> String {
    let extension = path.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub(crate) async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some(file) = Bundle::get(path).filter(|_| !path.is_empty()) {
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, content_type(path)),
                (header::CACHE_CONTROL, cache.to_string()),
            ],
            file.data.into_owned(),
        )
            .into_response();
    }
    match Bundle::get("index.html") {
        Some(index) => (
            [(header::CACHE_CONTROL, "no-cache")],
            Html(index.data.into_owned()),
        )
            .into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE, Html(NOT_BUILT)).into_response(),
    }
}
