//! The web console, embedded at compile time by `build.rs`.

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// The `<base>` tag in `web/index.html`; the served copy points it at `--base-path`.
const BASE_TAG: &str = r#"<base href="/" />"#;

const MISSING: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>jevmarket</title></head>
<body style="font-family: system-ui, sans-serif; max-width: 40rem; margin: 4rem auto; padding: 0 1rem; line-height: 1.5">
<h1>The web console is not built into this binary</h1>
<p>The daemon and its API run. To get the console, build it and then jevmarket:</p>
<pre>cd web &amp;&amp; bun install &amp;&amp; bun run build &amp;&amp; cd .. &amp;&amp; cargo build --release</pre>
<p>The API is served under <code>api/</code>, for example <code>api/status</code>.</p>
</body></html>"#;

fn get(path: &str) -> Option<&'static [u8]> {
    ASSETS.binary_search_by_key(&path, |(name, _)| name).ok().map(|i| ASSETS[i].1)
}

/// `index.html` with its `<base>` pointing at `base_path`, or `None` if the console was not built.
pub fn index_html(base_path: &str) -> Option<String> {
    let html = String::from_utf8_lossy(get("index.html")?);
    Some(html.replacen(BASE_TAG, &format!(r#"<base href="{base_path}/" />"#), 1))
}

/// A file from the build, or the console itself for any other path (client-side routing).
pub fn serve(path: &str, index: Option<&str>) -> Response {
    let path = path.trim_start_matches('/');
    if let Some(bytes) = get(path).filter(|_| path != "index.html") {
        // Vite puts content hashes in every file name under `assets/`.
        let cache =
            if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "public, max-age=3600" };
        return ([(CONTENT_TYPE, content_type(path)), (CACHE_CONTROL, cache)], bytes).into_response();
    }
    if path.starts_with("assets/") || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut response = Html(index.unwrap_or(MISSING).to_owned()).into_response();
    response.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
