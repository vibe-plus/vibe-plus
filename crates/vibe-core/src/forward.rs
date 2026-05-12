//! Request forwarding with retry loop, circuit-breaker integration, and streaming.
//!
//! Strategy:
//! 1. Build an ordered candidate list via `router::candidates`.
//! 2. For each candidate, check the circuit breaker — skip if Open.
//! 3. Optionally inject Anthropic cache_control into the body.
//! 4. Send the request.
//!    - Connection error / 5xx / 401 / 402 → record failure, try next provider.
//!    - 429 (quota / rate-limit) → rotate credential; **do not** trip circuit breaker.
//!    - Other 4xx                  → return immediately (caller's fault); no breaker trip.
//!    - 2xx                        → record success, stream or buffer response.
//! 5. If every candidate is exhausted, return 503.

use crate::cache;
use crate::circuit_breaker::CircuitBreakers;
use crate::claude_control::ClaudeRouteScenario;
use crate::claude_summary::ClaudeClientKind;
use crate::codex_summary::CodexClientKind;
use crate::codex_visual::{self, CodexVisualContext};
use crate::providers::{self, Adapter, Wire};
use crate::state::AppState;
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
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use vibe_protocol::{Credential, CredentialPlanSnapshot, ProviderKind, RequestLog, WsEvent};

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

fn wire_as_str(wire: Wire) -> &'static str {
    match wire {
        Wire::Anthropic => "anthropic",
        Wire::OpenaiChat => "openai-chat",
        Wire::OpenaiResponses => "openai-responses",
        Wire::GeminiNative => "gemini-native",
    }
}

fn needs_chat_to_responses_bridge(wire: Wire, provider_kind: ProviderKind) -> bool {
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
    let mut body = if wire == Wire::OpenaiResponses && !preserve_ws_envelope {
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
    let mut claude_selection_model = requested_model.clone();
    let mut claude_route_model: Option<String> = None;
    let mut claude_scenario = ClaudeRouteScenario::Default;
    if wire == Wire::Anthropic {
        let claude_cfg = state.claude_config();
        let prepared = crate::claude_control::prepare_request(
            body,
            requested_model.clone(),
            &claude_cfg.routing,
            &claude_cfg.request,
        );
        body = prepared.body;
        claude_selection_model = prepared.requested_model;
        claude_route_model = prepared.route_model;
        claude_scenario = prepared.scenario;
    }

    let request_snapshot = if state.config.log.bodies {
        lossy_optional_body(&body)
    } else {
        None
    };

    let providers_list = state
        .db
        .provider_list()
        .map_err(|e| PreparedForwardError::Db(format!("db error: {e}")))?;
    let routes = state.db.route_list().unwrap_or_default();

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
    let (_route, routed_candidates) = if wire == Wire::Anthropic {
        let claude_cfg = state.claude_config();
        crate::claude_control::candidates_for_request(
            &providers_list,
            &routes,
            wire,
            &claude_selection_model,
            claude_route_model.as_deref(),
            claude_scenario,
            &claude_cfg.fallback,
        )
    } else {
        router::candidates_with_routes(&providers_list, &routes, wire, &requested_model)
    };
    let candidates = rotate_candidates(routed_candidates, counter, &state.cb);
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

    let expanded_picks = expand_picks(candidates, &creds_by_provider, &plan_by_cred, counter);
    let mut last_error = String::from("all providers unavailable or circuit open");
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
                    state.cb.record_failure(&cb_key);
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
                        state.cb.record_failure(&cb_key);
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
                    last_error = format!("body rewrite: {e}");
                    continue;
                }
            }
        } else {
            match adapter.rewrite_body_model(&effective_body, &upstream_model) {
                Ok(b) => b,
                Err(e) => {
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
        });
    }

    let final_error = if attempted_after_cb == 0 && cb_skipped_total > 0 {
        let preview = if cb_skipped_provider_ids.is_empty() {
            String::new()
        } else {
            let ids = cb_skipped_provider_ids
                .iter()
                .take(6)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!(", providers=[{ids}]")
        };
        format!(
            "all providers blocked by circuit breaker ({cb_skipped_total} skipped{preview}). reset via POST /_vp/providers/:id/circuit/reset or Providers UI"
        )
    } else {
        last_error
    };

    Err(PreparedForwardError::Exhausted {
        log_id,
        started_at,
        started_instant,
        app,
        requested_model,
        log_ctx: empty_log_ctx,
        request_snapshot,
        message: final_error,
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

fn sanitized_headers_json(headers: &HeaderMap, redact_sensitive: bool) -> Option<String> {
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

fn maybe_record_codex_plan(
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

// ---------------------------------------------------------------------------
// Credential-expanded pick
// ---------------------------------------------------------------------------

/// OAuth tokens carried by an ExpandedPick when a credential uses direct storage.
#[derive(Clone)]
pub(crate) struct CredOAuth {
    pub(crate) access_token: String,
    pub(crate) expires_at: Option<i64>,
}

/// A single tryable unit: one provider + one auth source (credential or provider fallback).
struct ExpandedPick {
    provider: vibe_protocol::Provider,
    upstream_model: String,
    /// Circuit-breaker key: credential.id if using a credential, else provider.id.
    cb_key: String,
    /// auth_ref scheme (keyring:, env:, …). Mutually exclusive with `oauth`.
    auth_ref: Option<String>,
    /// OAuth direct-storage tokens. Mutually exclusive with `auth_ref`.
    oauth: Option<CredOAuth>,
    /// Set when auth comes from a credential row (for post-request RL update).
    credential_id: Option<String>,
    credential: Option<Credential>,
}

/// Expand provider-level picks into credential-level picks.
/// For providers with enabled credentials, each credential becomes one entry.
/// For providers without credentials, a single entry using provider.auth_ref is created.
///
/// Credentials known to be rate-limited (remaining==0 and reset_at is in the future),
/// or ChatGPT Codex plan snapshots showing **100%** on the UI-primary window (same order as
/// `primaryPlanPercent`: primary → 5h → 7d), are moved to the end so upstream is not hit only to 429.
fn expand_picks(
    picks: Vec<router::Pick>,
    creds_by_provider: &HashMap<String, Vec<Credential>>,
    plan_by_cred: &HashMap<String, CredentialPlanSnapshot>,
    counter: usize,
) -> Vec<ExpandedPick> {
    let now = chrono::Utc::now().timestamp();
    let mut out = Vec::new();
    let mut deferred: Vec<ExpandedPick> = Vec::new(); // rate-limited, try last

    for pick in picks {
        let creds = creds_by_provider
            .get(&pick.provider.id)
            .filter(|v| !v.is_empty());
        match creds {
            Some(creds) => {
                let n = creds.len();
                let start = if n > 0 { counter % n } else { 0 };
                for i in 0..n {
                    let c = &creds[(start + i) % n];
                    let (auth_ref, oauth) = if c.oauth_access_token.is_some() {
                        (
                            None,
                            Some(CredOAuth {
                                access_token: c.oauth_access_token.clone().unwrap(),
                                expires_at: c.oauth_expires_at,
                            }),
                        )
                    } else {
                        (c.auth_ref.clone(), None)
                    };
                    let epick = ExpandedPick {
                        provider: pick.provider.clone(),
                        upstream_model: pick.upstream_model.clone(),
                        cb_key: c.id.clone(),
                        auth_ref,
                        oauth,
                        credential_id: Some(c.id.clone()),
                        credential: Some(c.clone()),
                    };
                    // Proactively defer credentials whose remaining quota is zero and
                    // whose reset time hasn't arrived yet, or Codex plan snapshot at 100%.
                    let defer_plan = router::provider_is_chatgpt_codex_official(&pick.provider)
                        && plan_by_cred
                            .get(&c.id)
                            .is_some_and(credential_plan_display_exhausted);
                    if cred_is_rate_limited(c, now) || defer_plan {
                        tracing::debug!(
                            cred_id = %c.id, label = %c.label,
                            reqs_remaining = ?c.rl_requests_remaining,
                            tokens_remaining = ?c.rl_tokens_remaining,
                            defer_plan,
                            "deferring credential (rate-limit state or exhausted Codex plan snapshot)"
                        );
                        deferred.push(epick);
                    } else {
                        out.push(epick);
                    }
                }
            }
            None => {
                out.push(ExpandedPick {
                    cb_key: pick.provider.id.clone(),
                    auth_ref: pick.provider.auth_ref.clone(),
                    oauth: None,
                    provider: pick.provider,
                    upstream_model: pick.upstream_model,
                    credential_id: None,
                    credential: None,
                });
            }
        }
    }
    // Append deferred (rate-limited) picks at the end so they're used as a last resort.
    out.extend(deferred);
    out
}

/// Returns true if a credential is known to be rate-limited based on stored RL state.
/// Conditions: (requests_remaining == 0 AND reset not yet due)
///          OR (tokens_remaining == 0 AND reset not yet due)
fn cred_is_rate_limited(c: &Credential, now_secs: i64) -> bool {
    let req_exhausted = c.rl_requests_remaining == Some(0)
        && c.rl_requests_reset_at.map_or(false, |r| r > now_secs);
    let tok_exhausted =
        c.rl_tokens_remaining == Some(0) && c.rl_tokens_reset_at.map_or(false, |r| r > now_secs);
    req_exhausted || tok_exhausted
}

/// Matches website `primaryPlanPercent`: primary → 5h → 7d, clamped to `[0, 100]`.
fn credential_plan_display_percent(snap: &CredentialPlanSnapshot) -> Option<f64> {
    fn clamp_pct(v: Option<f64>) -> Option<f64> {
        let v = v?;
        if v.is_nan() {
            return None;
        }
        Some(v.clamp(0.0, 100.0))
    }
    clamp_pct(snap.codex_primary_used_percent)
        .or_else(|| clamp_pct(snap.codex_5h_used_percent))
        .or_else(|| clamp_pct(snap.codex_7d_used_percent))
}

fn credential_plan_display_exhausted(snap: &CredentialPlanSnapshot) -> bool {
    credential_plan_display_percent(snap).is_some_and(|p| p >= 100.0)
}

// ---------------------------------------------------------------------------
// Rate-limit header extraction
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RlHeaders {
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

fn extract_rl_headers(headers: &reqwest::header::HeaderMap) -> RlHeaders {
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

    RlHeaders {
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
    }
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

fn fire_credential_success(state: &AppState, credential_id: String, rl: RlHeaders) {
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

fn fire_credential_failure(state: &AppState, credential_id: String, error: Option<String>) {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _ = db.credential_record_failure(&credential_id, error.as_deref());
    });
}

/// Persist RL headers from a non-success response without counting success/failure streaks.
fn fire_credential_rate_limit_only(state: &AppState, credential_id: String, rl: RlHeaders) {
    if rl.is_empty() {
        return;
    }
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let _ = db.credential_update_rate_limit(
            &credential_id,
            rl.requests_limit,
            rl.requests_remaining,
            rl.requests_reset_at,
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
    let mut body = body;
    let mut claude_selection_model = requested_model.clone();
    let mut claude_route_model: Option<String> = None;
    let mut claude_scenario = ClaudeRouteScenario::Default;
    if wire == Wire::Anthropic {
        let claude_cfg = state.claude_config();
        let prepared = crate::claude_control::prepare_request(
            body,
            requested_model.clone(),
            &claude_cfg.routing,
            &claude_cfg.request,
        );
        body = prepared.body;
        claude_selection_model = prepared.requested_model;
        claude_route_model = prepared.route_model;
        claude_scenario = prepared.scenario;
    }

    let request_snapshot = if state.config.log.bodies {
        lossy_optional_body(&body)
    } else {
        None
    };

    let providers_list = match state.db.provider_list() {
        Ok(v) => v,
        Err(e) => return internal_error(format!("db error: {e}")),
    };
    let routes = state.db.route_list().unwrap_or_default();

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
    let (_route, routed_candidates) = if wire == Wire::Anthropic {
        let claude_cfg = state.claude_config();
        crate::claude_control::candidates_for_request(
            &providers_list,
            &routes,
            wire,
            &claude_selection_model,
            claude_route_model.as_deref(),
            claude_scenario,
            &claude_cfg.fallback,
        )
    } else {
        router::candidates_with_routes(&providers_list, &routes, wire, &requested_model)
    };
    let candidates = rotate_candidates(routed_candidates, counter, &state.cb);
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
            Some("no provider matches request shape".into()),
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

    let expanded_picks = expand_picks(candidates, &creds_by_provider, &plan_by_cred, counter);
    let mut last_error = String::from("all providers unavailable or circuit open");
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

        // ── circuit breaker ──────────────────────────────────────────────
        if !state.cb.allow(&cb_key) {
            tracing::debug!(cb_key = %cb_key, "circuit open, skipping");
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

        // ── auth ─────────────────────────────────────────────────────────
        let secret = if let Some(oauth) = epick.oauth.take() {
            // OAuth credential: tokens stored in SQLite, auto-refresh if near-expiry.
            match resolve_oauth_token(&state, epick.credential_id.as_deref(), oauth).await {
                Ok(t) => Some(t),
                Err(e) => {
                    state.cb.record_failure(&cb_key);
                    if let Some(cid) = &epick.credential_id {
                        fire_credential_failure(
                            &state,
                            cid.clone(),
                            Some(format!("oauth refresh failed: {e}")),
                        );
                    }
                    fire_health(
                        &state,
                        &provider.id,
                        false,
                        started_instant.elapsed().as_millis() as i64,
                        Some("oauth refresh failed".into()),
                    );
                    last_error = format!("oauth error for {}: {e}", provider.id);
                    continue;
                }
            }
        } else {
            // auth_ref scheme or passthrough.
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
                        state.cb.record_failure(&cb_key);
                        if let Some(cid) = &epick.credential_id {
                            fire_credential_failure(
                                &state,
                                cid.clone(),
                                Some(format!("auth resolve failed: {e}")),
                            );
                        }
                        fire_health(
                            &state,
                            &provider.id,
                            false,
                            started_instant.elapsed().as_millis() as i64,
                            Some("auth resolve failed".into()),
                        );
                        last_error = format!("auth error for {}: {e}", provider.id);
                        continue;
                    }
                },
                None => None,
            }
        };

        // ── cache injection (Anthropic only) ─────────────────────────────
        let effective_body: Bytes =
            if provider.kind == ProviderKind::Anthropic && state.config.failover.inject_cache {
                cache::inject(&body)
            } else {
                body.clone()
            };

        let effective_body =
            inject_chatgpt_codex_instructions_if_missing(&provider, wire, effective_body);

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
        let req = match adapter.build(
            &provider,
            secret.as_deref(),
            &state.http,
            wire,
            &body_up,
            upstream_path.as_deref(),
        ) {
            Ok(r) => r,
            Err(e) => {
                last_error = e.to_string();
                continue;
            }
        };
        let req = if wire == Wire::Anthropic {
            let timeout_ms = state.claude_config().request.api_timeout_ms.max(1);
            req.timeout(std::time::Duration::from_millis(timeout_ms))
        } else {
            req
        };

        // ── send ──────────────────────────────────────────────────────────
        let upstream = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                tracing::warn!(provider_id = %provider.id, cb_key = %cb_key, error = %msg, "upstream connection error");
                state.cb.record_failure(&cb_key);
                fire_health(
                    &state,
                    &provider.id,
                    false,
                    started_instant.elapsed().as_millis() as i64,
                    Some(msg.clone()),
                );
                if let Some(cid) = &epick.credential_id {
                    fire_credential_failure(&state, cid.clone(), Some(msg.clone()));
                }
                last_error = format!("connection to {}: {msg}", provider.id);
                continue;
            }
        };

        let status = upstream.status();

        // ── retryable errors ──────────────────────────────────────────────
        // 429: quota / rate-limit → rotate credential; do **not** trip circuit breaker.
        // 5xx / 401 / 402: record failure and try next pick.
        if status.is_server_error()
            || status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::UNAUTHORIZED
            || status == StatusCode::PAYMENT_REQUIRED
        {
            let headers = upstream.headers().clone();
            let body_bytes = upstream.bytes().await.unwrap_or_default();
            let msg = format!("upstream {} from {}", status, provider.id);

            if status == StatusCode::TOO_MANY_REQUESTS {
                tracing::warn!(
                    %msg,
                    bytes = body_bytes.len(),
                    "upstream 429; rotating credential without circuit breaker trip"
                );
                maybe_record_codex_plan(
                    &state,
                    &headers,
                    &provider,
                    epick.credential_id.as_deref(),
                );
                let rl = extract_rl_headers(&headers);
                if let Some(cid) = &epick.credential_id {
                    fire_credential_rate_limit_only(&state, cid.clone(), rl);
                }
                fire_health(
                    &state,
                    &provider.id,
                    false,
                    started_instant.elapsed().as_millis() as i64,
                    Some(format!("HTTP {status}")),
                );
                last_error = msg;
                continue;
            }

            tracing::warn!(
                %msg,
                error_body_bytes = body_bytes.len(),
                "retryable upstream error, trying next provider"
            );
            state.cb.record_failure(&cb_key);
            fire_health(
                &state,
                &provider.id,
                false,
                started_instant.elapsed().as_millis() as i64,
                Some(format!("HTTP {status}")),
            );
            if let Some(cid) = &epick.credential_id {
                fire_credential_failure(&state, cid.clone(), Some(format!("HTTP {status}")));
            }
            last_error = msg;
            continue;
        }

        // ── non-retryable client error (400/403/404 etc.) ────────────────
        // These indicate a malformed request or forbidden resource — not a
        // provider-specific issue, so retrying another provider won't help.
        // Do **not** trip the circuit breaker: bursts of 400 (e.g. bad bodies from one client)
        // would otherwise open the breaker and cause unrelated requests to see 503 when every
        // pick is CB-skipped.
        if status.is_client_error() {
            fire_health(
                &state,
                &provider.id,
                false,
                started_instant.elapsed().as_millis() as i64,
                Some(format!("client error {status}")),
            );
            if let Some(cid) = &epick.credential_id {
                fire_credential_failure(&state, cid.clone(), Some(format!("HTTP {status}")));
            }
            let resp_headers = copy_response_headers(upstream.headers());
            let buf = upstream.bytes().await.unwrap_or_default();
            let err_stored = if state.config.log.bodies {
                lossy_optional_body(&buf)
            } else {
                None
            };
            tracing::warn!(
                provider_id = %provider.id,
                status = %status,
                body_bytes = buf.len(),
                "non-retryable upstream error (4xx); full body stored in request_logs.response_body"
            );
            let sc = status.as_u16() as i32;
            let log = build_log(
                &log_ctx,
                &log_id,
                started_at,
                &started_instant,
                &app,
                Some(&provider.id),
                &requested_model,
                &upstream_model,
                Some(sc),
                Some(sc),
                err_stored.clone(),
                Some(format!("client error {status}")),
                Usage::default(),
                request_snapshot.clone(),
                if state.config.log.bodies {
                    lossy_optional_body(&buf)
                } else {
                    None
                },
            );
            persist_log(&state, log);
            return (status, resp_headers, buf).into_response();
        }

        // ── 2xx success ───────────────────────────────────────────────────
        state.cb.record_success(&cb_key);
        fire_health(
            &state,
            &provider.id,
            true,
            started_instant.elapsed().as_millis() as i64,
            None,
        );

        // Extract rate-limit headers before consuming the response.
        let rl = extract_rl_headers(upstream.headers());
        if let Some(cid) = &epick.credential_id {
            fire_credential_success(&state, cid.clone(), rl);
        }

        maybe_record_codex_plan(
            &state,
            upstream.headers(),
            &provider,
            epick.credential_id.as_deref(),
        );

        let resp_headers = copy_response_headers(upstream.headers());
        let visual = codex_visual_context(
            &provider,
            epick.credential.as_ref(),
            epick.credential_id.as_deref(),
            &plan_by_cred,
            &requested_model,
            &upstream_model,
        );

        if body_wants_stream(&body) {
            return stream_response(
                state,
                adapter,
                wire,
                upstream,
                status,
                resp_headers,
                log_id,
                started_at,
                started_instant,
                app,
                provider.id.clone(),
                requested_model,
                upstream_model,
                log_ctx,
                request_snapshot,
                visual,
            );
        }

        let buf = match upstream.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let log = build_log(
                    &log_ctx,
                    &log_id,
                    started_at,
                    &started_instant,
                    &app,
                    Some(&provider.id),
                    &requested_model,
                    &upstream_model,
                    Some(502),
                    Some(502),
                    None,
                    Some(format!("read upstream: {e}")),
                    Usage::default(),
                    request_snapshot.clone(),
                    None,
                );
                persist_log(&state, log);
                return (StatusCode::BAD_GATEWAY, format!("read upstream: {e}")).into_response();
            }
        };

        let usage = adapter.parse_usage_body(wire, &buf);
        let sc = status.as_u16() as i32;
        let do_c2r = needs_chat_to_responses_bridge(wire, provider.kind);
        let mut client_body = buf.clone();
        if wire == Wire::Anthropic {
            let metrics = crate::codex_summary::SummaryMetrics::from_usage(
                usage,
                Some(started_instant.elapsed().as_millis() as i64),
                None,
            );
            if let Some(with_summary) = crate::claude_summary::append_summary_to_message_body(
                &client_body,
                &state.claude_summary_config(),
                log_ctx.claude_client_kind,
                metrics,
            ) {
                client_body = Bytes::from(with_summary);
            }
        }
        let mut log = build_log(
            &log_ctx,
            &log_id,
            started_at,
            &started_instant,
            &app,
            Some(&provider.id),
            &requested_model,
            &upstream_model,
            Some(sc),
            Some(sc),
            None,
            None,
            usage,
            request_snapshot.clone(),
            if state.config.log.bodies {
                lossy_optional_body(&buf)
            } else {
                None
            },
        );
        let client_body = if do_c2r {
            let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
            let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());
            let converted = transforms::chat_body_to_responses(&client_body, &session_id, &item_id);
            if state.config.log.bodies {
                log.client_response_body = lossy_optional_body(&converted);
            }
            converted
        } else {
            if state.config.log.bodies && client_body != buf {
                log.client_response_body = lossy_optional_body(&client_body);
            }
            client_body
        };
        let client_body = if route_prefix.as_deref() == Some("codex-v1")
            && wire == Wire::OpenaiResponses
            && transforms::responses_input_ends_with_user_message(&body)
        {
            let status = codex_visual::status_message_text(
                &visual,
                started_instant.elapsed().as_millis() as i64,
            );
            let item_id = format!("vibe_route_{}", uuid::Uuid::new_v4().simple());
            let with_status = transforms::prepend_response_message(&client_body, &item_id, &status);
            if state.config.log.bodies {
                log.client_response_body = lossy_optional_body(&with_status);
            }
            with_status
        } else {
            client_body
        };
        persist_log(&state, log);
        let mut response = (status, resp_headers, client_body).into_response();
        response.extensions_mut().insert(VibeCodexVisual(visual));
        response
            .extensions_mut()
            .insert(VibeCodexClientKind(log_ctx.codex_client_kind));
        return response;
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
        Some(last_error.clone()),
        Usage::default(),
        request_snapshot.clone(),
        None,
    );
    persist_log(&state, log);
    let final_error = if attempted_after_cb == 0 && cb_skipped_total > 0 {
        let preview = if cb_skipped_provider_ids.is_empty() {
            String::new()
        } else {
            let ids = cb_skipped_provider_ids
                .iter()
                .take(6)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!(", providers=[{ids}]")
        };
        format!(
            "all providers blocked by circuit breaker ({cb_skipped_total} skipped{preview}). reset via POST /_vp/providers/:id/circuit/reset or Providers UI"
        )
    } else {
        last_error
    };
    (StatusCode::SERVICE_UNAVAILABLE, final_error).into_response()
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
        let response_body = if state_for_task.config.log.bodies {
            lossy_optional_body(&raw_accum)
        } else {
            None
        };
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
            + candidates[group_start..].partition_point(|p| p.provider.priority == priority);

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

fn codex_visual_context(
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

fn body_wants_stream(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream")?.as_bool())
        .unwrap_or(false)
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

fn lossy_optional_body(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[allow(clippy::too_many_arguments)]
fn build_log(
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

fn persist_log(state: &AppState, log: RequestLog) {
    let db = state.db.clone();
    let ws = state.ws.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = db.log_insert(&log) {
            tracing::warn!(?e, "failed to insert request log");
        }
        let mut thin = log;
        thin.request_headers = None;
        thin.request_body = None;
        thin.response_body = None;
        thin.client_response_body = None;
        ws.publish(WsEvent::LogAppended(thin));
    });
}

/// Insert row + WS notify, awaited before dropping the streaming channel so callers can PATCH `client_response_body`.
async fn finalize_stream_request_log(state: AppState, log: RequestLog) {
    let db = state.db.clone();
    let ws = state.ws.clone();
    let log_insert = log.clone();
    match tokio::task::spawn_blocking(move || db.log_insert(&log_insert)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(?e, "failed to insert stream request log"),
        Err(join_err) => tracing::warn!(%join_err, "stream log insert task panicked"),
    }
    let mut thin = log;
    thin.request_headers = None;
    thin.request_body = None;
    thin.response_body = None;
    thin.client_response_body = None;
    ws.publish(WsEvent::LogAppended(thin));
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

#[cfg(test)]
mod plan_pct_tests {
    use super::{credential_plan_display_exhausted, credential_plan_display_percent};
    use vibe_protocol::CredentialPlanSnapshot;

    fn snap(pp: Option<f64>, h5: Option<f64>, h7: Option<f64>) -> CredentialPlanSnapshot {
        CredentialPlanSnapshot {
            id: "i".into(),
            credential_id: "c".into(),
            captured_at: 0,
            codex_5h_used_percent: h5,
            codex_7d_used_percent: h7,
            codex_5h_reset_after_seconds: None,
            codex_7d_reset_after_seconds: None,
            codex_primary_used_percent: pp,
            codex_secondary_used_percent: None,
            summary: None,
            source: "t".into(),
        }
    }

    #[test]
    fn display_pct_follows_primary_then_5h_then_7d() {
        assert_eq!(
            credential_plan_display_percent(&snap(Some(12.0), Some(100.0), None)),
            Some(12.0)
        );
        assert_eq!(
            credential_plan_display_percent(&snap(None, Some(100.0), Some(50.0))),
            Some(100.0)
        );
    }

    #[test]
    fn exhausted_when_display_hundred() {
        assert!(credential_plan_display_exhausted(&snap(
            None,
            Some(100.0),
            None
        )));
        assert!(!credential_plan_display_exhausted(&snap(
            None,
            Some(99.0),
            None
        )));
    }
}
