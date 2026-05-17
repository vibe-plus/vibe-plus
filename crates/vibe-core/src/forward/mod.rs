//! Request forwarding with retry loop, circuit-breaker integration, and streaming.
//!
//! Strategy:
//! 1. Build a matching candidate list via `router::candidates`, then randomize it.
//! 2. Drop providers that have neither enabled credentials nor provider-level auth.
//! 3. For each candidate, check the circuit breaker — skip if Open.
//! 4. Optionally inject Anthropic cache_control into the body.
//! 5. Send the request.
//!    - Connection error / 5xx / 401 / 402 → record failure, try next provider.
//!    - 429 (quota / rate-limit) → rotate credential; **do not** trip circuit breaker.
//!    - 401                       → force-open CB cooldown; credential is NOT permanently
//!                                  disabled (use UI disable or wait for CB recovery).
//!    - OpenAI Responses 404      → treat as upstream route/model mismatch; release sticky route and try next provider.
//!    - Other 4xx                  → return immediately (caller's fault); no breaker trip.
//!    - 2xx                        → record success, stream or buffer response.
//! 6. If every candidate is exhausted, return 503.
//!
//! Sub-modules:
//! - `selector` — pure candidate selection & expansion logic (testable without AppState)
//! - `outcome`  — pure HTTP status → retry-decision mapping (testable in isolation)

pub mod outcome;
pub mod race;
pub mod selector;

use crate::cache;
use crate::claude_summary::ClaudeClientKind;
use crate::codex_summary::CodexClientKind;
use crate::codex_visual::{self, CodexVisualContext};
use crate::providers::{self, Adapter, Wire};
use crate::state::{AppState, CodexStickyRoute};
use crate::stream_trace::{empty_stream_fields, StreamTraceStats};
use crate::transforms;
use crate::usage::Usage;
use crate::{router, secrets};
use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use vibe_protocol::{
    AppLogEvent, AppLogLevel, Credential, CredentialPlanSnapshot, ProviderKind, RequestLog,
    UpstreamAttemptLog, UpstreamAttemptOutcome, UpstreamAttemptPhase,
};

// Re-export types used by callers outside this module.
pub use selector::{CredOAuth, ExpandedPick};

/// Persist a circuit-breaker state change as an app log entry.
pub(crate) fn emit_circuit_event(
    state: &AppState,
    cb_key: &str,
    change: crate::circuit_breaker::CircuitStateChange,
) {
    use crate::circuit_breaker::CircuitStateChange;
    let (level, message, detail) = match change {
        CircuitStateChange::Opened {
            consecutive_failures,
        } => (
            AppLogLevel::Warn,
            format!("Circuit opened: {cb_key}"),
            Some(format!("{consecutive_failures} consecutive failures")),
        ),
        CircuitStateChange::Closed => (
            AppLogLevel::Info,
            format!("Circuit recovered: {cb_key}"),
            None,
        ),
        CircuitStateChange::ManualReset => {
            (AppLogLevel::Info, format!("Circuit reset: {cb_key}"), None)
        }
    };
    let ev = AppLogEvent {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
        level,
        category: "circuit".to_string(),
        message,
        detail,
    };
    state.app_logs.push(ev);
}

const CODEX_STICKY_ROUTE_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Carried on streaming [`Response`] extensions (not HTTP headers) so Codex WS can patch `client_response_body` after translating Chat SSE → Responses events.
#[derive(Clone, Debug)]
pub struct VibeLogId(pub String);

/// Carried on streaming/non-streaming [`Response`] extensions so the Codex
/// route wrapper can emit client-visible route/quota status without affecting
/// plain OpenAI-compatible routes.
#[derive(Clone, Debug)]
pub struct VibeCodexVisual(pub CodexVisualContext);

/// Carried on Codex route responses so wrappers can render Vibe+ summaries
/// according to the original client (Desktop, CLI, or unknown).
#[derive(Clone, Debug)]
pub struct VibeCodexClientKind(pub CodexClientKind);

// ---------------------------------------------------------------------------
// ChatGPT Codex HTTP API: non-empty `instructions`
// ---------------------------------------------------------------------------

/// Codex CLI omits `instructions` when empty (`skip_serializing_if`); ChatGPT's
/// Codex handler returns `{"detail":"Instructions are required"}`. Inject a
/// minimal default only when the field is absent or whitespace-empty.
const CHATGPT_CODEX_FALLBACK_INSTRUCTIONS: &str =
    "You are Codex, OpenAI's coding agent. Help with software engineering tasks using available tools.";

pub(crate) fn inject_chatgpt_codex_instructions_if_missing(
    provider: &vibe_protocol::Provider,
    wire: Wire,
    body: Bytes,
) -> Bytes {
    if wire != Wire::OpenaiResponses || !router::provider_is_chatgpt_codex_official(provider) {
        return body;
    }
    tracing::debug!("injected default instructions for ChatGPT Codex (client omitted empty)");
    transforms::ensure_responses_instructions_if_missing(&body, CHATGPT_CODEX_FALLBACK_INSTRUCTIONS)
}

// ---------------------------------------------------------------------------
// Structured logging fields + Codex Plan snapshots from response headers
// ---------------------------------------------------------------------------

/// Per-request metadata persisted alongside [`RequestLog`].
#[derive(Clone, Debug)]
pub(crate) struct LogCtx {
    pub wire: Wire,
    pub route_prefix: Option<String>,
    pub credential_id: Option<String>,
    pub cb_key: Option<String>,
    pub dedupe_key: Option<String>,
    pub client_transport: Option<String>,
    pub request_headers: Option<String>,
    pub codex_client_kind: CodexClientKind,
    pub claude_client_kind: ClaudeClientKind,
}

pub(crate) fn wire_as_str(wire: Wire) -> &'static str {
    match wire {
        Wire::Anthropic => "anthropic",
        Wire::OpenaiChat => "openai-chat",
        Wire::OpenaiResponses => "openai-responses",
        Wire::GeminiNative => "gemini-native",
    }
}

/// Appended to `RequestLog.error` when several providers/credentials were tried (consistent with website Logs parsing).
const ROUTING_ATTEMPTS_MARKER: &str = "\n\n── routing attempts ──\n";

fn routing_id_tail(id: &str) -> String {
    if id.is_empty() {
        return "—".to_string();
    }
    let n = id.chars().count();
    if n <= 12 {
        return id.to_string();
    }
    let prefix: String = id.chars().take(10).collect();
    format!("{prefix}…")
}

fn routing_provider_line(provider: &vibe_protocol::Provider) -> String {
    let id_tail = routing_id_tail(&provider.id);
    let name = provider.name.trim();
    if name.is_empty() {
        return id_tail;
    }
    format!("{name} [{id_tail}]")
}

fn routing_credential_line(cred: Option<&Credential>, cred_id: &Option<String>) -> String {
    if let Some(c) = cred {
        let label = c.label.trim();
        let id_tail = routing_id_tail(&c.id);
        if !label.is_empty() {
            return format!("cred {label} [{id_tail}]");
        }
        return format!("cred [{id_tail}]");
    }
    match cred_id {
        Some(id) if !id.is_empty() => format!("cred {}", routing_id_tail(id)),
        _ => "cred —".to_string(),
    }
}

fn format_cb_skipped_provider_preview(
    ids: &[String],
    providers: &[vibe_protocol::Provider],
) -> String {
    let map: HashMap<&str, &vibe_protocol::Provider> =
        providers.iter().map(|p| (p.id.as_str(), p)).collect();
    ids.iter()
        .take(6)
        .map(|id| {
            map.get(id.as_str())
                .map(|p| routing_provider_line(*p))
                .unwrap_or_else(|| routing_id_tail(id))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn format_routing_attempt(
    provider: &vibe_protocol::Provider,
    credential: Option<&Credential>,
    cred_id: &Option<String>,
    outcome: impl std::fmt::Display,
) -> String {
    format!(
        "{} · {} · {}",
        routing_provider_line(provider),
        routing_credential_line(credential, cred_id),
        outcome
    )
}

fn push_routing_attempt(
    trace: &mut Vec<String>,
    provider: &vibe_protocol::Provider,
    credential: Option<&Credential>,
    cred_id: &Option<String>,
    outcome: impl std::fmt::Display,
) {
    trace.push(format_routing_attempt(provider, credential, cred_id, outcome));
}

pub(crate) fn compose_routing_error_message(summary: &str, trace: &[String]) -> String {
    if trace.is_empty() {
        summary.to_string()
    } else {
        format!("{}{}{}", summary, ROUTING_ATTEMPTS_MARKER, trace.join("\n"))
    }
}

pub(crate) fn needs_chat_to_responses_bridge(wire: Wire, provider_kind: ProviderKind) -> bool {
    wire == Wire::OpenaiResponses && provider_kind == ProviderKind::OpenaiChat
}

pub(crate) fn request_model_for_body(
    wire: Wire,
    upstream_path: Option<&str>,
    body: &[u8],
) -> String {
    if wire == Wire::GeminiNative {
        upstream_path
            .and_then(|p| p.rsplit('/').next())
            .and_then(|s| s.split(':').next())
            .unwrap_or("")
            .to_string()
    } else {
        extract_model(body).unwrap_or_default()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn request_log_from_parts(
    ctx: &LogCtx,
    id: &str,
    started_at: i64,
    started_instant: &Instant,
    app: &Option<String>,
    provider_id: Option<&str>,
    requested_model: &str,
    upstream_model: &str,
    status_code: Option<i32>,
    upstream_http_status: Option<i32>,
    upstream_error_preview: Option<String>,
    error: Option<String>,
    usage: Usage,
    request_body: Option<String>,
    response_body: Option<String>,
) -> RequestLog {
    build_log(
        ctx,
        id,
        started_at,
        started_instant,
        app,
        provider_id,
        requested_model,
        upstream_model,
        status_code,
        upstream_http_status,
        upstream_error_preview,
        error,
        usage,
        request_body,
        response_body,
    )
}

pub(crate) fn persist_request_log(state: &AppState, log: RequestLog) {
    persist_log(state, log);
}

pub(crate) fn publish_request_started(
    _state: &AppState,
    _id: &str,
    _started_at: i64,
    _app: &Option<String>,
    _log_ctx: &LogCtx,
    _provider_id: Option<&str>,
    _requested_model: &str,
) {
}

pub(crate) fn persist_request_log_placeholder(
    state: &AppState,
    id: &str,
    started_at: i64,
    app: &Option<String>,
    log_ctx: &LogCtx,
    provider_id: Option<&str>,
    requested_model: &str,
) {
    let log = RequestLog {
        id: id.to_string(),
        started_at,
        app: app.clone(),
        provider_id: provider_id.map(str::to_string),
        requested_model: if requested_model.is_empty() {
            None
        } else {
            Some(requested_model.to_string())
        },
        upstream_model: None,
        status_code: None,
        error: None,
        latency_ms: None,
        first_token_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        estimated_cost_usd: "0".to_string(),
        wire: Some(wire_as_str(log_ctx.wire).to_string()),
        route_prefix: log_ctx.route_prefix.clone(),
        credential_id: log_ctx.credential_id.clone(),
        cb_key: log_ctx.cb_key.clone(),
        upstream_http_status: None,
        upstream_error_preview: None,
        dedupe_key: log_ctx.dedupe_key.clone(),
        client_transport: log_ctx.client_transport.clone(),
        request_headers: log_ctx.request_headers.clone(),
        request_body: None,
        response_body: None,
        client_response_body: None,
        stream_kind: None,
        stream_terminal_seen: None,
        stream_end_reason: None,
        stream_error_detail: None,
        upstream_first_byte_ms: None,
        client_first_write_ms: None,
        last_upstream_event_ms: None,
        last_client_write_ms: None,
        upstream_chunk_count: 0,
        upstream_bytes: 0,
        client_chunk_count: 0,
        client_bytes: 0,
        sse_event_count: 0,
        sse_data_count: 0,
        sse_comment_count: 0,
        sse_keepalive_count: 0,
        sse_done_count: 0,
        parse_error_count: 0,
        first_keepalive_ms: None,
        last_keepalive_ms: None,
        max_gap_between_upstream_events_ms: None,
        max_gap_between_data_events_ms: None,
        keepalive_after_last_data_count: 0,
        last_data_event_ms: None,
        bridge_mode: None,
        status_injected: false,
        terminal_injected: false,
        upstream_terminal_type: None,
    };
    persist_log(state, log);
}

#[derive(Clone, Debug)]
pub(crate) struct AttemptCtx {
    pub attempt_id: String,
    pub request_id: String,
    pub attempt_index: i32,
    pub started_at: i64,
    pub provider_id: Option<String>,
    pub credential_id: Option<String>,
    pub requested_model: String,
    pub upstream_model: String,
}

pub(crate) fn new_attempt_ctx(
    request_id: &str,
    attempt_index: i32,
    started_at: i64,
    provider_id: Option<&str>,
    credential_id: Option<&str>,
    requested_model: &str,
    upstream_model: &str,
) -> AttemptCtx {
    AttemptCtx {
        attempt_id: uuid::Uuid::new_v4().to_string(),
        request_id: request_id.to_string(),
        attempt_index,
        started_at,
        provider_id: provider_id.map(str::to_string),
        credential_id: credential_id.map(str::to_string),
        requested_model: requested_model.to_string(),
        upstream_model: upstream_model.to_string(),
    }
}

pub(crate) fn publish_upstream_attempt_started(
    _state: &AppState,
    _log_ctx: &LogCtx,
    _attempt: &AttemptCtx,
    _phase: UpstreamAttemptPhase,
) {
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_runtime_stats(
    _state: &AppState,
    _request_id: &str,
    _attempt_id: Option<&str>,
    _provider_id: Option<&str>,
    _active_request_tokens_per_sec: Option<f64>,
    _active_upstream_decode_tps: Option<f64>,
    _active_downstream_emit_tps: Option<f64>,
    _active_output_tokens_per_sec: Option<f64>,
    _active_upstream_bytes_per_sec: f64,
    _active_downstream_bytes_per_sec: f64,
    _active_flow_bytes_per_sec: f64,
    _output_tokens_so_far: i64,
    _upstream_bytes_so_far: i64,
    _client_bytes_so_far: i64,
    _upstream_first_byte_ms: Option<i64>,
    _client_first_write_ms: Option<i64>,
    _attempt_scoped: bool,
) {
}

pub(crate) fn attempt_log_from_parts(
    log_ctx: &LogCtx,
    attempt: &AttemptCtx,
    phase: UpstreamAttemptPhase,
    outcome: UpstreamAttemptOutcome,
    started_instant: &Instant,
    status_code: Option<i32>,
    upstream_http_status: Option<i32>,
    error_summary: Option<String>,
    usage: Usage,
) -> UpstreamAttemptLog {
    UpstreamAttemptLog {
        attempt_id: attempt.attempt_id.clone(),
        request_id: attempt.request_id.clone(),
        attempt_index: attempt.attempt_index,
        started_at: attempt.started_at,
        ended_at: Some(chrono::Utc::now().timestamp()),
        provider_id: attempt.provider_id.clone(),
        credential_id: attempt.credential_id.clone(),
        wire: Some(wire_as_str(log_ctx.wire).to_string()),
        route_prefix: log_ctx.route_prefix.clone(),
        requested_model: (!attempt.requested_model.is_empty())
            .then(|| attempt.requested_model.clone()),
        upstream_model: (!attempt.upstream_model.is_empty())
            .then(|| attempt.upstream_model.clone()),
        phase,
        outcome,
        status_code,
        upstream_http_status,
        error_summary,
        latency_ms: Some(started_instant.elapsed().as_millis() as i64),
        first_token_ms: None,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        upstream_first_byte_ms: None,
        client_first_write_ms: None,
        last_upstream_event_ms: None,
        last_client_write_ms: None,
        upstream_chunk_count: 0,
        upstream_bytes: 0,
        client_chunk_count: 0,
        client_bytes: 0,
        sse_event_count: 0,
        sse_data_count: 0,
        sse_comment_count: 0,
        sse_keepalive_count: 0,
        sse_done_count: 0,
        parse_error_count: 0,
        first_keepalive_ms: None,
        last_keepalive_ms: None,
        max_gap_between_upstream_events_ms: None,
        max_gap_between_data_events_ms: None,
        keepalive_after_last_data_count: 0,
        last_data_event_ms: None,
        bridge_mode: Some("none".into()),
        status_injected: false,
        terminal_injected: false,
        upstream_terminal_type: None,
        active_upstream_decode_tps_peak: None,
        active_downstream_emit_tps_peak: None,
        request_headers: log_ctx.request_headers.clone(),
        request_body: None,
        response_headers: None,
        response_body: None,
    }
}

pub(crate) fn persist_upstream_attempt_log(state: &AppState, attempt: UpstreamAttemptLog) {
    state.upstream_attempt_logs.push(attempt);
}

pub(crate) fn mark_provider_health(
    state: &AppState,
    provider_id: &str,
    success: bool,
    latency_ms: i64,
    error: Option<String>,
) {
    fire_health(state, provider_id, success, latency_ms, error);
}

pub(crate) async fn record_codex_plan_from_response_headers(
    state: &AppState,
    headers: &http::HeaderMap,
    provider: &vibe_protocol::Provider,
    credential_id: Option<&str>,
) {
    maybe_record_codex_plan(state, headers, provider, credential_id);
}

#[derive(Debug)]
pub(crate) enum PreparedForwardError {
    Db(String),
    NoCandidates {
        log_id: String,
        started_at: i64,
        started_instant: Instant,
        app: Option<String>,
        requested_model: String,
        log_ctx: LogCtx,
        request_snapshot: Option<String>,
    },
    Exhausted {
        log_id: String,
        started_at: i64,
        started_instant: Instant,
        app: Option<String>,
        requested_model: String,
        log_ctx: LogCtx,
        request_snapshot: Option<String>,
        message: String,
    },
}

pub(crate) struct PreparedForward {
    pub log_id: String,
    pub started_at: i64,
    pub started_instant: Instant,
    pub app: Option<String>,
    pub log_ctx: LogCtx,
    pub request_snapshot: Option<String>,
    pub provider: vibe_protocol::Provider,
    pub upstream_model: String,
    pub requested_model: String,
    pub credential_id: Option<String>,
    pub secret: Option<String>,
    pub body_up: Bytes,
    pub visual: CodexVisualContext,
    /// Sticky-routing key derived from the request body/headers.
    /// Exposed so callers can forget it when the upstream connection fails,
    /// preventing future retries from being locked onto the same broken slot.
    pub sticky_key: Option<String>,
}

pub(crate) async fn prepare_forward_once(
    state: &AppState,
    wire: Wire,
    upstream_path: Option<&str>,
    req_headers: &HeaderMap,
    body: Bytes,
    route_prefix: Option<String>,
    preserve_ws_envelope: bool,
) -> Result<PreparedForward, PreparedForwardError> {
    let body = if wire == Wire::OpenaiResponses && !preserve_ws_envelope {
        let stripped = transforms::strip_ws_envelope(&body);
        if route_prefix.as_deref() == Some("codex-v1") {
            transforms::strip_vibe_codex_status_messages(&stripped)
        } else {
            stripped
        }
    } else if wire == Wire::OpenaiResponses && route_prefix.as_deref() == Some("codex-v1") {
        transforms::strip_vibe_codex_status_messages(&body)
    } else {
        body
    };

    let started_at = chrono::Utc::now().timestamp();
    let started_instant = Instant::now();
    let log_id = uuid::Uuid::new_v4().to_string();
    let app = detect_app(req_headers);
    let dedupe_key = dedupe_key_from_headers(req_headers, route_prefix.as_deref());
    let client_transport = req_headers
        .get("x-vibe-client-transport")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let request_headers =
        sanitized_headers_json(req_headers, state.config.log.redact_sensitive_headers);
    let codex_client_kind = crate::codex_summary::detect_client(req_headers);
    let claude_client_kind =
        crate::claude_summary::detect_client(req_headers, route_prefix.as_deref());
    let requested_model = request_model_for_body(wire, upstream_path, &body);

    let request_snapshot = None;

    let providers_list = state
        .db
        .provider_list()
        .map_err(|e| PreparedForwardError::Db(format!("db error: {e}")))?;

    let creds_by_provider: HashMap<String, Vec<Credential>> = {
        let creds = state.db.credential_list_all().unwrap_or_default();
        let mut map: HashMap<String, Vec<Credential>> = HashMap::new();
        for c in creds {
            if c.enabled {
                map.entry(c.provider_id.clone()).or_default().push(c);
            }
        }
        map
    };

    let counter = state.lb_counter.fetch_add(1, Ordering::Relaxed);
    let routed_candidates = router::candidates(&providers_list, wire, &requested_model);
    let candidates = selector::shuffle_candidates(routed_candidates, &state.cb);
    let empty_log_ctx = LogCtx {
        wire,
        route_prefix: route_prefix.clone(),
        credential_id: None,
        cb_key: None,
        dedupe_key: dedupe_key.clone(),
        client_transport: client_transport.clone(),
        request_headers: request_headers.clone(),
        codex_client_kind,
        claude_client_kind,
    };
    if candidates.is_empty() {
        return Err(PreparedForwardError::NoCandidates {
            log_id,
            started_at,
            started_instant,
            app,
            requested_model,
            log_ctx: empty_log_ctx,
            request_snapshot,
        });
    }

    let mut codex_plan_cred_ids: Vec<String> = Vec::new();
    for pick in &candidates {
        if !router::provider_is_chatgpt_codex_official(&pick.provider) {
            continue;
        }
        if let Some(creds) = creds_by_provider.get(&pick.provider.id) {
            for c in creds {
                codex_plan_cred_ids.push(c.id.clone());
            }
        }
    }
    codex_plan_cred_ids.sort();
    codex_plan_cred_ids.dedup();

    let plan_by_cred: HashMap<String, CredentialPlanSnapshot> = if codex_plan_cred_ids.is_empty() {
        HashMap::new()
    } else {
        let db = state.db.clone();
        let ids = codex_plan_cred_ids.clone();
        match tokio::task::spawn_blocking(move || db.plan_snapshot_latest_map(&ids)).await {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                tracing::warn!(?e, "batch load credential_plan_snapshots failed");
                HashMap::new()
            }
            Err(e) => {
                tracing::warn!(?e, "batch load credential_plan_snapshots join error");
                HashMap::new()
            }
        }
    };

    let expanded_picks =
        selector::expand_picks(candidates, &creds_by_provider, &plan_by_cred, counter);
    let sticky_key = codex_sticky_key(wire, req_headers, &body);
    let sticky_route = sticky_key
        .as_deref()
        .and_then(|k| state.get_codex_sticky_route(k, CODEX_STICKY_ROUTE_TTL));
    let expanded_picks = selector::apply_sticky_priority(sticky_route.as_ref(), expanded_picks);
    let mut last_error = String::from("all providers unavailable or circuit open");
    let mut routing_attempt_trace: Vec<String> = Vec::new();
    let mut cb_skipped_total: usize = 0;
    let mut cb_skipped_provider_ids: Vec<String> = Vec::new();
    let mut attempted_after_cb: usize = 0;

    for mut epick in expanded_picks {
        let provider = epick.provider;
        let upstream_model = epick.upstream_model;
        let cb_key = epick.cb_key.clone();
        let log_ctx = LogCtx {
            wire,
            route_prefix: route_prefix.clone(),
            credential_id: epick.credential_id.clone(),
            cb_key: Some(cb_key.clone()),
            dedupe_key: dedupe_key.clone(),
            client_transport: client_transport.clone(),
            request_headers: request_headers.clone(),
            codex_client_kind,
            claude_client_kind,
        };

        if !state.cb.allow(&cb_key) {
            tracing::debug!(cb_key = %cb_key, "circuit open, skipping");
            push_routing_attempt(
                &mut routing_attempt_trace,
                &provider,
                epick.credential.as_ref(),
                &epick.credential_id,
                "skipped (circuit open)",
            );
            cb_skipped_total += 1;
            if !cb_skipped_provider_ids
                .iter()
                .any(|pid| pid == &provider.id)
            {
                cb_skipped_provider_ids.push(provider.id.clone());
            }
            continue;
        }
        attempted_after_cb += 1;

        let secret = if let Some(oauth) = epick.oauth.take() {
            match resolve_oauth_token(state, epick.credential_id.as_deref(), oauth).await {
                Ok(t) => Some(t),
                Err(e) => {
                    if let Some(change) = state.cb.record_failure(&cb_key) {
                        emit_circuit_event(state, &cb_key, change);
                    }
                    if let Some(cid) = &epick.credential_id {
                        fire_credential_failure(
                            state,
                            cid.clone(),
                            Some(format!("oauth refresh failed: {e}")),
                        );
                    }
                    fire_health(
                        state,
                        &provider.id,
                        false,
                        started_instant.elapsed().as_millis() as i64,
                        Some("oauth refresh failed".into()),
                    );
                    push_routing_attempt(
                        &mut routing_attempt_trace,
                        &provider,
                        epick.credential.as_ref(),
                        &epick.credential_id,
                        format!("oauth refresh failed: {e}"),
                    );
                    last_error = format!("oauth error for {}: {e}", provider.id);
                    continue;
                }
            }
        } else {
            match epick.auth_ref.as_deref() {
                Some("passthrough") => {
                    if let Some(key) = req_headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
                        Some(key.to_string())
                    } else if let Some(auth) = req_headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                    {
                        Some(auth.strip_prefix("Bearer ").unwrap_or(auth).to_string())
                    } else {
                        None
                    }
                }
                Some(r) => match secrets::resolve(r) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        if let Some(change) = state.cb.record_failure(&cb_key) {
                            emit_circuit_event(state, &cb_key, change);
                        }
                        if let Some(cid) = &epick.credential_id {
                            fire_credential_failure(
                                state,
                                cid.clone(),
                                Some(format!("auth resolve failed: {e}")),
                            );
                        }
                        fire_health(
                            state,
                            &provider.id,
                            false,
                            started_instant.elapsed().as_millis() as i64,
                            Some("auth resolve failed".into()),
                        );
                        push_routing_attempt(
                            &mut routing_attempt_trace,
                            &provider,
                            epick.credential.as_ref(),
                            &epick.credential_id,
                            format!("auth resolve failed: {e}"),
                        );
                        last_error = format!("auth error for {}: {e}", provider.id);
                        continue;
                    }
                },
                None => None,
            }
        };

        let effective_body: Bytes =
            if provider.kind == ProviderKind::Anthropic && state.config.failover.inject_cache {
                cache::inject(&body)
            } else {
                body.clone()
            };

        let effective_body =
            inject_chatgpt_codex_instructions_if_missing(&provider, wire, effective_body);

        let adapter = providers::select(&provider);
        let body_up = if preserve_ws_envelope && wire == Wire::OpenaiResponses {
            match transforms::rewrite_responses_model(&effective_body, &upstream_model) {
                Ok(b) => b,
                Err(e) => {
                    push_routing_attempt(
                        &mut routing_attempt_trace,
                        &provider,
                        epick.credential.as_ref(),
                        &epick.credential_id,
                        format!("body rewrite: {e}"),
                    );
                    last_error = format!("body rewrite: {e}");
                    continue;
                }
            }
        } else {
            match adapter.rewrite_body_model(&effective_body, &upstream_model) {
                Ok(b) => b,
                Err(e) => {
                    push_routing_attempt(
                        &mut routing_attempt_trace,
                        &provider,
                        epick.credential.as_ref(),
                        &epick.credential_id,
                        format!("body rewrite: {e}"),
                    );
                    last_error = format!("body rewrite: {e}");
                    continue;
                }
            }
        };

        let visual = codex_visual_context(
            &provider,
            epick.credential.as_ref(),
            epick.credential_id.as_deref(),
            &plan_by_cred,
            &requested_model,
            &upstream_model,
        );

        return Ok(PreparedForward {
            log_id,
            started_at,
            started_instant,
            app,
            log_ctx,
            request_snapshot,
            provider,
            upstream_model,
            requested_model,
            credential_id: epick.credential_id,
            secret,
            body_up,
            visual,
            sticky_key: sticky_key.clone(),
        });
    }

    let final_error = if attempted_after_cb == 0 && cb_skipped_total > 0 {
        let preview = if cb_skipped_provider_ids.is_empty() {
            String::new()
        } else {
            let labels =
                format_cb_skipped_provider_preview(&cb_skipped_provider_ids, &providers_list);
            format!(", providers=[{labels}]")
        };
        format!(
            "all providers blocked by circuit breaker ({cb_skipped_total} skipped{preview}). reset via POST /_vp/providers/:id/circuit/reset or Providers UI"
        )
    } else {
        last_error
    };
    let message = compose_routing_error_message(&final_error, &routing_attempt_trace);

    Err(PreparedForwardError::Exhausted {
        log_id,
        started_at,
        started_instant,
        app,
        requested_model,
        log_ctx: empty_log_ctx,
        request_snapshot,
        message,
    })
}

fn dedupe_key_from_headers(headers: &HeaderMap, route_prefix: Option<&str>) -> Option<String> {
    let rid = headers
        .get("x-request-id")
        .or_else(|| headers.get("x-openai-request-id"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(format!("{}|{}", route_prefix.unwrap_or(""), rid))
}

pub(crate) fn sanitized_headers_json(headers: &HeaderMap, redact_sensitive: bool) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    let mut entries = serde_json::Map::new();
    for (name, value) in headers {
        let name = name.as_str().to_ascii_lowercase();
        if name.starts_with("x-vibe-") {
            continue;
        }
        let value = if redact_sensitive && is_sensitive_header(&name) {
            "<redacted>".to_string()
        } else {
            value
                .to_str()
                .map(str::to_owned)
                .unwrap_or_else(|_| "<non-utf8>".to_string())
        };
        entries.insert(name, serde_json::Value::String(value));
    }
    (!entries.is_empty()).then(|| serde_json::Value::Object(entries).to_string())
}

fn is_sensitive_header(name: &str) -> bool {
    name == "authorization"
        || name == "proxy-authorization"
        || name == "cookie"
        || name == "set-cookie"
        || name == "x-api-key"
        || name.ends_with("-api-key")
        || name.contains("token")
        || name.contains("secret")
}

pub(crate) fn maybe_record_codex_plan(
    state: &AppState,
    headers: &http::HeaderMap,
    provider: &vibe_protocol::Provider,
    credential_id: Option<&str>,
) {
    let Some(cid) = credential_id else {
        return;
    };
    if !router::provider_is_chatgpt_codex_official(provider) {
        return;
    }
    let Some(raw) = crate::codex_plan_headers::parse_codex_usage_headers(headers) else {
        return;
    };
    let norm = raw.normalize();
    let summary = raw.summary_line(&norm);
    let snap = CredentialPlanSnapshot {
        id: uuid::Uuid::new_v4().to_string(),
        credential_id: cid.to_string(),
        captured_at: chrono::Utc::now().timestamp(),
        codex_5h_used_percent: norm.used_5h_percent,
        codex_7d_used_percent: norm.used_7d_percent,
        codex_5h_reset_after_seconds: norm.reset_5h_seconds,
        codex_7d_reset_after_seconds: norm.reset_7d_seconds,
        codex_primary_used_percent: raw.primary_used_percent,
        codex_secondary_used_percent: raw.secondary_used_percent,
        summary: Some(summary),
        source: "response_headers".into(),
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _ = db.plan_snapshot_insert(&snap);
    });
}

// ExpandedPick, CredOAuth, expand_picks, cred_is_rate_limited, credential_plan_display_* and
// shuffle_candidates all live in the `selector` submodule.

// ---------------------------------------------------------------------------
// Rate-limit header extraction
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct RlHeaders {
    requests_limit: Option<i64>,
    requests_remaining: Option<i64>,
    requests_reset_at: Option<i64>,
    tokens_limit: Option<i64>,
    tokens_remaining: Option<i64>,
    tokens_reset_at: Option<i64>,
}

impl RlHeaders {
    fn is_empty(&self) -> bool {
        self.requests_limit.is_none()
            && self.requests_remaining.is_none()
            && self.tokens_limit.is_none()
            && self.tokens_remaining.is_none()
    }
}

pub(crate) fn extract_rl_headers(headers: &reqwest::header::HeaderMap) -> RlHeaders {
    fn hi(h: &reqwest::header::HeaderMap, name: &str) -> Option<i64> {
        h.get(name)?.to_str().ok()?.parse().ok()
    }

    /// Parse a reset header value to a Unix timestamp (seconds).
    /// Handles:
    ///   - RFC 3339 / ISO 8601: "2025-05-08T14:30:00Z"  (Anthropic)
    ///   - Duration string:     "3s", "1m30s", "90s"     (OpenAI)
    ///   - Raw integer:         "1746710400"              (epoch seconds)
    fn parse_reset(h: &reqwest::header::HeaderMap, name: &str) -> Option<i64> {
        let v = h.get(name)?.to_str().ok()?;
        // Integer epoch
        if let Ok(n) = v.parse::<i64>() {
            return Some(n);
        }
        // RFC 3339 / ISO 8601
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(v) {
            return Some(dt.timestamp());
        }
        // Duration string like "3s", "1m30s", "1m", "90s"
        let now = chrono::Utc::now().timestamp();
        let secs = parse_duration_secs(v)?;
        Some(now + secs)
    }

    let mut rl = RlHeaders {
        requests_limit: hi(headers, "anthropic-ratelimit-requests-limit")
            .or_else(|| hi(headers, "x-ratelimit-limit-requests")),
        requests_remaining: hi(headers, "anthropic-ratelimit-requests-remaining")
            .or_else(|| hi(headers, "x-ratelimit-remaining-requests")),
        requests_reset_at: parse_reset(headers, "anthropic-ratelimit-requests-reset")
            .or_else(|| parse_reset(headers, "x-ratelimit-reset-requests")),
        tokens_limit: hi(headers, "anthropic-ratelimit-tokens-limit")
            .or_else(|| hi(headers, "x-ratelimit-limit-tokens")),
        tokens_remaining: hi(headers, "anthropic-ratelimit-tokens-remaining")
            .or_else(|| hi(headers, "x-ratelimit-remaining-tokens")),
        tokens_reset_at: parse_reset(headers, "anthropic-ratelimit-tokens-reset")
            .or_else(|| parse_reset(headers, "x-ratelimit-reset-tokens")),
    };

    // Codex / ChatGPT plan quota headers (x-codex-primary-*).
    // When the primary (weekly) window is fully exhausted, synthesise standard RL fields
    // so `cred_is_rate_limited` blocks this credential until the plan resets.
    let codex_pct: Option<f64> = headers
        .get("x-codex-primary-used-percent")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    if codex_pct.is_some_and(|p| p >= 100.0) {
        let codex_reset = parse_reset(headers, "x-codex-primary-reset-at").or_else(|| {
            hi(headers, "x-codex-primary-reset-after-seconds")
                .map(|secs| chrono::Utc::now().timestamp() + secs)
        });
        if rl.requests_remaining.is_none() {
            rl.requests_remaining = Some(0);
        }
        if rl.requests_reset_at.is_none() {
            rl.requests_reset_at = codex_reset;
        }
    }

    // Codex secondary (5h) window exhaustion.  When the short window is full we
    // also block the credential, but prefer the *shorter* reset time so the
    // credential unlocks as soon as either window clears.
    let codex_secondary_pct: Option<f64> = headers
        .get("x-codex-secondary-used-percent")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    if codex_secondary_pct.is_some_and(|p| p >= 100.0) {
        let secondary_reset = parse_reset(headers, "x-codex-secondary-reset-at").or_else(|| {
            hi(headers, "x-codex-secondary-reset-after-seconds")
                .filter(|&s| s > 0)
                .map(|secs| chrono::Utc::now().timestamp() + secs)
        });
        if rl.requests_remaining.is_none() {
            rl.requests_remaining = Some(0);
        }
        rl.requests_reset_at = match (rl.requests_reset_at, secondary_reset) {
            (Some(existing), Some(new)) => Some(existing.min(new)),
            (existing, new) => existing.or(new),
        };
    }

    rl
}

/// Parse "3s", "1m", "1m30s", "90s" → seconds.
fn parse_duration_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    // Try "XmYs"
    if let Some(m_pos) = s.find('m') {
        let mins: i64 = s[..m_pos].parse().ok()?;
        let rest = &s[m_pos + 1..];
        let secs: i64 = if rest.is_empty() || rest == "s" {
            0
        } else {
            rest.trim_end_matches('s').parse().ok()?
        };
        return Some(mins * 60 + secs);
    }
    // Try "Xs"
    if let Some(stripped) = s.strip_suffix('s') {
        return stripped.parse::<i64>().ok();
    }
    None
}

pub(crate) fn fire_credential_success(state: &AppState, credential_id: String, rl: RlHeaders) {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _ = db.credential_record_success(&credential_id);
        if !rl.is_empty() {
            let _ = db.credential_update_rate_limit(
                &credential_id,
                rl.requests_limit,
                rl.requests_remaining,
                rl.requests_reset_at,
                rl.tokens_limit,
                rl.tokens_remaining,
                rl.tokens_reset_at,
            );
        }
    });
}

pub(crate) fn fire_credential_failure(state: &AppState, credential_id: String, error: Option<String>) {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _ = db.credential_record_failure(&credential_id, error.as_deref());
    });
}

/// Persist RL headers from a non-success response without counting success/failure streaks.
///
/// When the upstream sends 429 but no standard rate-limit headers (common for non-OpenAI
/// providers), we apply a 60-second default cooldown so the credential is skipped on
/// the next request instead of being retried immediately.
pub(crate) fn fire_credential_rate_limit_only(state: &AppState, credential_id: String, rl: RlHeaders) {
    let (req_remaining, req_reset_at) = if rl.requests_remaining.is_some() {
        (rl.requests_remaining, rl.requests_reset_at)
    } else {
        // No RL headers in the 429 response — apply a conservative 60-second cooldown.
        let reset_at = chrono::Utc::now().timestamp() + 60;
        (Some(0), Some(reset_at))
    };
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _ = db.credential_update_rate_limit(
            &credential_id,
            rl.requests_limit,
            req_remaining,
            req_reset_at,
            rl.tokens_limit,
            rl.tokens_remaining,
            rl.tokens_reset_at,
        );
    });
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn forward(
    state: AppState,
    wire: Wire,
    upstream_path: Option<String>,
    req_headers: HeaderMap,
    body: Bytes,
    route_prefix: Option<String>,
) -> Response {
    // Codex CLI/Desktop may POST `{"type":"response.create",...}` to `/v1/responses`
    // (not only `/codex/v1/responses`). Normalize here so routing + upstream body agree.
    let body = if wire == Wire::OpenaiResponses {
        let stripped = transforms::strip_ws_envelope(&body);
        if route_prefix.as_deref() == Some("codex-v1") {
            transforms::strip_vibe_codex_status_messages(&stripped)
        } else {
            stripped
        }
    } else {
        body
    };

    let started_at = chrono::Utc::now().timestamp();
    let started_instant = Instant::now();
    let log_id = uuid::Uuid::new_v4().to_string();
    let app = detect_app(&req_headers);
    let dedupe_key = dedupe_key_from_headers(&req_headers, route_prefix.as_deref());
    let client_transport = req_headers
        .get("x-vibe-client-transport")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let request_headers =
        sanitized_headers_json(&req_headers, state.config.log.redact_sensitive_headers);
    let codex_client_kind = crate::codex_summary::detect_client(&req_headers);
    let claude_client_kind =
        crate::claude_summary::detect_client(&req_headers, route_prefix.as_deref());
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

    let request_snapshot = None;

    let providers_list = match state.db.provider_list() {
        Ok(v) => v,
        Err(e) => return internal_error(format!("db error: {e}")),
    };

    // Load credentials grouped by provider_id for key-pool rotation.
    let creds_by_provider: HashMap<String, Vec<Credential>> = {
        let creds = state.db.credential_list_all().unwrap_or_default();
        let mut map: HashMap<String, Vec<Credential>> = HashMap::new();
        for c in creds {
            if c.enabled {
                map.entry(c.provider_id.clone()).or_default().push(c);
            }
        }
        map
    };

    let counter = state.lb_counter.fetch_add(1, Ordering::Relaxed);
    let routed_candidates = router::candidates(&providers_list, wire, &requested_model);
    let candidates = selector::shuffle_candidates(routed_candidates, &state.cb);
    if candidates.is_empty() {
        let log_ctx = LogCtx {
            wire,
            route_prefix: route_prefix.clone(),
            credential_id: None,
            cb_key: None,
            dedupe_key: dedupe_key.clone(),
            client_transport: client_transport.clone(),
            request_headers: request_headers.clone(),
            codex_client_kind,
            claude_client_kind,
        };
        let no_pick_ctx = vec![format!(
            "context · cred — · wire {} · model {}",
            wire_as_str(wire),
            requested_model
        )];
        let log = build_log(
            &log_ctx,
            &log_id,
            started_at,
            &started_instant,
            &app,
            None,
            &requested_model,
            "",
            Some(503),
            None,
            None,
            Some(compose_routing_error_message(
                "no provider matches request shape",
                &no_pick_ctx,
            )),
            Usage::default(),
            request_snapshot.clone(),
            None,
        );
        persist_log(&state, log);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "no provider matches request shape",
        )
            .into_response();
    }

    let mut codex_plan_cred_ids: Vec<String> = Vec::new();
    for pick in &candidates {
        if !router::provider_is_chatgpt_codex_official(&pick.provider) {
            continue;
        }
        if let Some(creds) = creds_by_provider.get(&pick.provider.id) {
            for c in creds {
                codex_plan_cred_ids.push(c.id.clone());
            }
        }
    }
    codex_plan_cred_ids.sort();
    codex_plan_cred_ids.dedup();

    let plan_by_cred: HashMap<String, CredentialPlanSnapshot> = if codex_plan_cred_ids.is_empty() {
        HashMap::new()
    } else {
        let db = state.db.clone();
        let ids = codex_plan_cred_ids.clone();
        match tokio::task::spawn_blocking(move || db.plan_snapshot_latest_map(&ids)).await {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                tracing::warn!(?e, "batch load credential_plan_snapshots failed");
                HashMap::new()
            }
            Err(e) => {
                tracing::warn!(?e, "batch load credential_plan_snapshots join error");
                HashMap::new()
            }
        }
    };

    let expanded_picks =
        selector::expand_picks(candidates, &creds_by_provider, &plan_by_cred, counter);
    let sticky_key = codex_sticky_key(wire, &req_headers, &body);
    let sticky_route = sticky_key
        .as_deref()
        .and_then(|k| state.get_codex_sticky_route(k, CODEX_STICKY_ROUTE_TTL));
    let expanded_picks = selector::apply_sticky_priority(sticky_route.as_ref(), expanded_picks);
    let request_start_ctx = LogCtx {
        wire,
        route_prefix: route_prefix.clone(),
        credential_id: None,
        cb_key: None,
        dedupe_key: dedupe_key.clone(),
        client_transport: client_transport.clone(),
        request_headers: request_headers.clone(),
        codex_client_kind,
        claude_client_kind,
    };
    publish_request_started(
        &state,
        &log_id,
        started_at,
        &app,
        &request_start_ctx,
        None,
        &requested_model,
    );
    persist_request_log_placeholder(
        &state,
        &log_id,
        started_at,
        &app,
        &request_start_ctx,
        None,
        &requested_model,
    );
    let mut last_error = String::from("all providers unavailable or circuit open");
    let mut routing_attempt_trace: Vec<String> = Vec::new();
    let mut cb_skipped_total: usize = 0;
    let mut cb_skipped_provider_ids: Vec<String> = Vec::new();
    let mut attempted_after_cb: usize = 0;
    let mut attempt_index: i32 = 0;

    let pick_ctx = Arc::new(race::PickCtx {
        wire,
        route_prefix: route_prefix.clone(),
        log_id: log_id.clone(),
        started_at,
        started_instant,
        app: app.clone(),
        requested_model: requested_model.clone(),
        upstream_path: upstream_path.clone(),
        dedupe_key: dedupe_key.clone(),
        client_transport: client_transport.clone(),
        request_headers_json: request_headers.clone(),
        codex_client_kind,
        claude_client_kind,
        body: body.clone(),
        req_headers: req_headers.clone(),
        request_snapshot: request_snapshot.clone(),
        sticky_key: sticky_key.clone(),
        plan_by_cred: Arc::new(plan_by_cred),
    });

    // Health-bucketed wave dispatch: 病患 (CB Open) 4-wide concurrent →
    // 半健康 (HalfOpen) 2-wide → 健康 (Closed) 1-at-a-time. Within each wave
    // racers run concurrently and the first terminal response wins, with
    // `CancellationToken` aborting losers. Between waves we serialize, so a
    // single healthy upstream is never wasted in parallel with itself.
    let waves = selector::build_waves(expanded_picks, &state.cb);
    for wave in waves {
        let n = wave.len() as i32;
        match race::run_wave(state.clone(), wave, pick_ctx.clone(), attempt_index).await {
            race::WaveOutcome::Final(resp) => return resp,
            race::WaveOutcome::AllNonTerminal {
                last_error: err,
                routing_notes,
                cb_skip_provider_ids,
                retry_count,
            } => {
                if let Some(e) = err {
                    last_error = e;
                }
                routing_attempt_trace.extend(routing_notes);
                attempted_after_cb += retry_count;
                for pid in cb_skip_provider_ids {
                    cb_skipped_total += 1;
                    if !cb_skipped_provider_ids.iter().any(|p| p == &pid) {
                        cb_skipped_provider_ids.push(pid);
                    }
                }
            }
        }
        attempt_index += n;
    }


    // All candidates exhausted
    let log_ctx = LogCtx {
        wire,
        route_prefix: route_prefix.clone(),
        credential_id: None,
        cb_key: None,
        dedupe_key: dedupe_key.clone(),
        client_transport,
        request_headers,
        codex_client_kind,
        claude_client_kind,
    };
    let final_error = if attempted_after_cb == 0 && cb_skipped_total > 0 {
        let preview = if cb_skipped_provider_ids.is_empty() {
            String::new()
        } else {
            let labels =
                format_cb_skipped_provider_preview(&cb_skipped_provider_ids, &providers_list);
            format!(", providers=[{labels}]")
        };
        format!(
            "all providers blocked by circuit breaker ({cb_skipped_total} skipped{preview}). reset via POST /_vp/providers/:id/circuit/reset or Providers UI"
        )
    } else {
        last_error
    };
    let log = build_log(
        &log_ctx,
        &log_id,
        started_at,
        &started_instant,
        &app,
        None,
        &requested_model,
        "",
        Some(503),
        None,
        None,
        Some(compose_routing_error_message(
            &final_error,
            &routing_attempt_trace,
        )),
        Usage::default(),
        request_snapshot.clone(),
        None,
    );
    persist_log(&state, log);
    (StatusCode::SERVICE_UNAVAILABLE, final_error).into_response()
}

// ---------------------------------------------------------------------------
// Streaming path
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) fn stream_response(
    state: AppState,
    adapter: Box<dyn Adapter + Send + Sync>,
    wire: Wire,
    upstream: reqwest::Response,
    status: StatusCode,
    resp_headers: HeaderMap,
    resp_headers_snapshot: Option<String>,
    log_id: String,
    started_at: i64,
    started_instant: Instant,
    app: Option<String>,
    attempt: AttemptCtx,
    provider_id: String,
    requested_model: String,
    upstream_model: String,
    log_ctx: LogCtx,
    request_body: Option<String>,
    visual: CodexVisualContext,
) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);
    let state_for_task = state.clone();
    let log_id_clone = log_id.clone();
    let codex_client_kind = log_ctx.codex_client_kind;
    let claude_summary_request_id = log_ctx.dedupe_key.clone().or_else(|| Some(log_id.clone()));

    tokio::spawn(async move {
        let mut byte_stream = upstream.bytes_stream();
        let mut acc = Usage::default();
        let mut first_token_ms: Option<i64> = None;
        let mut buf = String::new();
        let mut sse_buf: Vec<u8> = Vec::new();
        let mut raw_accum: Vec<u8> = Vec::new();
        let mut trace = StreamTraceStats::new("sse", "passthrough");
        let mut upstream_decode_tps_peak: Option<f64> = None;
        let mut downstream_emit_tps_peak: Option<f64> = None;
        let mut downstream_closed = false;
        let mut claude_summary_injection = (wire == Wire::Anthropic).then(|| {
            crate::claude_summary::ClaudeSummaryAccumulator::new_for_request(
                state_for_task.claude_summary_config(),
                log_ctx.claude_client_kind,
                Some(state_for_task.clone()),
                claude_summary_request_id,
            )
        });

        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    trace.record_upstream_chunk(&started_instant, bytes.len());
                    raw_accum.extend_from_slice(&bytes);
                    if first_token_ms.is_none() {
                        first_token_ms = Some(started_instant.elapsed().as_millis() as i64);
                    }
                    if let Some(summary_injection) = claude_summary_injection.as_mut() {
                        sse_buf.extend_from_slice(&bytes);
                        while let Some((pos, delimiter_len)) = find_sse_delimiter(&sse_buf) {
                            let event = sse_buf[..pos].to_vec();
                            let frame = sse_buf[..pos + delimiter_len].to_vec();
                            sse_buf.drain(..pos + delimiter_len);
                            let event_text = String::from_utf8_lossy(&event).into_owned();
                            trace.record_sse_block(&started_instant, &event_text);
                            parse_sse_block_usage(adapter.as_ref(), wire, &event_text, &mut acc);
                            if let Some(summary) = summary_injection.before_forwarding_sse_block(
                                &event_text,
                                started_instant.elapsed().as_millis() as i64,
                            ) {
                                let summary_len = summary.len();
                                if tx.send(Ok(Bytes::from(summary))).await.is_err() {
                                    trace.finish("downstream_closed");
                                    downstream_closed = true;
                                    break;
                                } else {
                                    trace.record_client_chunk(&started_instant, summary_len);
                                }
                            }
                            let frame_len = frame.len();
                            if tx.send(Ok(Bytes::from(frame))).await.is_err() {
                                trace.finish("downstream_closed");
                                downstream_closed = true;
                                break;
                            } else {
                                trace.record_client_chunk(&started_instant, frame_len);
                            }
                        }
                        if downstream_closed {
                            break;
                        }
                    } else {
                        if let Ok(s) = std::str::from_utf8(&bytes) {
                            buf.push_str(s);
                            while let Some(pos) = buf.find("\n\n") {
                                let event = buf[..pos].to_string();
                                buf.drain(..pos + 2);
                                trace.record_sse_block(&started_instant, &event);
                                parse_sse_block_usage(adapter.as_ref(), wire, &event, &mut acc);
                            }
                        }
                        let byte_len = bytes.len();
                        if tx.send(Ok(bytes)).await.is_err() {
                            trace.finish("downstream_closed");
                            break;
                        } else {
                            trace.record_client_chunk(&started_instant, byte_len);
                        }
                    }
                    let elapsed_ms = started_instant.elapsed().as_millis() as i64;
                    let active_upstream_decode_tps =
                        trace.active_upstream_decode_tps(acc.output_tokens, elapsed_ms);
                    let active_downstream_emit_tps =
                        trace.active_downstream_emit_tps(acc.output_tokens, elapsed_ms);
                    let runtime_rates = trace.runtime_rates(acc.output_tokens, elapsed_ms);
                    update_peak(&mut upstream_decode_tps_peak, active_upstream_decode_tps);
                    update_peak(&mut downstream_emit_tps_peak, active_downstream_emit_tps);
                    publish_runtime_stats(
                        &state_for_task,
                        &attempt.request_id,
                        Some(&attempt.attempt_id),
                        Some(&provider_id),
                        None,
                        active_upstream_decode_tps,
                        active_downstream_emit_tps,
                        runtime_rates.active_output_tokens_per_sec,
                        runtime_rates.active_upstream_bytes_per_sec,
                        runtime_rates.active_downstream_bytes_per_sec,
                        runtime_rates.active_flow_bytes_per_sec,
                        acc.output_tokens,
                        trace.upstream_bytes(),
                        trace.client_bytes(),
                        trace.upstream_first_byte_ms(),
                        trace.client_first_write_ms(),
                        true,
                    );
                    publish_runtime_stats(
                        &state_for_task,
                        &attempt.request_id,
                        None,
                        Some(&provider_id),
                        active_upstream_decode_tps.or(runtime_rates.active_output_tokens_per_sec),
                        None,
                        None,
                        runtime_rates.active_output_tokens_per_sec,
                        runtime_rates.active_upstream_bytes_per_sec,
                        runtime_rates.active_downstream_bytes_per_sec,
                        runtime_rates.active_flow_bytes_per_sec,
                        acc.output_tokens,
                        trace.upstream_bytes(),
                        trace.client_bytes(),
                        trace.upstream_first_byte_ms(),
                        trace.client_first_write_ms(),
                        false,
                    );
                }
                Err(e) => {
                    let detail = e.to_string();
                    trace.finish_error("upstream_read_error", detail.clone());
                    let _ = tx.send(Err(std::io::Error::other(detail))).await;
                    break;
                }
            }
            if downstream_closed {
                break;
            }
        }
        if let Some(summary_injection) = claude_summary_injection.as_mut() {
            if !sse_buf.is_empty() {
                let event_text = String::from_utf8_lossy(&sse_buf).into_owned();
                if !event_text.trim().is_empty() {
                    trace.record_sse_block(&started_instant, &event_text);
                    parse_sse_block_usage(adapter.as_ref(), wire, &event_text, &mut acc);
                    if let Some(summary) = summary_injection.before_forwarding_sse_block(
                        &event_text,
                        started_instant.elapsed().as_millis() as i64,
                    ) {
                        let summary_len = summary.len();
                        if tx.send(Ok(Bytes::from(summary))).await.is_ok() {
                            trace.record_client_chunk(&started_instant, summary_len);
                        }
                    }
                }
                let leftover_len = sse_buf.len();
                if tx
                    .send(Ok(Bytes::from(std::mem::take(&mut sse_buf))))
                    .await
                    .is_ok()
                {
                    trace.record_client_chunk(&started_instant, leftover_len);
                }
            }
        } else if !buf.trim().is_empty() {
            trace.record_sse_block(&started_instant, &buf);
        }
        if trace.terminal_seen() {
            trace.finish("completed");
        } else if trace.end_reason().is_none() {
            trace.finish("upstream_eof");
        }

        let sc = status.as_u16() as i32;
        let response_body = None;
        let mut log = build_log(
            &log_ctx,
            &log_id,
            started_at,
            &started_instant,
            &app,
            Some(&provider_id),
            &requested_model,
            &upstream_model,
            Some(sc),
            Some(sc),
            None,
            None,
            acc,
            request_body,
            response_body,
        );
        log.first_token_ms = first_token_ms;
        trace.apply_to_log(&mut log);
        let mut attempt_log = attempt_log_from_parts(
            &log_ctx,
            &attempt,
            UpstreamAttemptPhase::Completed,
            UpstreamAttemptOutcome::Success,
            &started_instant,
            Some(sc),
            Some(sc),
            None,
            acc,
        );
        attempt_log.first_token_ms = first_token_ms;
        attempt_log.upstream_first_byte_ms = log.upstream_first_byte_ms;
        attempt_log.client_first_write_ms = log.client_first_write_ms;
        attempt_log.last_upstream_event_ms = log.last_upstream_event_ms;
        attempt_log.last_client_write_ms = log.last_client_write_ms;
        attempt_log.upstream_chunk_count = log.upstream_chunk_count;
        attempt_log.upstream_bytes = log.upstream_bytes;
        attempt_log.client_chunk_count = log.client_chunk_count;
        attempt_log.client_bytes = log.client_bytes;
        attempt_log.sse_event_count = log.sse_event_count;
        attempt_log.sse_data_count = log.sse_data_count;
        attempt_log.sse_comment_count = log.sse_comment_count;
        attempt_log.sse_keepalive_count = log.sse_keepalive_count;
        attempt_log.sse_done_count = log.sse_done_count;
        attempt_log.parse_error_count = log.parse_error_count;
        attempt_log.first_keepalive_ms = log.first_keepalive_ms;
        attempt_log.last_keepalive_ms = log.last_keepalive_ms;
        attempt_log.max_gap_between_upstream_events_ms = log.max_gap_between_upstream_events_ms;
        attempt_log.max_gap_between_data_events_ms = log.max_gap_between_data_events_ms;
        attempt_log.keepalive_after_last_data_count = log.keepalive_after_last_data_count;
        attempt_log.last_data_event_ms = log.last_data_event_ms;
        attempt_log.bridge_mode = log.bridge_mode.clone();
        attempt_log.status_injected = log.status_injected;
        attempt_log.terminal_injected = log.terminal_injected;
        attempt_log.upstream_terminal_type = log.upstream_terminal_type.clone();
        attempt_log.active_upstream_decode_tps_peak = upstream_decode_tps_peak;
        attempt_log.active_downstream_emit_tps_peak = downstream_emit_tps_peak;
        attempt_log.response_headers = resp_headers_snapshot;
        persist_upstream_attempt_log(&state_for_task, attempt_log);
        finalize_stream_request_log(state_for_task, log).await;
        drop(tx);
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut res = Response::new(body);
    *res.status_mut() = status;
    *res.headers_mut() = resp_headers;
    res.extensions_mut().insert(VibeLogId(log_id_clone));
    res.extensions_mut().insert(VibeCodexVisual(visual));
    res.extensions_mut()
        .insert(VibeCodexClientKind(codex_client_kind));
    res
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn codex_visual_context(
    provider: &vibe_protocol::Provider,
    credential: Option<&Credential>,
    credential_id: Option<&str>,
    plan_by_cred: &HashMap<String, CredentialPlanSnapshot>,
    requested_model: &str,
    upstream_model: &str,
) -> CodexVisualContext {
    let coding_plan_snapshot = credential_id.and_then(|id| plan_by_cred.get(id).cloned());
    CodexVisualContext {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        credential_id: credential_id.map(str::to_string),
        credential_label: credential.map(|c| c.label.clone()),
        credential_plan_type: credential.and_then(|c| c.plan_type.clone()),
        credential_chatgpt_plan_slug: credential.and_then(|c| c.oauth_chatgpt_plan_slug.clone()),
        requested_model: requested_model.to_string(),
        upstream_model: upstream_model.to_string(),
        coding_plan_snapshot,
        token_plan: credential.and_then(codex_visual::token_plan_from_credential),
    }
}

pub(crate) fn body_wants_stream(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream")?.as_bool())
        .unwrap_or(false)
}

/// Extract a sticky routing key from request headers + body.
/// Only meaningful for the OpenaiResponses wire.
pub(crate) fn codex_sticky_key(wire: Wire, headers: &HeaderMap, body: &[u8]) -> Option<String> {
    if wire != Wire::OpenaiResponses {
        return None;
    }

    fn header_key(headers: &HeaderMap, name: &str) -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("hdr:{name}:{s}"))
    }

    header_key(headers, "thread_id")
        .or_else(|| header_key(headers, "session_id"))
        .or_else(|| selector::sticky_key_from_body(body))
}

pub(crate) fn remember_codex_sticky_route_for_pick(
    state: &AppState,
    sticky_key: Option<&str>,
    provider_id: &str,
    credential_id: Option<&str>,
) {
    let Some(key) = sticky_key else {
        return;
    };
    state.remember_codex_sticky_route(
        key.to_string(),
        CodexStickyRoute {
            provider_id: provider_id.to_string(),
            credential_id: credential_id.map(str::to_string),
        },
        CODEX_STICKY_ROUTE_TTL,
    );
}

pub(crate) fn forget_codex_sticky_route_if_present(state: &AppState, sticky_key: Option<&str>) {
    if let Some(key) = sticky_key {
        state.forget_codex_sticky_route(key);
    }
}

fn find_sse_delimiter(buf: &[u8]) -> Option<(usize, usize)> {
    buf.windows(2)
        .position(|w| w == b"\n\n")
        .map(|pos| (pos, 2))
        .or_else(|| {
            buf.windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|pos| (pos, 4))
        })
}

pub(crate) fn update_peak(slot: &mut Option<f64>, value: Option<f64>) {
    let Some(value) = value else {
        return;
    };
    if !value.is_finite() {
        return;
    }
    match slot {
        Some(current) if *current >= value => {}
        _ => *slot = Some(value),
    }
}

fn parse_sse_block_usage(
    adapter: &(dyn Adapter + Send + Sync),
    wire: Wire,
    event: &str,
    acc: &mut Usage,
) {
    for line in event.lines() {
        adapter.parse_usage_stream_event(wire, line, acc);
    }
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

pub(crate) fn copy_response_headers(src: &reqwest::header::HeaderMap) -> HeaderMap {
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

pub(crate) fn lossy_optional_body(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_log(
    ctx: &LogCtx,
    id: &str,
    started_at: i64,
    started_instant: &Instant,
    app: &Option<String>,
    provider_id: Option<&str>,
    requested_model: &str,
    upstream_model: &str,
    status_code: Option<i32>,
    upstream_http_status: Option<i32>,
    upstream_error_preview: Option<String>,
    error: Option<String>,
    usage: Usage,
    request_body: Option<String>,
    response_body: Option<String>,
) -> RequestLog {
    let mut log = RequestLog {
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
        wire: Some(wire_as_str(ctx.wire).to_string()),
        route_prefix: ctx.route_prefix.clone(),
        credential_id: ctx.credential_id.clone(),
        cb_key: ctx.cb_key.clone(),
        upstream_http_status,
        upstream_error_preview,
        dedupe_key: ctx.dedupe_key.clone(),
        client_transport: ctx.client_transport.clone(),
        request_headers: ctx.request_headers.clone(),
        request_body,
        response_body,
        client_response_body: None,
        stream_kind: None,
        stream_terminal_seen: None,
        stream_end_reason: None,
        stream_error_detail: None,
        upstream_first_byte_ms: None,
        client_first_write_ms: None,
        last_upstream_event_ms: None,
        last_client_write_ms: None,
        upstream_chunk_count: 0,
        upstream_bytes: 0,
        client_chunk_count: 0,
        client_bytes: 0,
        sse_event_count: 0,
        sse_data_count: 0,
        sse_comment_count: 0,
        sse_keepalive_count: 0,
        sse_done_count: 0,
        parse_error_count: 0,
        first_keepalive_ms: None,
        last_keepalive_ms: None,
        max_gap_between_upstream_events_ms: None,
        max_gap_between_data_events_ms: None,
        keepalive_after_last_data_count: 0,
        last_data_event_ms: None,
        bridge_mode: None,
        status_injected: false,
        terminal_injected: false,
        upstream_terminal_type: None,
    };
    empty_stream_fields(&mut log);
    log
}

pub(crate) fn persist_log(state: &AppState, log: RequestLog) {
    state.request_logs.push(log);
}

/// Insert the streaming request log; awaited before dropping the channel so callers can PATCH `client_response_body`.
async fn finalize_stream_request_log(state: AppState, log: RequestLog) {
    state.request_logs.push(log);
}

pub(crate) fn fire_health(
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

// ---------------------------------------------------------------------------
// OAuth token resolution + auto-refresh
// ---------------------------------------------------------------------------

/// Resolve an OAuth credential to a usable access token.
///
/// If the access token is within 60 seconds of expiry (or already expired)
/// **and** a refresh token is stored, this function refreshes via
/// `auth.openai.com/oauth/token`, persists the new tokens to SQLite, and
/// returns the fresh access token.
///
/// On refresh failure the stale access token is returned so the request can
/// still be attempted (the upstream 401 will be handled by the retry loop).
pub(crate) async fn resolve_oauth_token(
    state: &AppState,
    credential_id: Option<&str>,
    oauth: CredOAuth,
) -> anyhow::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let near_expiry = oauth.expires_at.map(|exp| now + 60 >= exp).unwrap_or(false);

    if near_expiry {
        // Load the refresh_token directly from DB (not exposed in Credential read model).
        let refresh_opt = if let Some(cid) = credential_id {
            let cid = cid.to_string();
            let db = state.db.clone();
            tokio::task::spawn_blocking(move || db.credential_get_refresh_token(&cid)).await??
        } else {
            None
        };

        if let Some(refresh_token) = refresh_opt {
            match do_oauth_refresh(&state.http, &refresh_token).await {
                Ok(fresh) => {
                    // Persist new tokens to SQLite (fire-and-forget).
                    if let Some(cid) = credential_id {
                        let db = state.db.clone();
                        let cid = cid.to_string();
                        let (at, rt, exp) = (
                            fresh.access_token.clone(),
                            fresh.refresh_token.clone(),
                            fresh.expires_at,
                        );
                        tokio::task::spawn_blocking(move || {
                            if let Err(e) = db.credential_update_oauth_tokens(&cid, &at, &rt, exp) {
                                tracing::warn!(?e, cred_id = %cid, "failed to persist refreshed OAuth tokens");
                            }
                        });
                    }
                    tracing::info!(cred_id = ?credential_id, "OAuth token refreshed successfully");
                    return Ok(fresh.access_token);
                }
                Err(e) => {
                    tracing::warn!(?e, cred_id = ?credential_id, "OAuth refresh failed, using stale token");
                }
            }
        }
    }

    Ok(oauth.access_token)
}

struct FreshTokens {
    access_token: String,
    refresh_token: String,
    expires_at: Option<i64>,
}

/// POST to `auth.openai.com/oauth/token` with `grant_type=refresh_token`.
async fn do_oauth_refresh(
    client: &reqwest::Client,
    refresh_token: &str,
) -> anyhow::Result<FreshTokens> {
    let resp: serde_json::Value = client
        .post("https://auth.openai.com/oauth/token")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let access_token = resp
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("refresh response missing access_token"))?
        .to_string();

    // OpenAI may or may not rotate the refresh token; keep the old one if not provided.
    let new_refresh = resp
        .get("refresh_token")
        .and_then(|t| t.as_str())
        .unwrap_or(refresh_token)
        .to_string();

    let expires_at = resp
        .get("expires_in")
        .and_then(|e| e.as_u64())
        .map(|secs| chrono::Utc::now().timestamp() + secs as i64);

    Ok(FreshTokens {
        access_token,
        refresh_token: new_refresh,
        expires_at,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn header_value(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).expect("test header value should be valid")
    }

    #[test]
    fn body_wants_stream_only_when_stream_boolean_true() {
        assert!(body_wants_stream(br#"{"model":"gpt-4.1","stream":true}"#));

        assert!(!body_wants_stream(br#"{"model":"gpt-4.1","stream":false}"#));
        assert!(!body_wants_stream(br#"{"model":"gpt-4.1"}"#));
        assert!(!body_wants_stream(br#"{"stream":"true"}"#));
        assert!(!body_wants_stream(b"not json"));
    }

    #[test]
    fn codex_sticky_key_only_applies_to_responses_wire() {
        let mut headers = HeaderMap::new();
        headers.insert("thread_id", header_value("thread-from-header"));
        let body = br#"{"thread_id":"thread-from-body"}"#;

        assert_eq!(
            codex_sticky_key(Wire::OpenaiResponses, &headers, body).as_deref(),
            Some("hdr:thread_id:thread-from-header")
        );
        assert_eq!(codex_sticky_key(Wire::OpenaiChat, &headers, body), None);
        assert_eq!(codex_sticky_key(Wire::Anthropic, &headers, body), None);
    }

    #[test]
    fn codex_sticky_key_prefers_thread_header_then_session_then_body() {
        let body = br#"{"previous_response_id":"resp-123"}"#;

        let mut headers = HeaderMap::new();
        headers.insert("session_id", header_value("session-1"));
        assert_eq!(
            codex_sticky_key(Wire::OpenaiResponses, &headers, body).as_deref(),
            Some("hdr:session_id:session-1")
        );

        headers.insert("thread_id", header_value(" thread-1 "));
        assert_eq!(
            codex_sticky_key(Wire::OpenaiResponses, &headers, body).as_deref(),
            Some("hdr:thread_id:thread-1")
        );

        let headers = HeaderMap::new();
        assert_eq!(
            codex_sticky_key(Wire::OpenaiResponses, &headers, body).as_deref(),
            Some("body:/previous_response_id:resp-123")
        );
    }

    #[test]
    fn update_peak_ignores_none_and_non_finite_and_keeps_maximum() {
        let mut peak = None;

        update_peak(&mut peak, None);
        update_peak(&mut peak, Some(f64::NAN));
        update_peak(&mut peak, Some(f64::INFINITY));
        assert_eq!(peak, None);

        update_peak(&mut peak, Some(7.5));
        assert_eq!(peak, Some(7.5));

        update_peak(&mut peak, Some(3.0));
        assert_eq!(peak, Some(7.5));

        update_peak(&mut peak, Some(8.25));
        assert_eq!(peak, Some(8.25));
    }

    #[test]
    fn find_sse_delimiter_detects_lf_and_crlf_block_boundaries() {
        assert_eq!(find_sse_delimiter(b"data: one\n\nrest"), Some((9, 2)));
        assert_eq!(
            find_sse_delimiter(b"event: message\r\ndata: one\r\n\r\nrest"),
            Some((25, 4))
        );
        assert_eq!(find_sse_delimiter(b"data: partial\n"), None);
        assert_eq!(find_sse_delimiter(b""), None);
    }

    #[test]
    fn find_sse_delimiter_prefers_earliest_lf_boundary() {
        // Current behavior checks LF/LF first, so a later LF delimiter wins even
        // when an earlier CRLF delimiter also exists. This locks in the behavior
        // before splitting the module.
        assert_eq!(
            find_sse_delimiter(b"data: a\r\n\r\nx\ndata: b\n\nrest"),
            Some((20, 2))
        );
    }

    #[test]
    fn extract_model_reads_top_level_string_model_only() {
        assert_eq!(
            extract_model(br#"{"model":"gpt-4.1","input":"hi"}"#).as_deref(),
            Some("gpt-4.1")
        );
        assert_eq!(extract_model(br#"{"model":42}"#), None);
        assert_eq!(extract_model(br#"{"input":"hi"}"#), None);
        assert_eq!(extract_model(b"not json"), None);
    }

    #[test]
    fn detect_app_returns_user_agent_when_utf8() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", header_value("codex-cli/1.2.3"));
        assert_eq!(detect_app(&headers).as_deref(), Some("codex-cli/1.2.3"));

        assert_eq!(detect_app(&HeaderMap::new()), None);
    }

    #[test]
    fn is_hop_header_matches_case_insensitively_and_excludes_end_to_end_headers() {
        assert!(is_hop_header("Connection"));
        assert!(is_hop_header("TRANSFER-ENCODING"));
        assert!(is_hop_header("content-length"));
        assert!(is_hop_header("proxy-authorization"));

        assert!(!is_hop_header("content-type"));
        assert!(!is_hop_header("x-request-id"));
        assert!(!is_hop_header("authorization"));
    }

    #[test]
    fn copy_response_headers_drops_hop_headers_and_keeps_response_metadata() {
        let mut src = reqwest::header::HeaderMap::new();
        src.insert(
            reqwest::header::CONTENT_TYPE,
            header_value("application/json"),
        );
        src.insert(reqwest::header::CONTENT_LENGTH, header_value("123"));
        src.insert(reqwest::header::TRANSFER_ENCODING, header_value("chunked"));
        src.insert("x-request-id", header_value("req-1"));

        let dst = copy_response_headers(&src);

        assert_eq!(
            dst.get("content-type").and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            dst.get("x-request-id").and_then(|v| v.to_str().ok()),
            Some("req-1")
        );
        assert!(!dst.contains_key("content-length"));
        assert!(!dst.contains_key("transfer-encoding"));
    }

    #[test]
    fn parse_duration_secs_accepts_seconds_minutes_and_minute_second_pairs() {
        assert_eq!(parse_duration_secs("3s"), Some(3));
        assert_eq!(parse_duration_secs("90s"), Some(90));
        assert_eq!(parse_duration_secs("1m"), Some(60));
        assert_eq!(parse_duration_secs("1ms"), Some(60));
        assert_eq!(parse_duration_secs("1m30s"), Some(90));
        assert_eq!(parse_duration_secs(" 2m05s "), Some(125));

        assert_eq!(parse_duration_secs("90"), None);
        assert_eq!(parse_duration_secs("1h"), None);
        assert_eq!(parse_duration_secs("m30s"), None);
    }

    #[test]
    fn sanitized_headers_json_redacts_sensitive_headers_and_omits_vibe_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", header_value("Bearer secret"));
        headers.insert("cookie", header_value("sid=secret"));
        headers.insert("x-api-key", header_value("api-secret"));
        headers.insert("x-custom-token", header_value("token-secret"));
        headers.insert("x-vibe-internal", header_value("do-not-log"));
        headers.insert("content-type", header_value("application/json"));

        let json = sanitized_headers_json(&headers, true).expect("headers should be serialized");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("sanitized headers should be valid json");

        assert_eq!(value["authorization"], "<redacted>");
        assert_eq!(value["cookie"], "<redacted>");
        assert_eq!(value["x-api-key"], "<redacted>");
        assert_eq!(value["x-custom-token"], "<redacted>");
        assert_eq!(value["content-type"], "application/json");
        assert!(value.get("x-vibe-internal").is_none());
    }

    #[test]
    fn sanitized_headers_json_can_preserve_sensitive_headers_for_debugging() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", header_value("Bearer visible"));

        let json = sanitized_headers_json(&headers, false).expect("headers should be serialized");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("sanitized headers should be valid json");

        assert_eq!(value["authorization"], "Bearer visible");
    }

    #[test]
    fn sanitized_headers_json_returns_none_for_empty_or_only_vibe_headers() {
        assert_eq!(sanitized_headers_json(&HeaderMap::new(), true), None);

        let mut headers = HeaderMap::new();
        headers.insert("x-vibe-trace", header_value("internal"));
        assert_eq!(sanitized_headers_json(&headers, true), None);
    }
}

// Tests for plan-percent and selector-specific sticky-key logic live in:
//   forward::selector (expand_picks, sticky_key, plan exhaustion)
//   forward::outcome  (classify_retryable)
