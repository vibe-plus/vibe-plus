use std::borrow::Cow;

use http::{Request, Response};
use rust_embed::RustEmbed;
use wry::WebViewId;

#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct UiAssets;

/// Custom-protocol handler for `app://localhost/*`.
/// Returns the embedded Vue dist asset for the given path, with SPA fallback to index.html.
pub fn handle(_id: WebViewId<'_>, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match UiAssets::get(path) {
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
            // SPA fallback: serve index.html for any unmatched path so Vue Router handles routing.
            match UiAssets::get("index.html") {
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
