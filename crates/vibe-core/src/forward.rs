//! Request forwarding with retry loop, circuit-breaker integration, and streaming.
//!
//! Strategy:
//! 1. Build an ordered candidate list via `router::candidates`.
//! 2. For each candidate, check the circuit breaker — skip if Open.
//! 3. Optionally inject Anthropic cache_control into the body.
//! 4. Send the request.
//!    - Connection error / 5xx / 429 → record failure, try next provider.
//!    - 4xx                          → record failure, return immediately (caller's fault).
//!    - 2xx                          → record success, stream or buffer response.
//! 5. If every candidate is exhausted, return 503.

use crate::cache;
use crate::circuit_breaker::CircuitBreakers;
use crate::providers::{self, Adapter, Wire};
use crate::state::AppState;
use crate::usage::Usage;
use crate::{router, secrets};
use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use vibe_protocol::{ProviderKind, RequestLog, WsEvent};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn forward(
    state: AppState,
    wire: Wire,
    upstream_path: Option<String>,
    req_headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started_at = chrono::Utc::now().timestamp();
    let started_instant = Instant::now();
    let log_id = uuid::Uuid::new_v4().to_string();
    let app = detect_app(&req_headers);
    // For GeminiNative the model is in the URL path, not the body.
    let requested_model = if wire == Wire::GeminiNative {
        upstream_path
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .and_then(|s| s.split(':').next())
            .unwrap_or("")
            .to_string()
    } else {
        extract_model(&body).unwrap_or_default()
    };

    let providers_list = match state.db.provider_list() {
        Ok(v) => v,
        Err(e) => return internal_error(format!("db error: {e}")),
    };

    let counter = state.lb_counter.fetch_add(1, Ordering::Relaxed);
    let candidates = rotate_candidates(
        router::candidates(&providers_list, wire, &requested_model),
        counter,
        &state.cb,
    );
    if candidates.is_empty() {
        let log = build_log(
            &log_id, started_at, &started_instant, &app,
            None, &requested_model, "",
            Some(503), Some("no provider matches request shape".into()), Usage::default(),
        );
        persist_log(&state, log);
        return (StatusCode::SERVICE_UNAVAILABLE, "no provider matches request shape")
            .into_response();
    }

    let mut last_error = String::from("all providers unavailable or circuit open");

    for pick in candidates {
        let provider = pick.provider;
        let upstream_model = pick.upstream_model;

        // ── circuit breaker ──────────────────────────────────────────────
        if !state.cb.allow(&provider.id) {
            tracing::debug!(provider_id = %provider.id, "circuit open, skipping");
            continue;
        }

        // ── auth ─────────────────────────────────────────────────────────
        let secret = match provider.auth_ref.as_deref() {
            Some(r) => match secrets::resolve(r) {
                Ok(s) => Some(s),
                Err(e) => {
                    last_error = format!("auth error for {}: {e}", provider.id);
                    continue;
                }
            },
            None => None,
        };

        // ── cache injection (Anthropic only) ─────────────────────────────
        let effective_body: Bytes = if provider.kind == ProviderKind::Anthropic
            && state.config.failover.inject_cache
        {
            cache::inject(&body)
        } else {
            body.clone()
        };

        // ── model rewrite + adapter ───────────────────────────────────────
        let adapter = providers::select(&provider);
        let body_up = match adapter.rewrite_body_model(&effective_body, &upstream_model) {
            Ok(b) => b,
            Err(e) => {
                last_error = format!("body rewrite: {e}");
                continue;
            }
        };

        // ── build request ─────────────────────────────────────────────────
        let req = match adapter.build(&provider, secret.as_deref(), &state.http, wire, &body_up, upstream_path.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                last_error = e.to_string();
                continue;
            }
        };

        // ── send ──────────────────────────────────────────────────────────
        let upstream = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                tracing::warn!(provider_id = %provider.id, error = %msg, "upstream connection error");
                state.cb.record_failure(&provider.id);
                fire_health(&state, &provider.id, false,
                    started_instant.elapsed().as_millis() as i64, Some(msg.clone()));
                last_error = format!("connection to {}: {msg}", provider.id);
                continue;
            }
        };

        let status = upstream.status();

        // ── retryable errors ──────────────────────────────────────────────
        // 429: rate-limited on this provider → try next
        // 5xx: server fault → try next
        // 401: auth invalid for this provider key → try next with another provider's key
        // 402: subscription/payment issue → try next provider
        if status.is_server_error()
            || status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::UNAUTHORIZED
            || status == StatusCode::PAYMENT_REQUIRED
        {
            let msg = format!("upstream {} from {}", status, provider.id);
            tracing::warn!(%msg, "retryable upstream error, trying next provider");
            state.cb.record_failure(&provider.id);
            fire_health(&state, &provider.id, false,
                started_instant.elapsed().as_millis() as i64, Some(format!("HTTP {status}")));
            last_error = msg;
            continue;
        }

        // ── non-retryable client error (400/403/404 etc.) ────────────────
        // These indicate a malformed request or forbidden resource — not a
        // provider-specific issue, so retrying another provider won't help.
        if status.is_client_error() {
            state.cb.record_failure(&provider.id);
            fire_health(&state, &provider.id, false,
                started_instant.elapsed().as_millis() as i64,
                Some(format!("client error {status}")));
            let resp_headers = copy_response_headers(upstream.headers());
            let buf = upstream.bytes().await.unwrap_or_default();
            let log = build_log(
                &log_id, started_at, &started_instant, &app,
                Some(&provider.id), &requested_model, &upstream_model,
                Some(status.as_u16() as i32),
                Some(format!("client error {status}")),
                Usage::default(),
            );
            persist_log(&state, log);
            return (status, resp_headers, buf).into_response();
        }

        // ── 2xx success ───────────────────────────────────────────────────
        state.cb.record_success(&provider.id);
        fire_health(&state, &provider.id, true,
            started_instant.elapsed().as_millis() as i64, None);

        let resp_headers = copy_response_headers(upstream.headers());

        if body_wants_stream(&body) {
            return stream_response(
                state, adapter, wire, upstream, status, resp_headers,
                log_id, started_at, started_instant, app,
                provider.id, requested_model, upstream_model,
            );
        }

        let buf = match upstream.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let log = build_log(
                    &log_id, started_at, &started_instant, &app,
                    Some(&provider.id), &requested_model, &upstream_model,
                    None, Some(format!("read upstream: {e}")), Usage::default(),
                );
                persist_log(&state, log);
                return (StatusCode::BAD_GATEWAY, format!("read upstream: {e}")).into_response();
            }
        };

        let usage = adapter.parse_usage_body(wire, &buf);
        let log = build_log(
            &log_id, started_at, &started_instant, &app,
            Some(&provider.id), &requested_model, &upstream_model,
            Some(status.as_u16() as i32), None, usage,
        );
        persist_log(&state, log);
        return (status, resp_headers, buf).into_response();
    }

    // All candidates exhausted
    let log = build_log(
        &log_id, started_at, &started_instant, &app,
        None, &requested_model, "",
        Some(503), Some(last_error.clone()), Usage::default(),
    );
    persist_log(&state, log);
    (StatusCode::SERVICE_UNAVAILABLE, last_error).into_response()
}

// ---------------------------------------------------------------------------
// Streaming path
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn stream_response(
    state: AppState,
    adapter: Box<dyn Adapter + Send + Sync>,
    wire: Wire,
    upstream: reqwest::Response,
    status: StatusCode,
    resp_headers: HeaderMap,
    log_id: String,
    started_at: i64,
    started_instant: Instant,
    app: Option<String>,
    provider_id: String,
    requested_model: String,
    upstream_model: String,
) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    tokio::spawn(async move {
        let mut byte_stream = upstream.bytes_stream();
        let mut acc = Usage::default();
        let mut first_token_ms: Option<i64> = None;
        let mut buf = String::new();

        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if first_token_ms.is_none() {
                        first_token_ms = Some(started_instant.elapsed().as_millis() as i64);
                    }
                    if let Ok(s) = std::str::from_utf8(&bytes) {
                        buf.push_str(s);
                        while let Some(pos) = buf.find("\n\n") {
                            let event = buf[..pos].to_string();
                            buf.drain(..pos + 2);
                            for line in event.lines() {
                                adapter.parse_usage_stream_event(wire, line, &mut acc);
                            }
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    break;
                }
            }
        }

        let mut log = build_log(
            &log_id, started_at, &started_instant, &app,
            Some(&provider_id), &requested_model, &upstream_model,
            Some(status.as_u16() as i32), None, acc,
        );
        log.first_token_ms = first_token_ms;
        persist_log(&state, log);
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    (status, resp_headers, body).into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Within the first priority group that contains at least one available
/// (non-circuit-open) provider, rotate by `counter` for round-robin.
/// This skips entirely-blocked priority groups so that dead providers at a
/// higher priority don't prevent distribution across healthy lower-priority peers.
fn rotate_candidates(
    mut candidates: Vec<router::Pick>,
    counter: usize,
    cb: &CircuitBreakers,
) -> Vec<router::Pick> {
    if candidates.len() <= 1 {
        return candidates;
    }

    // Walk priority groups from highest to lowest, find the first group
    // that has at least one provider the circuit breaker is not blocking.
    let mut group_start = 0;
    while group_start < candidates.len() {
        let priority = candidates[group_start].provider.priority;
        let group_end = group_start
            + candidates[group_start..]
                .partition_point(|p| p.provider.priority == priority);

        let has_available = candidates[group_start..group_end]
            .iter()
            .any(|p| !cb.is_blocking(&p.provider.id));

        if has_available && group_end - group_start > 1 {
            let offset = counter % (group_end - group_start);
            candidates[group_start..group_end].rotate_left(offset);
            break;
        }
        if has_available {
            break; // single-provider group — no rotation needed
        }
        group_start = group_end; // all blocked, try next priority group
    }

    candidates
}

fn body_wants_stream(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream")?.as_bool())
        .unwrap_or(false)
}

fn extract_model(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("model")?.as_str().map(str::to_string)
}

fn detect_app(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn copy_response_headers(src: &reqwest::header::HeaderMap) -> HeaderMap {
    let mut dst = HeaderMap::new();
    for (k, v) in src {
        if is_hop_header(k.as_str()) {
            continue;
        }
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_str().as_bytes()),
            HeaderValue::from_bytes(v.as_bytes()),
        ) {
            dst.insert(name, val);
        }
    }
    dst
}

fn is_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}

#[allow(clippy::too_many_arguments)]
fn build_log(
    id: &str,
    started_at: i64,
    started_instant: &Instant,
    app: &Option<String>,
    provider_id: Option<&str>,
    requested_model: &str,
    upstream_model: &str,
    status_code: Option<i32>,
    error: Option<String>,
    usage: Usage,
) -> RequestLog {
    RequestLog {
        id: id.to_string(),
        started_at,
        app: app.clone(),
        provider_id: provider_id.map(str::to_string),
        requested_model: Some(requested_model.to_string()),
        upstream_model: Some(upstream_model.to_string()),
        status_code,
        error,
        latency_ms: Some(started_instant.elapsed().as_millis() as i64),
        first_token_ms: None,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        estimated_cost_usd: usage.estimated_cost_usd(upstream_model),
    }
}

fn persist_log(state: &AppState, log: RequestLog) {
    let db = state.db.clone();
    let ws = state.ws.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = db.log_insert(&log) {
            tracing::warn!(?e, "failed to insert request log");
        }
        ws.publish(WsEvent::LogAppended(log));
    });
}

fn fire_health(
    state: &AppState,
    provider_id: &str,
    success: bool,
    latency_ms: i64,
    error: Option<String>,
) {
    let db = state.db.clone();
    let pid = provider_id.to_string();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = db.health_upsert(&pid, success, Some(latency_ms), error.as_deref()) {
            tracing::warn!(?e, provider_id = %pid, "failed to upsert provider health");
        }
    });
}

fn internal_error(msg: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
}
