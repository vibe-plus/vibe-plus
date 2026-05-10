//! axum HTTP server: routes, handlers, listener.

use crate::circuit_breaker::State as CbState;
use crate::forward;
use crate::local_import;
use crate::providers::Wire;
use crate::state::AppState;
use crate::transforms;
use crate::VERSION;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use vibe_protocol::{
    CodexPlanRefreshResult, Credential, CredentialInput, CredentialPlanSnapshot, DashboardStats,
    Health, HealthSummary, LogPage, Provider, ProviderCodexPlanItem, ProviderHealth, ProviderHealthSummary,
    ProviderInput, Status, UsageSummary, WsEvent,
};

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);

    Router::new()
        // health / status
        .route("/health", get(health))
        .route("/status", get(status))
        // Generic model APIs (no tool prefix — for direct / legacy usage)
        .route("/v1/models", get(list_models_all))
        .route("/v1/messages", post(post_messages_plain))
        .route("/v1/chat/completions", post(post_chat_completions_plain))
        .route("/v1/responses", post(post_responses_plain))
        // ── Claude Code tool prefix (/claude/*) ─────────────────────────────
        // ANTHROPIC_BASE_URL = http://127.0.0.1:PORT/claude
        // Claude Code SDK appends /v1/messages, /v1/models, etc.
        .route("/claude/v1/messages", post(post_messages_claude))
        .route("/claude/v1/models", get(list_models_claude))
        // ── Codex tool prefix (/codex/*) ────────────────────────────────────
        // openai_base_url = http://127.0.0.1:PORT/codex/v1
        //
        // Codex CLI uses the WebSocket Responses API as its primary transport.
        // We implement a WS→HTTP bridge: accept the WS upgrade, receive the JSON
        // request body, forward via HTTP to upstream, stream SSE events back as
        // WS text messages.  Chat Completions falls back to plain POST.
        .route("/codex/v1/chat/completions", any(post_or_reject))
        .route("/codex/v1/responses",         any(codex_responses_handler))
        .route("/codex/v1/responses/compact",  any(codex_responses_handler))
        // Codex sometimes sends double-prefix paths (openai_base_url already has /v1)
        .route("/codex/v1/v1/responses",       any(codex_responses_handler))
        .route("/codex/v1/v1/chat/completions",any(post_or_reject))
        .route("/codex/v1/models", get(list_models_openai))
        // ── OpenCode tool prefix (/opencode/*) ──────────────────────────────
        // baseURL = http://127.0.0.1:PORT/opencode/v1
        .route("/opencode/v1/chat/completions", post(post_chat_completions_opencode))
        .route("/opencode/v1/responses", post(post_responses_opencode))
        .route("/opencode/v1/models", get(list_models_openai))
        // Gemini Native passthrough — wildcard captures the full model/action path
        .route("/v1beta/models/*path", post(post_gemini))
        // providers CRUD + local import
        .route("/_vp/providers", get(list_providers).post(create_provider))
        .route("/_vp/providers/import-local", get(scan_local_providers).post(import_local_providers))
        .route(
            "/_vp/providers/:id",
            put(update_provider).delete(delete_provider),
        )
        .route("/_vp/providers/:id/health", get(provider_health))
        .route("/_vp/providers/:id/circuit/reset", post(provider_circuit_reset))
        // credentials
        .route(
            "/_vp/providers/:id/credentials",
            get(list_credentials).post(create_credential),
        )
        .route(
            "/_vp/credentials/:id/plan",
            get(credential_plan_latest),
        )
        .route(
            "/_vp/credentials/:id/plan/refresh",
            post(credential_plan_refresh),
        )
        .route(
            "/_vp/providers/:id/codex-plan",
            get(provider_codex_plan_list),
        )
        .route(
            "/_vp/providers/:id/codex-plan/refresh",
            post(provider_codex_plan_refresh_all),
        )
        .route(
            "/_vp/credentials/:id",
            get(get_credential)
                .put(update_credential)
                .delete(delete_credential),
        )
        // health overview
        .route("/_vp/health/providers", get(health_all_providers))
        // logs + usage + stats
        .route("/_vp/logs/:id", get(get_request_log))
        .route("/_vp/logs", get(list_logs))
        .route("/_vp/usage/summary", get(usage_summary))
        .route("/_vp/stats/dashboard", get(dashboard_stats))
        // websocket
        .route("/_vp/ws", any(ws_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "vibe-core listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Core endpoints
// ---------------------------------------------------------------------------

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

async fn status(State(state): State<AppState>) -> Result<Json<Status>, AppError> {
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let one_hour_ago = chrono::Utc::now().timestamp() - 3600;
    let recent = run_blocking(state.clone(), move |s| s.db.count_logs_since(one_hour_ago)).await?;
    Ok(Json(Status {
        version: VERSION.to_string(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        port: state.port,
        providers_total: providers.len(),
        providers_enabled: providers.iter().filter(|p| p.enabled).count(),
        requests_last_hour: recent,
    }))
}

// ---------------------------------------------------------------------------
// Model discovery
// ---------------------------------------------------------------------------

/// `/v1/models` — 所有启用的别名，OpenAI 格式（兜底 / 通用客户端）
async fn list_models_all(State(state): State<AppState>) -> Response {
    model_list_openai(&state, None).await
}

/// `/claude/v1/models` — 仅 Anthropic 供应商，Anthropic SDK 格式
/// Claude Code 的 Anthropic SDK 期望 `{data:[...], has_more, first_id, last_id}`
async fn list_models_claude(State(state): State<AppState>) -> Response {
    let providers = match state.db.provider_list() {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for p in providers
        .iter()
        .filter(|p| p.enabled && p.kind == vibe_protocol::ProviderKind::Anthropic)
    {
        for alias in &p.model_aliases {
            if seen.insert(alias.alias.clone()) {
                data.push(serde_json::json!({
                    "id": alias.alias,
                    "display_name": alias.alias,
                    "type": "model",
                    "created_at": "2025-01-01T00:00:00Z"
                }));
            }
        }
    }
    data.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));

    let first = data.first().and_then(|m| m["id"].as_str()).map(String::from);
    let last  = data.last().and_then(|m| m["id"].as_str()).map(String::from);

    Json(serde_json::json!({
        "data": data,
        "has_more": false,
        "first_id": first,
        "last_id": last
    })).into_response()
}

/// `/codex/v1/models` 和 `/opencode/v1/models`
/// 仅 OpenAI-compat / OpenAI-Responses 供应商，OpenAI 格式
async fn list_models_openai(State(state): State<AppState>) -> Response {
    use vibe_protocol::ProviderKind;
    model_list_openai(
        &state,
        Some(&[ProviderKind::OpenaiChat, ProviderKind::OpenaiResponses]),
    ).await
}

async fn model_list_openai(
    state: &AppState,
    kinds: Option<&[vibe_protocol::ProviderKind]>,
) -> Response {
    let providers = match state.db.provider_list() {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for p in providers.iter().filter(|p| {
        p.enabled && kinds.map_or(true, |ks| ks.contains(&p.kind))
    }) {
        for alias in &p.model_aliases {
            if seen.insert(alias.alias.clone()) {
                data.push(serde_json::json!({
                    "id": alias.alias,
                    "slug": alias.alias,                    // Codex v0.129+
                    "display_name": alias.alias,            // Codex v0.129+
                    "supported_reasoning_levels": [],       // Codex v0.129+
                    "shell_type": "default",                // Codex v0.129+ (enum: default|local|unified_exec|disabled|shell_command)
                    "visibility": "list",                   // Codex v0.129+ (enum: list|hide|none)
                    "supported_in_api": true,               // Codex v0.129+
                    "priority": 0,                          // Codex v0.129+
                    "base_instructions": "",                // Codex v0.129+ (must be string, not null)
                    "supports_reasoning_summaries": false,  // Codex v0.129+
                    "support_verbosity": false,             // Codex v0.129+
                    // Align with codex-rs ModelFamily conservative defaults.
                    "truncation_policy": {"mode": "bytes", "limit": 10000},
                    "supports_parallel_tool_calls": false,
                    "experimental_supported_tools": [],
                    "object": "model",
                    "created": 0,
                    "owned_by": "vibe-plus"
                }));
            }
        }
    }
    data.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));

    // Codex v0.129+ expects a top-level "models" field that is an array of ModelInfo
    // objects (same structure as "data"), not a plain string array.
    Json(serde_json::json!({
        "object": "list",
        "data": data,
        "models": data           // Codex v0.129+ compatibility: same objects as data
    })).into_response()
}

// ---------------------------------------------------------------------------
// Model API handlers
// ---------------------------------------------------------------------------

async fn post_messages_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::Anthropic, None, headers, body, Some("plain-v1".into())).await
}

async fn post_messages_claude(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::Anthropic, None, headers, body, Some("claude-v1".into())).await
}

async fn post_chat_completions_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::OpenaiChat, None, headers, body, Some("plain-v1".into())).await
}

async fn post_chat_completions_opencode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::OpenaiChat, None, headers, body, Some("opencode-v1".into())).await
}

async fn post_responses_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::OpenaiResponses, None, headers, body, Some("plain-v1".into())).await
}

async fn post_responses_opencode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::OpenaiResponses, None, headers, body, Some("opencode-v1".into())).await
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

fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

/// Unified handler for /codex/v1/responses (and compact/double-prefix variants).
/// Accepts both WS upgrades and plain HTTP POST.
async fn codex_responses_handler(
    ws_upgrade: Option<WebSocketUpgrade>,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(upgrade) = ws_upgrade {
        upgrade.on_upgrade(move |socket| codex_ws_bridge(socket, state))
    } else {
        // Plain HTTP POST — Codex may send the WS-envelope format even over HTTP.
        // Strip the {"type":"response.create",...} envelope so forward() sees a
        // clean Responses API body with a top-level "model" field.
        let stripped = transforms::strip_ws_envelope(&body);
        let upstream = forward::forward(
            state.clone(),
            Wire::OpenaiResponses,
            None,
            headers,
            stripped,
            Some("codex-v1".into()),
        )
        .await;
        codex_plain_http_maybe_chat_to_responses_sse(state, upstream).await
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CodexHttpSseMode {
    Undecided,
    Passthrough,
    C2r,
}

/// Inspect one SSE frame (delimiter `\n\n`) and decide whether it looks like **Chat Completions** JSON.
///
/// Returns:
/// - `Some(true)`  — contains `choices` (typical upstream Chat SSE)
/// - `Some(false)` — contains structured JSON without `choices` (likely Responses-native)
/// - `None`        — heartbeat / comments / `[DONE]` / empty — stay undecided
fn classify_codex_upstream_sse_frame(block: &str) -> Option<bool> {
    let mut saw_data = false;
    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        saw_data = true;
        let d = payload.trim();
        if d.is_empty() || d == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(d) else {
            return Some(false);
        };
        if v.get("choices").is_some() {
            return Some(true);
        }
        return Some(false);
    }
    if saw_data {
        Some(false)
    } else {
        None
    }
}

/// Codex **`/codex/v1/responses` HTTP**：上游若是 Chat SSE，则在此处做 **SSE → Responses SSE**，并写入 `request_logs.client_response_body`。
async fn codex_plain_http_maybe_chat_to_responses_sse(state: AppState, upstream: Response) -> Response {
    let (parts, body) = upstream.into_parts();
    let log_row_id = parts
        .extensions
        .get::<forward::VibeLogId>()
        .map(|x| x.0.clone());

    if !parts.status.is_success() {
        return Response::from_parts(parts, body);
    }

    let content_type_hdr = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type_hdr.contains("event-stream") {
        return Response::from_parts(parts, body);
    }

    let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
    let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());

    let mut out_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        let n = name.as_str();
        if n.eq_ignore_ascii_case("content-length")
            || n.eq_ignore_ascii_case("transfer-encoding")
            || n.eq_ignore_ascii_case("content-type")
        {
            continue;
        }
        out_headers.insert(name.clone(), value.clone());
    }
    out_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));

    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(96);
    tokio::spawn(async move {
        let mut trace = String::new();
        let mut mode = CodexHttpSseMode::Undecided;

        #[inline]
        async fn emit_raw_frame(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            block: &str,
        ) -> bool {
            let mut chunk = block.to_owned();
            chunk.push_str("\n\n");
            tx.send(Ok(Bytes::from(chunk))).await.is_ok()
        }

        #[inline]
        async fn emit_c2r_frame(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            frame_json: &str,
        ) -> bool {
            append_codex_ws_client_trace(trace, frame_json);
            let sse_line = format!("data: {}\n\n", frame_json);
            tx.send(Ok(Bytes::from(sse_line))).await.is_ok()
        }

        #[inline]
        async fn flush_one_sse_block(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            mode: &mut CodexHttpSseMode,
            event_block: &str,
            session_id: &str,
            item_id: &str,
            accumulator: &mut transforms::ChatCompletionsC2rAccumulator,
            terminal_done: &mut bool,
        ) -> bool {
            loop {
                match *mode {
                    CodexHttpSseMode::Undecided => match classify_codex_upstream_sse_frame(event_block) {
                        Some(true) => {
                            *mode = CodexHttpSseMode::C2r;
                            continue;
                        }
                        Some(false) => {
                            *mode = CodexHttpSseMode::Passthrough;
                            continue;
                        }
                        None => {
                            return emit_raw_frame(tx, event_block).await;
                        }
                    },
                    CodexHttpSseMode::Passthrough => {
                        return emit_raw_frame(tx, event_block).await;
                    }
                    CodexHttpSseMode::C2r => {
                        for ws_frame in codex_sse_block_to_ws_frames(
                            event_block,
                            session_id,
                            item_id,
                            accumulator,
                            terminal_done,
                        ) {
                            if !emit_c2r_frame(tx, trace, &ws_frame).await {
                                return false;
                            }
                        }
                        return true;
                    }
                }
            }
        }

        let mut accumulator = transforms::ChatCompletionsC2rAccumulator::default();
        let mut terminal_done = false;

        let mut byte_stream = body.into_data_stream();
        let mut buf = String::new();
        let mut stream_broken = false;

        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => buf.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => {
                    stream_broken = true;
                    break;
                }
            }

            while let Some(end) = buf.find("\n\n") {
                let block = buf[..end].to_string();
                buf.drain(..end + 2);
                if !flush_one_sse_block(
                    &tx,
                    &mut trace,
                    &mut mode,
                    &block,
                    &session_id,
                    &item_id,
                    &mut accumulator,
                    &mut terminal_done,
                )
                .await
                {
                    drop(tx);
                    persist_codex_client_response_body(&state, log_row_id, trace).await;
                    return;
                }
            }
        }

        if !buf.trim().is_empty() {
            buf.push('\n');
            buf.push('\n');
            while let Some(end) = buf.find("\n\n") {
                let block = buf[..end].to_string();
                buf.drain(..end + 2);
                let _ = flush_one_sse_block(
                    &tx,
                    &mut trace,
                    &mut mode,
                    &block,
                    &session_id,
                    &item_id,
                    &mut accumulator,
                    &mut terminal_done,
                )
                .await;
            }
        }

        if mode == CodexHttpSseMode::C2r && !terminal_done {
            let detail = if stream_broken {
                "upstream SSE read error before a terminal chunk (no finish_reason / [DONE] seen)"
            } else {
                "upstream stream ended before a terminal chunk (no finish_reason / [DONE] seen)"
            };
            let payload = transforms::codex_response_proxy_fault_event(
                &session_id,
                "upstream_stream_truncated",
                detail,
            );
            append_codex_ws_client_trace(&mut trace, &payload);
            let sse_line = format!("data: {}\n\n", payload);
            let _ = tx.send(Ok(Bytes::from(sse_line))).await;
        }

        drop(tx);
        persist_codex_client_response_body(&state, log_row_id, trace).await;
    });

    let mut out = Response::new(Body::from_stream(ReceiverStream::new(rx)));
    *out.status_mut() = parts.status;
    *out.version_mut() = parts.version;
    *out.headers_mut() = out_headers;
    out
}

/// One SSE event block (text between `\n\n`) → Codex WebSocket text frames.
/// 累积整段转发给 Codex 的 JSON-Lines trace，供 `client_response_body` 全量落库（不做截断）。
fn append_codex_ws_client_trace(acc: &mut String, json_line: &str) {
    if !acc.is_empty() {
        acc.push('\n');
    }
    acc.push_str(json_line);
}

async fn persist_codex_client_response_body(state: &AppState, row_id: Option<String>, trace: String) {
    let Some(id) = row_id else {
        return;
    };
    if trace.is_empty() {
        return;
    }
    let db = state.db.clone();
    let id_for_warn = id.clone();
    let res = tokio::task::spawn_blocking(move || db.log_set_client_response_body(&id, Some(&trace))).await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(
            log_id = %id_for_warn,
            ?e,
            "failed to PATCH client_response_body"
        ),
        Err(j) => tracing::warn!(log_id = %id_for_warn, %j, "join error patching client_response_body"),
    }
}

fn codex_sse_block_to_ws_frames(
    event_block: &str,
    session_id: &str,
    item_id: &str,
    accumulator: &mut transforms::ChatCompletionsC2rAccumulator,
    terminal_done: &mut bool,
) -> Vec<String> {
    let mut frames = Vec::new();
    for raw_line in event_block.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" {
                continue;
            }
            if transforms::upstream_sse_data_is_terminal(data) {
                *terminal_done = true;
            }
            let events = if data.contains("\"choices\"") {
                transforms::chat_event_to_responses_events(
                    data,
                    session_id,
                    item_id,
                    accumulator,
                )
            } else {
                let skip_usage_tail = serde_json::from_str::<serde_json::Value>(data)
                    .map(|v| v.get("choices").is_none() && v.get("usage").is_some())
                    .unwrap_or(false);
                if skip_usage_tail {
                    vec![]
                } else {
                    vec![data.to_string()]
                }
            };
            frames.extend(events);
        }
    }
    frames
}

/// WebSocket bridge: receive request JSON → forward via HTTP → stream events back.
async fn codex_ws_bridge(mut socket: WebSocket, state: AppState) {
    // Codex keeps the WS connection alive across multiple turns (tool execution
    // cycles).  We loop here to handle each `response.create` message that
    // arrives on the same connection.
    loop {
        // 1. Wait for the next request message from Codex.
        let body_bytes: Bytes = loop {
            match socket.recv().await {
                Some(Ok(Message::Text(t)))   => break Bytes::from(t.into_bytes()),
                Some(Ok(Message::Binary(b))) => break Bytes::from(b),
                Some(Ok(Message::Close(_))) | None => return,
                Some(Ok(_)) => continue,   // ping/pong — ignore
                Some(Err(_)) => return,
            }
        };

        // 2. Strip the WS envelope: {"type":"response.create", ...} → {...}
        {
            let preview = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(300)]);
            tracing::debug!(preview = %preview, "codex ws body (first 300 bytes)");
        }
        let stripped = transforms::strip_ws_envelope(&body_bytes);

        // For WS mode, always request streaming from the upstream.
        let http_body: Bytes = {
            let mut val: serde_json::Value =
                serde_json::from_slice(&stripped).unwrap_or(serde_json::Value::Object(Default::default()));
            if let Some(obj) = val.as_object_mut() {
                obj.insert("stream".into(), serde_json::Value::Bool(true));
            }
            serde_json::to_vec(&val).map(Bytes::from).unwrap_or(stripped.clone())
        };
        {
            let model = serde_json::from_slice::<serde_json::Value>(&http_body)
                .ok()
                .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
                .unwrap_or_default();
            tracing::debug!(model = %model, "codex ws stripped body model");
        }

        // Per-response IDs.
        let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
        let item_id    = format!("msg-{}", uuid::Uuid::new_v4().simple());

        // 3. Build minimal headers for the forward call.
        let mut req_headers = HeaderMap::new();
        req_headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        // 4. Forward to upstream; get back an axum Response.
        let response = forward::forward(
            state.clone(),
            Wire::OpenaiResponses,
            None,
            req_headers,
            http_body,
            Some("codex-v1".into()),
        )
        .await;

        let (parts, body) = response.into_parts();
        let stream_log_row_id = parts
            .extensions
            .get::<forward::VibeLogId>()
            .map(|x| x.0.clone());
        let mut client_ws_trace = String::new();

        // 5. Non-2xx: emit a Responses-shaped `response.failed` frame so Codex CLI can
        //    surface ServerOverloaded / retry / quota — not plain text (which leaves the
        //    turn without a terminal event and triggers reconnect spam).
        if !parts.status.is_success() {
            let status_u16 = parts.status.as_u16();
            let detail = axum::body::to_bytes(body, 64 * 1024)
                .await
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let payload = transforms::codex_response_failed_event(&session_id, status_u16, &detail);
            append_codex_ws_client_trace(&mut client_ws_trace, &payload);
            let _ = socket.send(Message::Text(payload)).await;
            persist_codex_client_response_body(&state, stream_log_row_id, client_ws_trace).await;
            continue;
        }

        let content_type = parts
            .headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let is_sse  = content_type.contains("event-stream");

        if is_sse {
            // 6a. Streaming SSE: parse each event and emit as Responses API WS messages.
            use futures_util::StreamExt as _;
            let mut stream = body.into_data_stream();
            let mut buf = String::new();
            let mut accumulator = transforms::ChatCompletionsC2rAccumulator::default();
            let mut terminal_done = false;
            let mut stream_broken = false;

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        // Never drop bytes on a non-UTF8 chunk boundary (reqwest can split codepoints).
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Err(_) => {
                        stream_broken = true;
                        break;
                    }
                }

                // Consume complete SSE events (terminated by blank line).
                while let Some(event_end) = buf.find("\n\n") {
                    let event_block = buf[..event_end].to_string();
                    buf.drain(..event_end + 2);
                    for event_str in codex_sse_block_to_ws_frames(
                        &event_block,
                        &session_id,
                        &item_id,
                        &mut accumulator,
                        &mut terminal_done,
                    ) {
                        tracing::debug!(event = %&event_str[..event_str.len().min(200)], "codex ws → client event");
                        append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                        if socket.send(Message::Text(event_str)).await.is_err() {
                            return; // client disconnected
                        }
                    }
                }
            }

            // Some upstreams end the body without a final blank line after the last `data:`.
            if !buf.trim().is_empty() {
                buf.push('\n');
                buf.push('\n');
                while let Some(event_end) = buf.find("\n\n") {
                    let event_block = buf[..event_end].to_string();
                    buf.drain(..event_end + 2);
                    for event_str in codex_sse_block_to_ws_frames(
                        &event_block,
                        &session_id,
                        &item_id,
                        &mut accumulator,
                        &mut terminal_done,
                    ) {
                        tracing::debug!(event = %&event_str[..event_str.len().min(200)], "codex ws flush → client event");
                        append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                        if socket.send(Message::Text(event_str)).await.is_err() {
                            return;
                        }
                    }
                }
            }

            if !terminal_done {
                let detail = if stream_broken {
                    "upstream SSE read error before a terminal chunk (no finish_reason / [DONE] seen)"
                } else {
                    "upstream stream ended before a terminal chunk (no finish_reason / [DONE] seen)"
                };
                let payload = transforms::codex_response_proxy_fault_event(
                    &session_id,
                    "upstream_stream_truncated",
                    detail,
                );
                append_codex_ws_client_trace(&mut client_ws_trace, &payload);
                if socket.send(Message::Text(payload)).await.is_err() {
                    return;
                }
            }
        } else {
            // 6b. 非 SSE：上游仍可能返回完整 Chat JSON。Codex WS 只认带 `type` 的事件序列，
            //    不能直接发裸 `response` 对象（见 transforms::chat_completion_non_stream_to_ws_events）。
            if let Ok(bytes) = axum::body::to_bytes(body, 8 * 1024 * 1024).await {
                if bytes.windows(9).any(|w| w == b"\"choices\"") {
                    match transforms::chat_completion_non_stream_to_ws_events(
                        &bytes,
                        &session_id,
                        &item_id,
                    ) {
                        Ok(frames) => {
                            for event_str in frames {
                                tracing::debug!(
                                    event = %&event_str[..event_str.len().min(200)],
                                    "codex ws non-sse → client event"
                                );
                                append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                                if socket.send(Message::Text(event_str)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        Err(()) => {
                            let payload = transforms::codex_response_proxy_fault_event(
                                &session_id,
                                "upstream_invalid_chat_completion_json",
                                "upstream returned a non-stream body that is not valid Chat Completions JSON",
                            );
                            if socket.send(Message::Text(payload)).await.is_err() {
                                return;
                            }
                        }
                    }
                } else {
                    let detail = String::from_utf8_lossy(&bytes).into_owned();
                    let payload = transforms::codex_response_proxy_fault_event(
                        &session_id,
                        "upstream_body_not_chat_completion",
                        &format!("upstream body did not look like Chat Completions JSON: {detail}"),
                    );
                    append_codex_ws_client_trace(&mut client_ws_trace, &payload);
                    if socket.send(Message::Text(payload)).await.is_err() {
                        return;
                    }
                }
            }
        }
        persist_codex_client_response_body(&state, stream_log_row_id, client_ws_trace).await;
        // Loop back to wait for the next response.create on this same connection.
    }
}

/// Plain-HTTP wrapper for /codex/v1/chat/completions (no WS needed).
async fn post_or_reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if is_websocket_upgrade(&headers) {
        return (StatusCode::NOT_IMPLEMENTED, "WebSocket not supported on chat/completions").into_response();
    }
    forward::forward(state, Wire::OpenaiChat, None, headers, body, Some("codex-v1".into())).await
}

async fn post_gemini(
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

// ---------------------------------------------------------------------------
// Local import (scan installed tools → ready-to-use ProviderInput)
// ---------------------------------------------------------------------------

/// `GET /_vp/providers/import-local`
/// 扫描本地已安装的 Claude Code / Codex CLI，返回可导入候选列表。
/// 不写库，只读文件系统。
async fn scan_local_providers() -> Json<Vec<local_import::LocalCandidate>> {
    Json(local_import::scan())
}

/// `POST /_vp/providers/import-local`
/// body: `["claude", "codex"]`  — 指定要导入的 client 名称列表
///
/// 对每个候选：
///   1. 若已有相同 kind + base_url 的 provider → 跳过（幂等）
///   2. 插入 provider（Codex 不再写入 `codex-auth` auth_ref）
///   3. 插入 credentials（Codex：每个 auth*.json → 一行 oauth_* 入库）
async fn import_local_providers(
    State(state): State<AppState>,
    Json(clients): Json<Vec<String>>,
) -> Result<Json<Vec<Provider>>, AppError> {
    let candidates = local_import::scan();
    let mut created = Vec::new();
    for c in candidates.iter().filter(|c| clients.contains(&c.client)) {
        let plan = local_import::candidate_to_plan(c)?;
        let kind = plan.provider.kind;
        let base = plan.provider.base_url.clone();
        let dup = run_blocking(state.clone(), {
            let base = base.clone();
            move |s| s.db.provider_find_by_kind_and_base_url(kind, &base)
        })
        .await?;
        if dup.is_some() {
            tracing::info!(%base, ?kind, "import-local: skipped duplicate provider");
            continue;
        }
        let credentials = plan.credentials;
        let provider_input = plan.provider;
        let p = run_blocking(state.clone(), move |s| s.db.provider_insert(provider_input)).await?;
        let pid = p.id.clone();
        for cred in credentials {
            let pid2 = pid.clone();
            let fp = crate::auth_fingerprint::credential_fingerprint(
                cred.auth_ref.as_deref(),
                cred.oauth_access_token.as_deref(),
            );
            run_blocking(state.clone(), move |s| s.db.credential_insert(&pid2, cred, Some(fp))).await?;
        }
        created.push(p);
    }
    Ok(Json(created))
}

// ---------------------------------------------------------------------------
// Provider CRUD
// ---------------------------------------------------------------------------

async fn list_providers(State(state): State<AppState>) -> Result<Json<Vec<Provider>>, AppError> {
    let v = run_blocking(state, |s| s.db.provider_list()).await?;
    Ok(Json(v))
}

async fn create_provider(
    State(state): State<AppState>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let p = run_blocking(state.clone(), move |s| s.db.provider_insert(input)).await?;
    state.ws.publish(WsEvent::Hello { version: VERSION.into() });
    Ok(Json(p))
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let id_for_update = id.clone();
    let p = run_blocking(state.clone(), move |s| s.db.provider_update(&id_for_update, input)).await?;
    // 绑定“开关”与熔断：切换后清空该 provider 及其 credential 的熔断状态，
    // 避免 UI 显示 enabled 但请求仍被历史熔断阻断。
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;
    state.cb.reset(&id);
    for cid in cred_ids {
        state.cb.reset(&cid);
    }
    Ok(Json(p))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    run_blocking(state, move |s| s.db.provider_delete(&id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Provider health
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RollingHoursQuery {
    #[serde(default = "default_rolling_hours")]
    hours: i64,
}

fn default_rolling_hours() -> i64 {
    24
}

fn cb_state_rank(state: CbState) -> i32 {
    match state {
        CbState::Open => 3,
        CbState::HalfOpen => 2,
        CbState::Closed => 1,
    }
}

fn effective_circuit_for_provider(
    state: &AppState,
    provider_id: &str,
    credential_ids: &[String],
) -> (String, i32, bool) {
    let mut worst = state.cb.state_of(provider_id);
    let mut max_failures = state.cb.consecutive_failures(provider_id) as i32;

    for cid in credential_ids {
        let s = state.cb.state_of(cid);
        if cb_state_rank(s) > cb_state_rank(worst) {
            worst = s;
        }
        let cf = state.cb.consecutive_failures(cid) as i32;
        if cf > max_failures {
            max_failures = cf;
        }
    }

    let is_healthy = worst != CbState::Open;
    (worst.as_str().to_string(), max_failures, is_healthy)
}

async fn provider_health(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProviderHealthSummary>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;

    let db_row = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let row = s.db.health_get(&id2)?;
            Ok(row)
        }
    })
    .await?
    .unwrap_or_else(|| {
        vibe_db::DbHealth {
            provider_id: id.clone(),
            is_healthy: true,
            consecutive_failures: 0,
            total_requests: 0,
            total_successes: 0,
            total_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            last_error: None,
            avg_latency_ms: None,
            updated_at: 0,
        }
    });
    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(&state, &id, &cred_ids);

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    let cumulative = ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy,
        circuit_state,
        consecutive_failures,
        total_requests: db_row.total_requests,
        total_successes: db_row.total_successes,
        total_failures: db_row.total_failures,
        success_rate,
        last_success_at: db_row.last_success_at,
        last_failure_at: db_row.last_failure_at,
        last_error: db_row.last_error,
        avg_latency_ms: db_row.avg_latency_ms,
        updated_at: db_row.updated_at,
    };

    let rolling = run_blocking(state.clone(), move |s| s.db.provider_stat_single(&id, hours)).await?;

    Ok(Json(ProviderHealthSummary {
        cumulative,
        rolling_hours: hours,
        rolling,
    }))
}

async fn provider_circuit_reset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProviderHealth>, AppError> {
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;
    state.cb.reset(&id);
    for cid in &cred_ids {
        state.cb.reset(cid);
    }
    let db_row = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| s.db.health_get(&id2)
    })
    .await?
    .unwrap_or_else(|| vibe_db::DbHealth {
        provider_id: id.clone(),
        is_healthy: true,
        consecutive_failures: 0,
        total_requests: 0,
        total_successes: 0,
        total_failures: 0,
        last_success_at: None,
        last_failure_at: None,
        last_error: None,
        avg_latency_ms: None,
        updated_at: 0,
    });

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(&state, &id, &cred_ids);
    Ok(Json(ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy,
        circuit_state,
        consecutive_failures,
        total_requests: db_row.total_requests,
        total_successes: db_row.total_successes,
        total_failures: db_row.total_failures,
        success_rate,
        last_success_at: db_row.last_success_at,
        last_failure_at: db_row.last_failure_at,
        last_error: db_row.last_error,
        avg_latency_ms: db_row.avg_latency_ms,
        updated_at: db_row.updated_at,
    }))
}

async fn health_all_providers(
    State(state): State<AppState>,
) -> Result<Json<HealthSummary>, AppError> {
    let rows = run_blocking(state.clone(), |s| s.db.health_list()).await?;
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let creds_all = run_blocking(state.clone(), |s| s.db.credential_list_all()).await?;
    let mut cred_ids_by_provider: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for c in creds_all {
        cred_ids_by_provider
            .entry(c.provider_id)
            .or_default()
            .push(c.id);
    }

    // Build health entries for every known provider (even those never hit)
    let mut health_map: std::collections::HashMap<String, vibe_db::DbHealth> =
        rows.into_iter().map(|r| (r.provider_id.clone(), r)).collect();

    for p in &providers {
        health_map.entry(p.id.clone()).or_insert_with(|| vibe_db::DbHealth {
            provider_id: p.id.clone(),
            is_healthy: true,
            consecutive_failures: 0,
            total_requests: 0,
            total_successes: 0,
            total_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            last_error: None,
            avg_latency_ms: None,
            updated_at: 0,
        });
    }

    let all: Vec<ProviderHealth> = health_map
        .into_values()
        .map(|row| {
            let cred_ids = cred_ids_by_provider
                .get(&row.provider_id)
                .cloned()
                .unwrap_or_default();
            let (cs, cf, is_healthy) =
                effective_circuit_for_provider(&state, &row.provider_id, &cred_ids);
            let success_rate = if row.total_requests > 0 {
                row.total_successes as f64 / row.total_requests as f64
            } else {
                1.0
            };
            ProviderHealth {
                provider_id: row.provider_id,
                is_healthy,
                circuit_state: cs,
                consecutive_failures: cf,
                total_requests: row.total_requests,
                total_successes: row.total_successes,
                total_failures: row.total_failures,
                success_rate,
                last_success_at: row.last_success_at,
                last_failure_at: row.last_failure_at,
                last_error: row.last_error,
                avg_latency_ms: row.avg_latency_ms,
                updated_at: row.updated_at,
            }
        })
        .collect();

    let healthy_providers = all.iter().filter(|h| h.is_healthy).count();
    let total_providers = all.len();

    Ok(Json(HealthSummary {
        providers: all,
        total_providers,
        healthy_providers,
    }))
}

// ---------------------------------------------------------------------------
// Credential CRUD
// ---------------------------------------------------------------------------

async fn list_credentials(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<Credential>>, AppError> {
    let v = run_blocking(state, move |s| s.db.credential_list_for_provider(&provider_id)).await?;
    Ok(Json(v))
}

async fn create_credential(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<Credential>, AppError> {
    let fp = crate::auth_fingerprint::credential_fingerprint(
        input.auth_ref.as_deref(),
        input.oauth_access_token.as_deref(),
    );
    let c =
        run_blocking(state, move |s| s.db.credential_insert(&provider_id, input, Some(fp))).await?;
    Ok(Json(c))
}

async fn get_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    let c = run_blocking(state, move |s| s.db.credential_get(&id)).await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    Ok(Json(c))
}

async fn update_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<Credential>, AppError> {
    let fp = crate::auth_fingerprint::credential_fingerprint(
        input.auth_ref.as_deref(),
        input.oauth_access_token.as_deref(),
    );
    let c = run_blocking(state, move |s| s.db.credential_update(&id, input, Some(fp))).await?;
    Ok(Json(c))
}

async fn credential_plan_latest(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<CredentialPlanSnapshot>>, AppError> {
    let snap = run_blocking(state, move |s| s.db.plan_snapshot_latest(&id)).await?;
    Ok(Json(snap))
}

async fn refresh_codex_plan_for_credential(state: &AppState, cred: &Credential) -> anyhow::Result<()> {
    let Some(access) = cred.oauth_access_token.as_ref().filter(|t| !t.is_empty()) else {
        anyhow::bail!("credential has no OAuth access token");
    };
    let oauth = crate::forward::CredOAuth {
        access_token: access.clone(),
        expires_at: cred.oauth_expires_at,
    };
    let token = crate::forward::resolve_oauth_token(state, Some(cred.id.as_str()), oauth).await?;
    let acct = crate::auth_fingerprint::chatgpt_account_id_from_access_token(&token);
    let snap = crate::codex_wham_usage::fetch_wham_plan_snapshot(
        &state.http,
        &token,
        acct.as_deref(),
        &cred.id,
    )
    .await?;
    let db = state.db.clone();
    let snap_ins = snap.clone();
    tokio::task::spawn_blocking(move || db.plan_snapshot_insert(&snap_ins))
        .await??;
    Ok(())
}

async fn credential_plan_refresh(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CredentialPlanSnapshot>, AppError> {
    let cred_opt = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.credential_get(&id)
    })
    .await?;
    let cred = cred_opt.ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let pid = cred.provider_id.clone();
    let provider_opt = run_blocking(state.clone(), move |s| s.db.provider_get(&pid)).await?;
    let provider = provider_opt.ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    if !crate::router::provider_is_chatgpt_codex_official(&provider) {
        return Err(anyhow::anyhow!(
            "Not a ChatGPT Codex official provider (chatgpt.com … /backend-api/…/codex)."
        )
        .into());
    }
    refresh_codex_plan_for_credential(&state, &cred).await?;
    let snap_opt = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.plan_snapshot_latest(&id)
    })
    .await?;
    let snap = snap_opt.ok_or_else(|| anyhow::anyhow!("plan snapshot missing after refresh"))?;
    Ok(Json(snap))
}

async fn provider_codex_plan_list(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<ProviderCodexPlanItem>>, AppError> {
    let pid = provider_id.clone();
    let items = run_blocking(state.clone(), move |s| {
        let p = s
            .db
            .provider_get(&pid)?
            .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if !crate::router::provider_is_chatgpt_codex_official(&p) {
            return Ok(Vec::new());
        }
        let creds = s.db.credential_list_for_provider(&pid)?;
        let mut out = Vec::new();
        for c in creds {
            let plan = s.db.plan_snapshot_latest(&c.id)?;
            out.push(ProviderCodexPlanItem {
                credential_id: c.id,
                label: c.label,
                plan,
            });
        }
        Ok(out)
    })
    .await?;
    Ok(Json(items))
}

async fn provider_codex_plan_refresh_all(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<CodexPlanRefreshResult>, AppError> {
    let creds = run_blocking(state.clone(), move |s| {
        let p = s
            .db
            .provider_get(&provider_id)?
            .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if !crate::router::provider_is_chatgpt_codex_official(&p) {
            anyhow::bail!("not a ChatGPT Codex official provider");
        }
        s.db.credential_list_for_provider(&provider_id)
    })
    .await?;

    let mut attempted = 0usize;
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for c in creds {
        if c.oauth_access_token.as_ref().map_or(true, |t: &String| t.is_empty()) {
            continue;
        }
        attempted += 1;
        match refresh_codex_plan_for_credential(&state, &c).await {
            Err(e) => errors.push(format!("{}: {e}", c.label)),
            Ok(()) => ok += 1,
        }
    }

    Ok(Json(CodexPlanRefreshResult {
        attempted,
        ok,
        errors,
    }))
}

async fn delete_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    run_blocking(state, move |s| s.db.credential_delete(&id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LogQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since: Option<i64>,
    provider_id: Option<String>,
    /// "ok" | "error"
    status: Option<String>,
}

async fn get_request_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let log = run_blocking(state, move |s| s.db.log_get(&id)).await?;
    Ok(match log {
        Some(log) => Json(log).into_response(),
        None => (StatusCode::NOT_FOUND, "log not found").into_response(),
    })
}

async fn list_logs(
    State(state): State<AppState>,
    Query(q): Query<LogQuery>,
) -> Result<Json<LogPage>, AppError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let status_ok: Option<bool> = match q.status.as_deref() {
        Some("ok") => Some(true),
        Some("error") => Some(false),
        _ => None,
    };
    let p = run_blocking(state, move |s| {
        s.db.log_list_filtered(limit, offset, q.since, q.provider_id.as_deref(), status_ok)
    })
    .await?;
    Ok(Json(p))
}

// ---------------------------------------------------------------------------
// Usage / stats
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UsageQuery {
    hours: Option<i64>,
}

async fn usage_summary(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageSummary>, AppError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 30);
    let s = run_blocking(state, move |s| s.db.usage_summary_last_hours(hours)).await?;
    Ok(Json(s))
}

async fn dashboard_stats(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<DashboardStats>, AppError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 30);
    let stats = run_blocking(state, move |s| {
        let now = chrono::Utc::now().timestamp();
        let since_window = now - hours * 3600;
        let since_1h = now - 3600;

        let requests_last_hour = s.db.count_logs_since(since_1h)?;
        let requests_last_24h = s.db.count_logs_since(now - 86400)?;

        let (ok_window, total_window) = s.db.ok_total_since(since_window)?;
        let (ok_1h, total_1h) = s.db.ok_total_since(since_1h)?;
        let success_rate_in_window = if total_window == 0 {
            1.0
        } else {
            ok_window as f64 / total_window as f64
        };
        let success_rate_last_hour = if total_1h == 0 {
            1.0
        } else {
            ok_1h as f64 / total_1h as f64
        };

        let (p50, p95) = s.db.latency_percentiles(hours)?;
        let top_models = s.db.top_models(hours, 10)?;
        let per_provider = s.db.per_provider_stats(hours)?;
        let summary_window = s.db.usage_summary_last_hours(hours)?;
        let summary_24h = s.db.usage_summary_last_hours(24)?;

        let window_label = match hours {
            1 => "Last 1 hour".to_string(),
            5 => "Last 5 hours".to_string(),
            24 => "Last 24 hours".to_string(),
            168 => "Last 7 days".to_string(),
            720 => "Last 30 days".to_string(),
            h if h % 24 == 0 && h > 24 => format!("Last {} days", h / 24),
            h => format!("Last {h} hours"),
        };

        Ok(vibe_protocol::DashboardStats {
            window_hours: hours,
            window_label,
            requests_in_window: summary_window.requests,
            success_rate_in_window,
            input_tokens_in_window: summary_window.input_tokens,
            output_tokens_in_window: summary_window.output_tokens,
            requests_last_hour,
            requests_last_24h,
            success_rate_last_hour,
            avg_latency_ms: p50,
            p95_latency_ms: p95,
            input_tokens_last_24h: summary_24h.input_tokens,
            output_tokens_last_24h: summary_24h.output_tokens,
            top_models,
            per_provider,
        })
    })
    .await?;
    Ok(Json(stats))
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| ws_session(socket, state))
}

async fn ws_session(socket: WebSocket, state: AppState) {
    let (mut tx, mut rx) = socket.split();
    let mut sub = state.ws.subscribe();
    let hello = WsEvent::Hello { version: VERSION.into() };
    if let Ok(j) = serde_json::to_string(&hello) {
        let _ = tx.send(Message::Text(j)).await;
    }
    loop {
        tokio::select! {
            ev = sub.recv() => {
                let Ok(ev) = ev else { break };
                let Ok(j) = serde_json::to_string(&ev) else { continue };
                if tx.send(Message::Text(j)).await.is_err() { break; }
            }
            incoming = rx.next() => {
                match incoming {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Close(_))) => break,
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Error & blocking helpers
// ---------------------------------------------------------------------------

pub struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self.0, "request error");
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

async fn run_blocking<F, R>(state: AppState, f: F) -> anyhow::Result<R>
where
    F: FnOnce(&AppState) -> anyhow::Result<R> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || f(&state)).await?
}
