use super::*;

pub(super) async fn post_messages_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::Anthropic,
        None,
        headers,
        body,
        Some("plain-v1".into()),
    )
    .await
}

pub(super) async fn post_messages_claude(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::Anthropic,
        None,
        headers,
        body,
        Some("claude-v1".into()),
    )
    .await
}

pub(super) async fn post_chat_completions_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiChat,
        None,
        headers,
        body,
        Some("plain-v1".into()),
    )
    .await
}

pub(super) async fn post_chat_completions_opencode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiChat,
        None,
        headers,
        body,
        Some("opencode-v1".into()),
    )
    .await
}

pub(super) async fn post_responses_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiResponses,
        None,
        headers,
        body,
        Some("plain-v1".into()),
    )
    .await
}

pub(super) async fn post_responses_opencode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiResponses,
        None,
        headers,
        body,
        Some("opencode-v1".into()),
    )
    .await
}

// ---------------------------------------------------------------------------
// Codex WebSocket + HTTP handler for /responses
//
// Codex CLI uses WebSocket as primary transport for /v1/responses:
//   1. Client sends one JSON message  = the HTTP request body
//   2. Server streams back SSE-style events as individual WS text messages
//   3. Server closes the socket when the response is complete
//
// For plain HTTP POST we still forward via `forward`, then may translate upstream Chat SSE
// into Responses-shaped SSE frames for Codex HTTP clients (`C2R`).
// ---------------------------------------------------------------------------

pub(super) fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

/// Unified handler for /codex/v1/responses (and compact/double-prefix variants).
/// Accepts both WS upgrades and plain HTTP POST.

pub(super) async fn post_or_reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if is_websocket_upgrade(&headers) {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "WebSocket not supported on chat/completions",
        )
            .into_response();
    }
    forward::forward(
        state,
        Wire::OpenaiChat,
        None,
        headers,
        body,
        Some("codex-v1".into()),
    )
    .await
}

pub(super) async fn post_gemini(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let upstream_path = format!("/v1beta/models/{}", path);
    forward::forward(
        state,
        Wire::GeminiNative,
        Some(upstream_path),
        headers,
        body,
        Some("gemini-v1".into()),
    )
    .await
}
