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

// ---------------------------------------------------------------------------
// ChatGPT Codex HTTP API: non-empty `instructions`
// ---------------------------------------------------------------------------

/// Codex CLI omits `instructions` when empty (`skip_serializing_if`); ChatGPT's
/// Codex handler returns `{"detail":"Instructions are required"}`. Inject a
/// minimal default only when the field is absent or whitespace-empty.
const CHATGPT_CODEX_FALLBACK_INSTRUCTIONS: &str =
    "You are Codex, OpenAI's coding agent. Help with software engineering tasks using available tools.";

fn inject_chatgpt_codex_instructions_if_missing(
    provider: &vibe_protocol::Provider,
    wire: Wire,
    body: Bytes,
) -> Bytes {
    if wire != Wire::OpenaiResponses || !router::provider_is_chatgpt_codex_official(provider) {
        return body;
    }
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(obj) = v.as_object_mut() else {
        return body;
    };
    let empty = match obj.get("instructions") {
        None => true,
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        Some(_) => false,
    };
    if !empty {
        return body;
    }
    obj.insert(
        "instructions".into(),
        serde_json::Value::String(CHATGPT_CODEX_FALLBACK_INSTRUCTIONS.into()),
    );
    tracing::debug!("injected default instructions for ChatGPT Codex (client omitted empty)");
    serde_json::to_vec(&v).map(Bytes::from).unwrap_or(body)
}

// ---------------------------------------------------------------------------
// Structured logging fields + Codex Plan snapshots from response headers
// ---------------------------------------------------------------------------

/// Per-request metadata persisted alongside [`RequestLog`].
#[derive(Clone)]
pub(crate) struct LogCtx {
    pub wire: Wire,
    pub route_prefix: Option<String>,
    pub credential_id: Option<String>,
    pub cb_key: Option<String>,
    pub dedupe_key: Option<String>,
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

fn dedupe_key_from_headers(headers: &HeaderMap, route_prefix: Option<&str>) -> Option<String> {
    let rid = headers
        .get("x-request-id")
        .or_else(|| headers.get("x-openai-request-id"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(format!("{}|{}", route_prefix.unwrap_or(""), rid))
}

fn maybe_record_codex_plan(
    state: &AppState,
    headers: &HeaderMap,
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
}

/// Expand provider-level picks into credential-level picks.
/// For providers with enabled credentials, each credential becomes one entry.
/// For providers without credentials, a single entry using provider.auth_ref is created.
///
/// Credentials known to be rate-limited (remaining==0 and reset_at is in the future)
/// are moved to the end so they're only tried if every other credential is exhausted.
fn expand_picks(
    picks: Vec<router::Pick>,
    creds_by_provider: &HashMap<String, Vec<Credential>>,
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
                        (None, Some(CredOAuth {
                            access_token: c.oauth_access_token.clone().unwrap(),
                            expires_at: c.oauth_expires_at,
                        }))
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
                    };
                    // Proactively defer credentials whose remaining quota is zero and
                    // whose reset time hasn't arrived yet.
                    if cred_is_rate_limited(c, now) {
                        tracing::debug!(
                            cred_id = %c.id, label = %c.label,
                            reqs_remaining = ?c.rl_requests_remaining,
                            tokens_remaining = ?c.rl_tokens_remaining,
                            "deferring rate-limited credential"
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
    let tok_exhausted = c.rl_tokens_remaining == Some(0)
        && c.rl_tokens_reset_at.map_or(false, |r| r > now_secs);
    req_exhausted || tok_exhausted
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
        if let Ok(n) = v.parse::<i64>() { return Some(n); }
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
                rl.requests_limit, rl.requests_remaining, rl.requests_reset_at,
                rl.tokens_limit, rl.tokens_remaining, rl.tokens_reset_at,
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
        transforms::strip_ws_envelope(&body)
    } else {
        body
    };

    let request_snapshot = lossy_optional_body(&body);

    let started_at = chrono::Utc::now().timestamp();
    let started_instant = Instant::now();
    let log_id = uuid::Uuid::new_v4().to_string();
    let app = detect_app(&req_headers);
    let dedupe_key = dedupe_key_from_headers(&req_headers, route_prefix.as_deref());
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
    let candidates = rotate_candidates(
        router::candidates(&providers_list, wire, &requested_model),
        counter,
        &state.cb,
    );
    if candidates.is_empty() {
        let log_ctx = LogCtx {
            wire,
            route_prefix: route_prefix.clone(),
            credential_id: None,
            cb_key: None,
            dedupe_key: dedupe_key.clone(),
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
        return (StatusCode::SERVICE_UNAVAILABLE, "no provider matches request shape")
            .into_response();
    }

    let expanded_picks = expand_picks(candidates, &creds_by_provider, counter);
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
        };

        // ── circuit breaker ──────────────────────────────────────────────
        if !state.cb.allow(&cb_key) {
            tracing::debug!(cb_key = %cb_key, "circuit open, skipping");
            cb_skipped_total += 1;
            if !cb_skipped_provider_ids.iter().any(|pid| pid == &provider.id) {
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
                        fire_credential_failure(&state, cid.clone(), Some(format!("oauth refresh failed: {e}")));
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
                    } else if let Some(auth) = req_headers.get("authorization").and_then(|v| v.to_str().ok()) {
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
                            fire_credential_failure(&state, cid.clone(), Some(format!("auth resolve failed: {e}")));
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
        let effective_body: Bytes = if provider.kind == ProviderKind::Anthropic
            && state.config.failover.inject_cache
        {
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
                tracing::warn!(provider_id = %provider.id, cb_key = %cb_key, error = %msg, "upstream connection error");
                state.cb.record_failure(&cb_key);
                fire_health(&state, &provider.id, false,
                    started_instant.elapsed().as_millis() as i64, Some(msg.clone()));
                if let Some(cid) = &epick.credential_id {
                    fire_credential_failure(&state, cid.clone(), Some(msg.clone()));
                }
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
            state.cb.record_failure(&cb_key);
            fire_health(&state, &provider.id, false,
                started_instant.elapsed().as_millis() as i64, Some(format!("HTTP {status}")));
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
            fire_health(&state, &provider.id, false,
                started_instant.elapsed().as_millis() as i64,
                Some(format!("client error {status}")));
            if let Some(cid) = &epick.credential_id {
                fire_credential_failure(&state, cid.clone(), Some(format!("HTTP {status}")));
            }
            let resp_headers = copy_response_headers(upstream.headers());
            let buf = upstream.bytes().await.unwrap_or_default();
            let err_stored = lossy_optional_body(&buf);
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
                lossy_optional_body(&buf),
            );
            persist_log(&state, log);
            return (status, resp_headers, buf).into_response();
        }

        // ── 2xx success ───────────────────────────────────────────────────
        state.cb.record_success(&cb_key);
        fire_health(&state, &provider.id, true,
            started_instant.elapsed().as_millis() as i64, None);

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
            lossy_optional_body(&buf),
        );
        let client_body = if do_c2r {
            let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
            let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());
            let converted = transforms::chat_body_to_responses(&buf, &session_id, &item_id);
            log.client_response_body = lossy_optional_body(&converted);
            converted
        } else {
            buf.clone()
        };
        persist_log(&state, log);
        return (status, resp_headers, client_body).into_response();
    }

    // All candidates exhausted
    let log_ctx = LogCtx {
        wire,
        route_prefix: route_prefix.clone(),
        credential_id: None,
        cb_key: None,
        dedupe_key: dedupe_key.clone(),
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
) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);
    let state_for_task = state.clone();
    let log_id_clone = log_id.clone();

    tokio::spawn(async move {
        let mut byte_stream = upstream.bytes_stream();
        let mut acc = Usage::default();
        let mut first_token_ms: Option<i64> = None;
        let mut buf = String::new();
        let mut raw_accum: Vec<u8> = Vec::new();

        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    raw_accum.extend_from_slice(&bytes);
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

        let sc = status.as_u16() as i32;
        let response_body = lossy_optional_body(&raw_accum);
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
        finalize_stream_request_log(state_for_task, log).await;
        drop(tx);
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    let mut res = Response::new(body);
    *res.status_mut() = status;
    *res.headers_mut() = resp_headers;
    res.extensions_mut().insert(VibeLogId(log_id_clone));
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
        wire: Some(wire_as_str(ctx.wire).to_string()),
        route_prefix: ctx.route_prefix.clone(),
        credential_id: ctx.credential_id.clone(),
        cb_key: ctx.cb_key.clone(),
        upstream_http_status,
        upstream_error_preview,
        dedupe_key: ctx.dedupe_key.clone(),
        request_body,
        response_body,
        client_response_body: None,
    }
}

fn persist_log(state: &AppState, log: RequestLog) {
    let db = state.db.clone();
    let ws = state.ws.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = db.log_insert(&log) {
            tracing::warn!(?e, "failed to insert request log");
        }
        let mut thin = log;
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
            tokio::task::spawn_blocking(move || db.credential_get_refresh_token(&cid))
                .await??
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
                        let (at, rt, exp) = (fresh.access_token.clone(), fresh.refresh_token.clone(), fresh.expires_at);
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
async fn do_oauth_refresh(client: &reqwest::Client, refresh_token: &str) -> anyhow::Result<FreshTokens> {
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

    Ok(FreshTokens { access_token, refresh_token: new_refresh, expires_at })
}
