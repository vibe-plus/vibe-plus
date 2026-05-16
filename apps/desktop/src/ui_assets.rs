use std::borrow::Cow;
use std::path::PathBuf;

use http::{Request, Response};
use rust_embed::RustEmbed;
use wry::WebViewId;

/// Embedded snapshot of the UI dist baked into the binary at compile time.
/// Used as offline fallback when no newer cached version is available.
#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct EmbeddedUi;

/// Custom-protocol handler for `app://localhost/*`.
///
/// Serving priority:
///   1. `~/.vibe/ui-cache/dist/` — a newer version downloaded by the background
///      updater; invalidated and replaced atomically on each successful update.
///   2. `EmbeddedUi` — the snapshot baked into the binary at compile time.
///   3. SPA fallback: `index.html` (from cache or embedded, same priority order).
pub fn handle(_id: WebViewId<'_>, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(resp) = try_cached(path) {
        return resp;
    }
    serve_embedded(path)
}

/// Returns the version string embedded in `dist/version.json`, or `"unknown"`.
/// Called once at startup to seed the update checker.
pub fn embedded_ui_version() -> String {
    EmbeddedUi::get("version.json")
        .and_then(|f| serde_json::from_slice::<serde_json::Value>(&f.data).ok())
        .and_then(|v| v["version"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Returns `~/.vibe/ui-cache/dist/`, creating it if necessary.
/// `None` when the home directory cannot be determined.
pub fn ui_cache_dist_dir() -> Option<PathBuf> {
    let home = std::env::var_os("VIBE_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::UserDirs::new().map(|u| u.home_dir().join(".vibe")))?;
    Some(home.join("ui-cache").join("dist"))
}

/// Path to `~/.vibe/ui-cache/version.json` — written last after a successful
/// download so it acts as an atomic "cache is valid" marker.
pub fn ui_cache_version_file() -> Option<PathBuf> {
    ui_cache_dist_dir().map(|d| d.parent().unwrap().join("version.json"))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn try_cached(path: &str) -> Option<Response<Cow<'static, [u8]>>> {
    let cache_path = ui_cache_dist_dir()?.join(path);
    let data = std::fs::read(&cache_path).ok()?;
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    Response::builder()
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Owned(data))
        .ok()
}

fn serve_embedded(path: &str) -> Response<Cow<'static, [u8]>> {
    match EmbeddedUi::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            Response::builder()
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(file.data)
                .unwrap()
        }
        None => {
            // SPA fallback: try cache then embedded index.html
            if let Some(resp) = try_cached("index.html") {
                return resp;
            }
            match EmbeddedUi::get("index.html") {
                Some(file) => Response::builder()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(file.data)
                    .unwrap(),
                None => Response::builder()
                    .status(404)
                    .body(Cow::Borrowed(b"not found" as &[u8]))
                    .unwrap(),
            }
        }
    }
}
