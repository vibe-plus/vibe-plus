//! Local request proxy: lets the dashboard issue arbitrary HTTP requests
//! through the gateway (sidesteps browser CORS).
//!
//! Frontend sets `X-Vibe-Target: https://api.example.com` (origin only) and
//! calls `ANY /_vp/proxy/<path-to-forward>`. The gateway re-issues the request
//! against `<target><path>?<query>` and streams the upstream response back.

use super::*;
use axum::body::Body;
use axum::extract::{OriginalUri, Path};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];

const TARGET_HEADER: &str = "x-vibe-target";

pub(super) async fn proxy_forward(
    State(state): State<AppState>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    OriginalUri(original): OriginalUri,
    body: Bytes,
) -> Response {
    let Some(target_raw) = headers
        .get(TARGET_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            format!("missing or empty {TARGET_HEADER} header"),
        )
            .into_response();
    };

    let target_base = match reqwest::Url::parse(target_raw) {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid {TARGET_HEADER} URL: {e}"),
            )
                .into_response();
        }
    };
    if !matches!(target_base.scheme(), "http" | "https") {
        return (
            StatusCode::BAD_REQUEST,
            format!("{TARGET_HEADER} must use http or https"),
        )
            .into_response();
    }

    let mut url = target_base.clone();
    {
        let trimmed = url.path().trim_end_matches('/').to_string();
        let suffix = if path.starts_with('/') {
            path.clone()
        } else {
            format!("/{path}")
        };
        url.set_path(&format!("{trimmed}{suffix}"));
    }
    if let Some(query) = original.query() {
        url.set_query(Some(query));
    }

    let mut req = state.http.request(method.clone(), url.clone()).body(body);
    for (name, value) in headers.iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) {
            continue;
        }
        if lower == TARGET_HEADER {
            continue;
        }
        if lower == "origin" || lower == "referer" {
            continue;
        }
        req = req.header(name.clone(), value.clone());
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error=%e, %url, "proxy_forward upstream error");
            return (StatusCode::BAD_GATEWAY, format!("upstream error: {e}")).into_response();
        }
    };

    let status = upstream.status();
    let mut out_headers = HeaderMap::new();
    for (name, value) in upstream.headers().iter() {
        let lower = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) {
            continue;
        }
        if let (Ok(n), Ok(v)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out_headers.append(n, v);
        }
    }

    let stream = upstream.bytes_stream();
    let body = Body::from_stream(stream);
    (status, out_headers, body).into_response()
}
