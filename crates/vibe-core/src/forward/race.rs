//! Fanout-race forwarding harness (Phase 3).
//!
//! When a matched [`vibe_protocol::Route`] is set to
//! [`vibe_protocol::ForwardStrategy::Race`], the gateway should:
//!
//! 1. Take the first `Route::fanout_n` expanded credential picks.
//! 2. Fire each upstream request concurrently via `tokio::spawn`.
//! 3. The first racer to return a terminal response (2xx or non-retryable 4xx)
//!    wins; its response is streamed/buffered back to the downstream client.
//! 4. Losers are aborted via a shared [`tokio_util::sync::CancellationToken`]
//!    and logged with [`UpstreamAttemptOutcome::RaceAborted`].
//! 5. If every racer returns retryable failures, fall through to the
//!    remaining picks sequentially.

use super::outcome;
use super::selector::ExpandedPick;
use super::{
    attempt_log_from_parts, body_wants_stream, build_log, codex_visual_context,
    copy_response_headers, emit_circuit_event, extract_rl_headers, fire_credential_failure,
    fire_credential_rate_limit_only, fire_credential_success, fire_health,
    forget_codex_sticky_route_if_present, format_routing_attempt,
    inject_chatgpt_codex_instructions_if_missing, lossy_optional_body, maybe_record_codex_plan,
    needs_chat_to_responses_bridge, new_attempt_ctx, persist_log, persist_upstream_attempt_log,
    publish_upstream_attempt_started, remember_codex_sticky_route_for_pick, resolve_oauth_token,
    sanitized_headers_json, stream_response, LogCtx, VibeCodexClientKind, VibeCodexVisual,
};
use crate::cache;
use crate::claude_summary::ClaudeClientKind;
use crate::codex_summary::CodexClientKind;
use crate::providers::{self, Wire};
use crate::secrets;
use crate::state::AppState;
use crate::transforms;
use crate::usage::Usage;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use vibe_protocol::{
    CredentialPlanSnapshot, ProviderKind, UpstreamAttemptOutcome, UpstreamAttemptPhase,
};

/// Per-request data shared across all racers and the sequential fallback.
///
/// All fields are owned (or `Arc`-wrapped) so a `PickCtx` can be cloned cheaply
/// into spawned racer tasks without lifetime entanglement.
#[derive(Clone)]
pub struct PickCtx {
    pub wire: Wire,
    pub route_prefix: Option<String>,
    pub log_id: String,
    pub started_at: i64,
    pub started_instant: Instant,
    pub app: Option<String>,
    pub requested_model: String,
    pub upstream_path: Option<String>,
    pub dedupe_key: Option<String>,
    pub client_transport: Option<String>,
    pub request_headers_json: Option<String>,
    pub codex_client_kind: CodexClientKind,
    pub claude_client_kind: ClaudeClientKind,
    pub body: Bytes,
    pub req_headers: HeaderMap,
    pub request_snapshot: Option<String>,
    pub sticky_key: Option<String>,
    pub plan_by_cred: Arc<HashMap<String, CredentialPlanSnapshot>>,
}

/// Outcome of attempting a single credential pick.
pub enum PickResult {
    /// Terminal — propagate to the downstream client unchanged.
    /// Includes both 2xx success (streaming or buffered) and non-retryable
    /// 4xx errors (caller's fault — don't try another upstream).
    Final(Response),
    /// Retryable — move on to the next pick (or, in race mode, wait for
    /// another racer). Caller updates `last_error` and routing trace.
    Retry {
        last_error: String,
        routing_note: String,
    },
    /// Circuit-breaker open — pick was skipped before any upstream call.
    CircuitSkip { provider_id: String },
    /// Race loser — another racer won, this attempt was aborted in-flight.
    /// Caller logs but does not surface to downstream.
    #[allow(dead_code)]
    RaceAborted { provider_id: String },
}

/// Attempt a single credential pick. Returns the terminal response, a
/// retry/skip decision, or a race-aborted marker.
pub(crate) async fn try_one_pick(
    state: AppState,
    mut epick: ExpandedPick,
    attempt_index: i32,
    ctx: Arc<PickCtx>,
) -> PickResult {
    let provider = epick.provider.clone();
    let upstream_model = epick.upstream_model.clone();
    let cb_key = epick.cb_key.clone();
    let log_ctx = LogCtx {
        wire: ctx.wire,
        route_prefix: ctx.route_prefix.clone(),
        credential_id: epick.credential_id.clone(),
        cb_key: Some(cb_key.clone()),
        dedupe_key: ctx.dedupe_key.clone(),
        client_transport: ctx.client_transport.clone(),
        request_headers: ctx.request_headers_json.clone(),
        codex_client_kind: ctx.codex_client_kind,
        claude_client_kind: ctx.claude_client_kind,
    };
    let attempt = new_attempt_ctx(
        &ctx.log_id,
        attempt_index,
        chrono::Utc::now().timestamp(),
        Some(&provider.id),
        epick.credential_id.as_deref(),
        &ctx.requested_model,
        &upstream_model,
    );

    // ── circuit breaker ──────────────────────────────────────────────
    if !state.cb.allow(&cb_key) {
        tracing::debug!(cb_key = %cb_key, "circuit open, skipping");
        let attempt_log = attempt_log_from_parts(
            &log_ctx,
            &attempt,
            UpstreamAttemptPhase::Abandoned,
            UpstreamAttemptOutcome::CircuitSkip,
            &ctx.started_instant,
            None,
            None,
            Some("circuit open".into()),
            Usage::default(),
        );
        persist_upstream_attempt_log(&state, attempt_log);
        return PickResult::CircuitSkip {
            provider_id: provider.id.clone(),
        };
    }
    publish_upstream_attempt_started(&state, &log_ctx, &attempt, UpstreamAttemptPhase::Connecting);

    // ── auth ─────────────────────────────────────────────────────────
    let secret = if let Some(oauth) = epick.oauth.take() {
        match resolve_oauth_token(&state, epick.credential_id.as_deref(), oauth).await {
            Ok(t) => Some(t),
            Err(e) => {
                if let Some(change) = state.cb.record_failure(&cb_key) {
                    emit_circuit_event(&state, &cb_key, change);
                }
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
                    ctx.started_instant.elapsed().as_millis() as i64,
                    Some("oauth refresh failed".into()),
                );
                let routing_note = format_routing_attempt(
                    &provider,
                    epick.credential.as_ref(),
                    &epick.credential_id,
                    format!("oauth refresh failed: {e}"),
                );
                let attempt_log = attempt_log_from_parts(
                    &log_ctx,
                    &attempt,
                    UpstreamAttemptPhase::Failed,
                    UpstreamAttemptOutcome::TransportError,
                    &ctx.started_instant,
                    None,
                    None,
                    Some(format!("oauth refresh failed: {e}")),
                    Usage::default(),
                );
                persist_upstream_attempt_log(&state, attempt_log);
                return PickResult::Retry {
                    last_error: format!("oauth error for {}: {e}", provider.id),
                    routing_note,
                };
            }
        }
    } else {
        match epick.auth_ref.as_deref() {
            Some("passthrough") => {
                if let Some(key) = ctx
                    .req_headers
                    .get("x-api-key")
                    .and_then(|v| v.to_str().ok())
                {
                    Some(key.to_string())
                } else if let Some(auth) = ctx
                    .req_headers
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
                        emit_circuit_event(&state, &cb_key, change);
                    }
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
                        ctx.started_instant.elapsed().as_millis() as i64,
                        Some("auth resolve failed".into()),
                    );
                    let routing_note = format_routing_attempt(
                        &provider,
                        epick.credential.as_ref(),
                        &epick.credential_id,
                        format!("auth resolve failed: {e}"),
                    );
                    let attempt_log = attempt_log_from_parts(
                        &log_ctx,
                        &attempt,
                        UpstreamAttemptPhase::Failed,
                        UpstreamAttemptOutcome::TransportError,
                        &ctx.started_instant,
                        None,
                        None,
                        Some(format!("auth resolve failed: {e}")),
                        Usage::default(),
                    );
                    persist_upstream_attempt_log(&state, attempt_log);
                    return PickResult::Retry {
                        last_error: format!("auth error for {}: {e}", provider.id),
                        routing_note,
                    };
                }
            },
            None => None,
        }
    };

    // ── cache injection (Anthropic only) ─────────────────────────────
    let effective_body: Bytes =
        if provider.kind == ProviderKind::Anthropic && state.config.failover.inject_cache {
            cache::inject(&ctx.body)
        } else {
            ctx.body.clone()
        };
    let effective_body =
        inject_chatgpt_codex_instructions_if_missing(&provider, ctx.wire, effective_body);

    // ── model rewrite + adapter ───────────────────────────────────────
    let adapter = providers::select(&provider);
    let body_up = match adapter.rewrite_body_model(&effective_body, &upstream_model) {
        Ok(b) => b,
        Err(e) => {
            let routing_note = format_routing_attempt(
                &provider,
                epick.credential.as_ref(),
                &epick.credential_id,
                format!("body rewrite: {e}"),
            );
            let attempt_log = attempt_log_from_parts(
                &log_ctx,
                &attempt,
                UpstreamAttemptPhase::Failed,
                UpstreamAttemptOutcome::ClientError,
                &ctx.started_instant,
                None,
                None,
                Some(format!("body rewrite: {e}")),
                Usage::default(),
            );
            persist_upstream_attempt_log(&state, attempt_log);
            return PickResult::Retry {
                last_error: format!("body rewrite: {e}"),
                routing_note,
            };
        }
    };

    // ── build request ─────────────────────────────────────────────────
    let req = match adapter.build(
        &provider,
        secret.as_deref(),
        &state.http,
        ctx.wire,
        &body_up,
        ctx.upstream_path.as_deref(),
    ) {
        Ok(r) => r,
        Err(e) => {
            let routing_note = format_routing_attempt(
                &provider,
                epick.credential.as_ref(),
                &epick.credential_id,
                format!("build upstream request: {e}"),
            );
            let attempt_log = attempt_log_from_parts(
                &log_ctx,
                &attempt,
                UpstreamAttemptPhase::Failed,
                UpstreamAttemptOutcome::ClientError,
                &ctx.started_instant,
                None,
                None,
                Some(format!("build upstream request: {e}")),
                Usage::default(),
            );
            persist_upstream_attempt_log(&state, attempt_log);
            return PickResult::Retry {
                last_error: e.to_string(),
                routing_note,
            };
        }
    };
    let req = if ctx.wire == Wire::Anthropic {
        // Claude Messages streams can take 10+ minutes for long thinking blocks; we
        // give them a wide-but-finite cap so the underlying reqwest socket doesn't
        // sit idle forever if the upstream stalls mid-stream.
        const CLAUDE_REQUEST_TIMEOUT_MS: u64 = 600_000;
        req.timeout(std::time::Duration::from_millis(CLAUDE_REQUEST_TIMEOUT_MS))
    } else {
        req
    };

    // ── send ─────────────────────────────────────────────────────────
    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            tracing::warn!(provider_id = %provider.id, cb_key = %cb_key, error = %msg, "upstream connection error");
            if let Some(change) = state.cb.record_failure(&cb_key) {
                emit_circuit_event(&state, &cb_key, change);
            }
            fire_health(
                &state,
                &provider.id,
                false,
                ctx.started_instant.elapsed().as_millis() as i64,
                Some(msg.clone()),
            );
            if let Some(cid) = &epick.credential_id {
                fire_credential_failure(&state, cid.clone(), Some(msg.clone()));
            }
            let routing_note = format_routing_attempt(
                &provider,
                epick.credential.as_ref(),
                &epick.credential_id,
                format!("connection error: {msg}"),
            );
            let attempt_log = attempt_log_from_parts(
                &log_ctx,
                &attempt,
                UpstreamAttemptPhase::Failed,
                UpstreamAttemptOutcome::TransportError,
                &ctx.started_instant,
                Some(502),
                None,
                Some(msg.clone()),
                Usage::default(),
            );
            persist_upstream_attempt_log(&state, attempt_log);
            return PickResult::Retry {
                last_error: format!("connection to {}: {msg}", provider.id),
                routing_note,
            };
        }
    };

    let status = upstream.status();

    // ── retryable errors ──────────────────────────────────────────────
    if let Some(retry_outcome) = outcome::classify_retryable(status, ctx.wire) {
        let headers = upstream.headers().clone();
        let retryable_resp_headers_snapshot = sanitized_headers_json(
            &copy_response_headers(&headers),
            state.config.log.redact_sensitive_headers,
        );
        let body_bytes = upstream.bytes().await.unwrap_or_default();
        let msg = format!("upstream {} from {}", status, provider.id);
        forget_codex_sticky_route_if_present(&state, ctx.sticky_key.as_deref());

        if retry_outcome == outcome::RetryOutcome::RateLimit {
            tracing::warn!(
                %msg,
                bytes = body_bytes.len(),
                "upstream 429; rotating credential without circuit breaker trip"
            );
            maybe_record_codex_plan(&state, &headers, &provider, epick.credential_id.as_deref());
            let rl = extract_rl_headers(&headers);
            if let Some(cid) = &epick.credential_id {
                fire_credential_rate_limit_only(&state, cid.clone(), rl);
            }
            fire_health(
                &state,
                &provider.id,
                false,
                ctx.started_instant.elapsed().as_millis() as i64,
                Some(format!("HTTP {status}")),
            );
            let routing_note = format_routing_attempt(
                &provider,
                epick.credential.as_ref(),
                &epick.credential_id,
                format!("HTTP {status} (rate limit / quota)"),
            );
            let mut attempt_log = attempt_log_from_parts(
                &log_ctx,
                &attempt,
                UpstreamAttemptPhase::Failed,
                UpstreamAttemptOutcome::RateLimit,
                &ctx.started_instant,
                Some(status.as_u16() as i32),
                Some(status.as_u16() as i32),
                Some(msg.clone()),
                Usage::default(),
            );
            attempt_log.response_headers = retryable_resp_headers_snapshot.clone();

            persist_upstream_attempt_log(&state, attempt_log);
            return PickResult::Retry {
                last_error: msg,
                routing_note,
            };
        }

        tracing::warn!(
            %msg,
            error_body_bytes = body_bytes.len(),
            "retryable upstream error, trying next provider"
        );
        if retry_outcome == outcome::RetryOutcome::AuthError {
            let change = state.cb.force_open(&cb_key);
            emit_circuit_event(&state, &cb_key, change);
        }
        fire_health(
            &state,
            &provider.id,
            false,
            ctx.started_instant.elapsed().as_millis() as i64,
            Some(format!("HTTP {status}")),
        );
        if let Some(cid) = &epick.credential_id {
            fire_credential_failure(&state, cid.clone(), Some(format!("HTTP {status}")));
        }
        let routing_note = format_routing_attempt(
            &provider,
            epick.credential.as_ref(),
            &epick.credential_id,
            format!("HTTP {status} (retryable upstream)"),
        );
        let mut attempt_log = attempt_log_from_parts(
            &log_ctx,
            &attempt,
            UpstreamAttemptPhase::Failed,
            UpstreamAttemptOutcome::RetryableError,
            &ctx.started_instant,
            Some(status.as_u16() as i32),
            Some(status.as_u16() as i32),
            Some(msg.clone()),
            Usage::default(),
        );
        attempt_log.response_headers = retryable_resp_headers_snapshot;

        persist_upstream_attempt_log(&state, attempt_log);
        return PickResult::Retry {
            last_error: msg,
            routing_note,
        };
    }

    // ── non-retryable client error (400/403 etc.) ────────────────────
    if status.is_client_error() {
        fire_health(
            &state,
            &provider.id,
            false,
            ctx.started_instant.elapsed().as_millis() as i64,
            Some(format!("client error {status}")),
        );
        if let Some(cid) = &epick.credential_id {
            fire_credential_failure(&state, cid.clone(), Some(format!("HTTP {status}")));
        }
        let resp_headers = copy_response_headers(upstream.headers());
        let resp_headers_snapshot = sanitized_headers_json(
            upstream.headers(),
            state.config.log.redact_sensitive_headers,
        );
        let buf = upstream.bytes().await.unwrap_or_default();
        let err_stored = lossy_optional_body(&buf);
        tracing::warn!(
            provider_id = %provider.id,
            status = %status,
            body_bytes = buf.len(),
            "non-retryable upstream error (4xx); network recording disabled"
        );
        let sc = status.as_u16() as i32;
        let mut attempt_log = attempt_log_from_parts(
            &log_ctx,
            &attempt,
            UpstreamAttemptPhase::Failed,
            UpstreamAttemptOutcome::ClientError,
            &ctx.started_instant,
            Some(sc),
            Some(sc),
            Some(format!("client error {status}")),
            Usage::default(),
        );
        attempt_log.response_headers = resp_headers_snapshot;

        persist_upstream_attempt_log(&state, attempt_log);
        let log = build_log(
            &log_ctx,
            &ctx.log_id,
            ctx.started_at,
            &ctx.started_instant,
            &ctx.app,
            Some(&provider.id),
            &ctx.requested_model,
            &upstream_model,
            Some(sc),
            Some(sc),
            err_stored.clone(),
            Some(format!("client error {status}")),
            Usage::default(),
            None,
            None,
        );
        persist_log(&state, log);
        return PickResult::Final((status, resp_headers, buf).into_response());
    }

    // ── 2xx success ───────────────────────────────────────────────────
    if let Some(change) = state.cb.record_success(&cb_key) {
        emit_circuit_event(&state, &cb_key, change);
    }
    fire_health(
        &state,
        &provider.id,
        true,
        ctx.started_instant.elapsed().as_millis() as i64,
        None,
    );

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
    let resp_headers_snapshot = sanitized_headers_json(
        upstream.headers(),
        state.config.log.redact_sensitive_headers,
    );
    let visual = codex_visual_context(
        &provider,
        epick.credential.as_ref(),
        epick.credential_id.as_deref(),
        &ctx.plan_by_cred,
        &ctx.requested_model,
        &upstream_model,
    );

    if body_wants_stream(&ctx.body) {
        remember_codex_sticky_route_for_pick(
            &state,
            ctx.sticky_key.as_deref(),
            &provider.id,
            epick.credential_id.as_deref(),
        );
        return PickResult::Final(stream_response(
            state,
            adapter,
            ctx.wire,
            upstream,
            status,
            resp_headers,
            resp_headers_snapshot,
            ctx.log_id.clone(),
            ctx.started_at,
            ctx.started_instant,
            ctx.app.clone(),
            attempt,
            provider.id.clone(),
            ctx.requested_model.clone(),
            upstream_model,
            log_ctx,
            None,
            visual,
        ));
    }

    let buf = match upstream.bytes().await {
        Ok(b) => b,
        Err(e) => {
            let mut attempt_log = attempt_log_from_parts(
                &log_ctx,
                &attempt,
                UpstreamAttemptPhase::Failed,
                UpstreamAttemptOutcome::TransportError,
                &ctx.started_instant,
                Some(502),
                Some(502),
                Some(format!("read upstream: {e}")),
                Usage::default(),
            );
            attempt_log.response_headers = resp_headers_snapshot.clone();

            persist_upstream_attempt_log(&state, attempt_log);
            let log = build_log(
                &log_ctx,
                &ctx.log_id,
                ctx.started_at,
                &ctx.started_instant,
                &ctx.app,
                Some(&provider.id),
                &ctx.requested_model,
                &upstream_model,
                Some(502),
                Some(502),
                None,
                Some(format!("read upstream: {e}")),
                Usage::default(),
                None,
                None,
            );
            persist_log(&state, log);
            return PickResult::Final(
                (StatusCode::BAD_GATEWAY, format!("read upstream: {e}")).into_response(),
            );
        }
    };

    let usage = adapter.parse_usage_body(ctx.wire, &buf);
    let sc = status.as_u16() as i32;
    let mut attempt_log = attempt_log_from_parts(
        &log_ctx,
        &attempt,
        UpstreamAttemptPhase::Completed,
        UpstreamAttemptOutcome::Success,
        &ctx.started_instant,
        Some(sc),
        Some(sc),
        None,
        usage,
    );
    attempt_log.response_headers = resp_headers_snapshot;

    let do_c2r = needs_chat_to_responses_bridge(ctx.wire, provider.kind);
    if do_c2r {
        attempt_log.bridge_mode = Some("c2r".into());
    }
    persist_upstream_attempt_log(&state, attempt_log);
    let mut client_body = buf.clone();
    if ctx.wire == Wire::Anthropic {
        let metrics = crate::codex_summary::SummaryMetrics::from_usage(
            usage,
            Some(ctx.started_instant.elapsed().as_millis() as i64),
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
        &ctx.log_id,
        ctx.started_at,
        &ctx.started_instant,
        &ctx.app,
        Some(&provider.id),
        &ctx.requested_model,
        &upstream_model,
        Some(sc),
        Some(sc),
        None,
        None,
        usage,
        None,
        None,
    );
    let client_body = if do_c2r {
        let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
        let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());
        let converted = transforms::chat_body_to_responses(&client_body, &session_id, &item_id);
        log.client_response_body = None;
        converted
    } else {
        if client_body != buf {
            log.client_response_body = None;
        }
        client_body
    };
    let client_body = if ctx.route_prefix.as_deref() == Some("codex-v1")
        && ctx.wire == Wire::OpenaiResponses
        && state.codex_summary_config().enabled
    {
        let mut response_value = serde_json::from_slice::<serde_json::Value>(&client_body).ok();
        if let Some(response) = response_value.as_mut() {
            let mut summary_injection = crate::codex_summary::SummaryAccumulator::new_for_turn(
                state.codex_summary_config(),
                log_ctx.codex_client_kind,
                Some(state.clone()),
                crate::codex_summary::turn_id_from_request(&ctx.body),
                crate::codex_summary::thread_id_from_request(&ctx.body),
                upstream_model.clone(),
            );
            let completed = serde_json::json!({
                "type": "response.completed",
                "response": response.clone(),
            })
            .to_string();
            if let Some(appended) = summary_injection
                .maybe_append_to_frame(&completed, ctx.started_instant.elapsed().as_millis() as i64)
            {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&appended) {
                    if let Some(r) = v.get("response") {
                        *response = r.clone();
                    }
                }
            }
        }
        response_value
            .and_then(|v| serde_json::to_vec(&v).ok())
            .map(Bytes::from)
            .unwrap_or(client_body)
    } else {
        client_body
    };
    let client_body = if ctx.route_prefix.as_deref() == Some("codex-v1")
        && ctx.wire == Wire::OpenaiResponses
        && transforms::responses_input_ends_with_user_message(&ctx.body)
        && state.codex_route_status_enabled()
    {
        let status_text = crate::codex_visual::status_message_text(
            &visual,
            ctx.started_instant.elapsed().as_millis() as i64,
        );
        let item_id = format!("vibe_route_{}", uuid::Uuid::new_v4().simple());
        let with_status =
            transforms::prepend_response_message(&client_body, &item_id, &status_text);
        log.client_response_body = None;
        with_status
    } else {
        log.client_response_body = None;
        client_body
    };
    remember_codex_sticky_route_for_pick(
        &state,
        ctx.sticky_key.as_deref(),
        &provider.id,
        epick.credential_id.as_deref(),
    );
    persist_log(&state, log);
    let mut response = (status, resp_headers, client_body).into_response();
    response.extensions_mut().insert(VibeCodexVisual(visual));
    response
        .extensions_mut()
        .insert(VibeCodexClientKind(log_ctx.codex_client_kind));
    PickResult::Final(response)
}

/// Outcome of a single wave (group of picks raced concurrently).
pub enum WaveOutcome {
    /// One pick returned terminal — its response wins the wave, losers were
    /// cancelled in-flight. Caller propagates to the downstream client.
    Final(Response),
    /// Every pick in this wave was non-terminal. Caller folds the accumulated
    /// trace and tallies into request-level state and advances to the next
    /// wave.
    AllNonTerminal {
        last_error: Option<String>,
        /// Routing-trace lines, one per pick that completed without winning.
        routing_notes: Vec<String>,
        /// Provider ids whose CB was open — used by the dispatcher to grow the
        /// `cb_skipped_provider_ids` dedupe set.
        cb_skip_provider_ids: Vec<String>,
        /// How many picks actually issued an upstream attempt (i.e. were not
        /// CB-skipped). Used to grow `attempted_after_cb`.
        retry_count: usize,
    },
}

/// Carrier for an in-flight racer's completion. `cb_skip_note` is `Some` only
/// when the pick was [`PickResult::CircuitSkip`] — we pre-format the routing
/// note inside the spawned task, before the `ExpandedPick`'s provider /
/// credential metadata goes out of scope.
struct WaveEvent {
    result: PickResult,
    cb_skip_note: Option<String>,
}

/// Run one bucket of picks concurrently — the unit of work that the wave
/// dispatcher in [`super::forward`] composes into a "病患先 → 健康兜底"
/// schedule. First terminal response wins; losers are cancelled via
/// [`CancellationToken`].
///
/// `base_attempt` is the running attempt counter; this wave assigns
/// `base_attempt + 1 ..= base_attempt + wave.len()` to its picks.
pub(crate) async fn run_wave(
    state: AppState,
    wave: Vec<ExpandedPick>,
    ctx: Arc<PickCtx>,
    base_attempt: i32,
) -> WaveOutcome {
    if wave.is_empty() {
        return WaveOutcome::AllNonTerminal {
            last_error: None,
            routing_notes: Vec::new(),
            cb_skip_provider_ids: Vec::new(),
            retry_count: 0,
        };
    }

    let size = wave.len();
    let cancel = CancellationToken::new();
    let (tx, mut rx) = mpsc::channel::<WaveEvent>(size);

    for (i, epick) in wave.into_iter().enumerate() {
        let state_c = state.clone();
        let ctx_c = ctx.clone();
        let tx_c = tx.clone();
        let cancel_c = cancel.clone();
        // Snapshot metadata so we can build the CB-skip routing note even
        // after `epick` is moved into `try_one_pick`.
        let provider_for_note = epick.provider.clone();
        let credential_for_note = epick.credential.clone();
        let credential_id_for_note = epick.credential_id.clone();
        let provider_id = epick.provider.id.clone();
        let attempt_index = base_attempt + (i as i32) + 1;
        tokio::spawn(async move {
            let result = tokio::select! {
                _ = cancel_c.cancelled() => PickResult::RaceAborted {
                    provider_id: provider_id.clone(),
                },
                r = try_one_pick(state_c, epick, attempt_index, ctx_c) => r,
            };
            let cb_skip_note = match &result {
                PickResult::CircuitSkip { .. } => Some(super::format_routing_attempt(
                    &provider_for_note,
                    credential_for_note.as_ref(),
                    &credential_id_for_note,
                    "skipped (circuit open)",
                )),
                _ => None,
            };
            let _ = tx_c
                .send(WaveEvent {
                    result,
                    cb_skip_note,
                })
                .await;
        });
    }
    drop(tx);

    let mut last_error: Option<String> = None;
    let mut routing_notes: Vec<String> = Vec::new();
    let mut cb_skip_provider_ids: Vec<String> = Vec::new();
    let mut retry_count: usize = 0;
    let mut received = 0usize;
    while let Some(event) = rx.recv().await {
        received += 1;
        match event.result {
            PickResult::Final(resp) => {
                cancel.cancel();
                return WaveOutcome::Final(resp);
            }
            PickResult::Retry {
                last_error: err,
                routing_note,
            } => {
                last_error = Some(err);
                routing_notes.push(routing_note);
                retry_count += 1;
            }
            PickResult::CircuitSkip { provider_id } => {
                cb_skip_provider_ids.push(provider_id);
                if let Some(note) = event.cb_skip_note {
                    routing_notes.push(note);
                }
            }
            PickResult::RaceAborted { .. } => {
                // Aborted-loser bookkeeping is owned by `try_one_pick`'s
                // attempt log — nothing to surface to the caller.
            }
        }
        if received >= size {
            break;
        }
    }

    WaveOutcome::AllNonTerminal {
        last_error,
        routing_notes,
        cb_skip_provider_ids,
        retry_count,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use axum::http::HeaderValue;
    use axum::routing::any;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Duration;
    use tokio::net::TcpListener;
    use vibe_protocol::{Credential, Provider, ProviderKind};

    // ── Mock upstream helpers ───────────────────────────────────────────

    /// Start an axum mock that responds with `(status, body)` for any path.
    async fn start_mock(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().fallback(any(move || async move {
            (
                axum::http::StatusCode::from_u16(status).expect("status"),
                body,
            )
        }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{}", addr)
    }

    /// Start an axum mock that delays before responding.
    async fn start_mock_with_delay(delay_ms: u64, status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().fallback(any(move || async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            (
                axum::http::StatusCode::from_u16(status).expect("status"),
                body,
            )
        }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{}", addr)
    }

    // ── Fixture builders ────────────────────────────────────────────────

    fn make_state() -> AppState {
        let db = vibe_db::Db::memory().expect("db");
        AppState::init(db, Config::default(), 0).expect("state")
    }

    fn make_provider(id: &str, kind: ProviderKind, base_url: String) -> Provider {
        Provider {
            id: id.into(),
            name: id.into(),
            group_name: None,
            avatar_url: None,
            kind,
            base_url,
            protocols: vec![],
            host: None,
            auth_ref: Some("passthrough".into()),
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: vec![],
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_epick_passthrough(provider: Provider) -> ExpandedPick {
        ExpandedPick {
            cb_key: provider.id.clone(),
            upstream_model: "gpt-test".into(),
            auth_ref: Some("passthrough".into()),
            oauth: None,
            credential_id: None,
            credential: None,
            provider,
        }
    }

    fn make_epick_with_auth_ref(provider: Provider, auth_ref: &str) -> ExpandedPick {
        ExpandedPick {
            cb_key: provider.id.clone(),
            upstream_model: "gpt-test".into(),
            auth_ref: Some(auth_ref.into()),
            oauth: None,
            credential_id: None,
            credential: None,
            provider,
        }
    }

    fn make_ctx(wire: Wire, body: Bytes) -> Arc<PickCtx> {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("test-key"));
        Arc::new(PickCtx {
            wire,
            route_prefix: None,
            log_id: "test-log".into(),
            started_at: 0,
            started_instant: Instant::now(),
            app: None,
            requested_model: "gpt-test".into(),
            upstream_path: None,
            dedupe_key: None,
            client_transport: None,
            request_headers_json: None,
            codex_client_kind: CodexClientKind::Unknown,
            claude_client_kind: ClaudeClientKind::Unknown,
            body,
            req_headers: headers,
            request_snapshot: None,
            sticky_key: None,
            plan_by_cred: Arc::new(HashMap::new()),
        })
    }

    fn openai_chat_body() -> Bytes {
        Bytes::from(r#"{"model":"gpt-test","messages":[{"role":"user","content":"hi"}]}"#)
    }

    fn anthropic_body() -> Bytes {
        Bytes::from(
            r#"{"model":"claude-test","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#,
        )
    }

    // ── 1. 2xx success on OpenAI Chat upstream ─────────────────────────

    #[tokio::test]
    async fn pick_returns_final_on_200_openai_chat() {
        let base = start_mock(200, r#"{"id":"ok","choices":[]}"#).await;
        let state = make_state();
        let provider = make_provider("p-200", ProviderKind::OpenaiChat, base);
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        match result {
            PickResult::Final(resp) => assert_eq!(resp.status(), StatusCode::OK),
            other => panic!("expected Final(200), got {:?}", variant_name(&other)),
        }
    }

    // ── 2. Non-retryable 4xx (400 Bad Request) ──────────────────────────

    #[tokio::test]
    async fn pick_returns_final_on_400_bad_request() {
        let base = start_mock(400, r#"{"error":"bad"}"#).await;
        let state = make_state();
        let provider = make_provider("p-400", ProviderKind::OpenaiChat, base);
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        match result {
            PickResult::Final(resp) => assert_eq!(resp.status(), StatusCode::BAD_REQUEST),
            other => panic!("expected Final(400), got {:?}", variant_name(&other)),
        }
    }

    // ── 3. 404 on OpenAI Chat wire — non-retryable terminal ─────────────

    #[tokio::test]
    async fn pick_returns_final_on_404_when_wire_is_chat() {
        let base = start_mock(404, "not found").await;
        let state = make_state();
        let provider = make_provider("p-404chat", ProviderKind::OpenaiChat, base);
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        match result {
            PickResult::Final(resp) => assert_eq!(resp.status(), StatusCode::NOT_FOUND),
            other => panic!("expected Final(404), got {:?}", variant_name(&other)),
        }
    }

    // ── 4. 404 on OpenaiResponses wire — retryable ──────────────────────

    #[tokio::test]
    async fn pick_retries_on_404_when_wire_is_openai_responses() {
        let base = start_mock(404, "no route").await;
        let state = make_state();
        let provider = make_provider("p-404resp", ProviderKind::OpenaiResponses, base);
        let epick = make_epick_passthrough(provider);
        let body = Bytes::from(r#"{"model":"gpt-test","input":"hi"}"#);
        let ctx = make_ctx(Wire::OpenaiResponses, body);

        let result = try_one_pick(state, epick, 1, ctx).await;
        assert!(
            matches!(result, PickResult::Retry { .. }),
            "expected Retry, got {}",
            variant_name(&result)
        );
    }

    // ── 5. 429 rate-limit — retryable, CB NOT tripped ───────────────────

    #[tokio::test]
    async fn pick_retries_on_429_without_tripping_cb() {
        let base = start_mock(429, r#"{"error":"rate_limit"}"#).await;
        let state = make_state();
        let provider = make_provider("p-429", ProviderKind::OpenaiChat, base);
        let cb_key = provider.id.clone();
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state.clone(), epick, 1, ctx).await;
        assert!(matches!(result, PickResult::Retry { .. }));
        // CB must NOT have been tripped by 429 alone (record_failure is skipped).
        assert!(
            state.cb.allow(&cb_key),
            "CB should still allow after a single 429"
        );
    }

    // ── 6. 401 auth error — retryable, CB force-opened ──────────────────

    #[tokio::test]
    async fn pick_retries_on_401_and_force_opens_cb() {
        let base = start_mock(401, r#"{"error":"unauthorized"}"#).await;
        let state = make_state();
        let provider = make_provider("p-401", ProviderKind::OpenaiChat, base);
        let cb_key = provider.id.clone();
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state.clone(), epick, 1, ctx).await;
        assert!(matches!(result, PickResult::Retry { .. }));
        assert!(!state.cb.allow(&cb_key), "CB must be force-opened by 401");
    }

    // ── 7. 403 forbidden — treated as auth error, retryable ─────────────

    #[tokio::test]
    async fn pick_retries_on_403_treated_as_auth_error() {
        let base = start_mock(403, r#"{"error":"forbidden"}"#).await;
        let state = make_state();
        let provider = make_provider("p-403", ProviderKind::OpenaiChat, base);
        let cb_key = provider.id.clone();
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state.clone(), epick, 1, ctx).await;
        assert!(matches!(result, PickResult::Retry { .. }));
        assert!(
            !state.cb.allow(&cb_key),
            "CB must be force-opened by 403 (treated like 401)"
        );
    }

    // ── 8. 500 server error — retryable ─────────────────────────────────

    #[tokio::test]
    async fn pick_retries_on_500_server_error() {
        let base = start_mock(500, "oops").await;
        let state = make_state();
        let provider = make_provider("p-500", ProviderKind::OpenaiChat, base);
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        assert!(matches!(result, PickResult::Retry { .. }));
    }

    // ── 9. 502 bad gateway — retryable ──────────────────────────────────

    #[tokio::test]
    async fn pick_retries_on_502_bad_gateway() {
        let base = start_mock(502, "upstream down").await;
        let state = make_state();
        let provider = make_provider("p-502", ProviderKind::OpenaiChat, base);
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        assert!(matches!(result, PickResult::Retry { .. }));
    }

    // ── 10. Connection error (port closed) — retryable ──────────────────

    #[tokio::test]
    async fn pick_retries_on_connection_error() {
        // Port 1 is reserved + universally refused.
        let state = make_state();
        let provider = make_provider(
            "p-conn",
            ProviderKind::OpenaiChat,
            "http://127.0.0.1:1".into(),
        );
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        assert!(
            matches!(result, PickResult::Retry { .. }),
            "expected Retry on connection error, got {}",
            variant_name(&result)
        );
    }

    // ── 11. CB already open — CircuitSkip without sending ───────────────

    #[tokio::test]
    async fn pick_returns_circuit_skip_when_cb_open() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_handler = counter.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().fallback(any(move || {
            let counter = counter_for_handler.clone();
            async move {
                counter.fetch_add(1, AtomicOrdering::SeqCst);
                (axum::http::StatusCode::OK, "should not be called")
            }
        }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let base = format!("http://{}", addr);

        let state = make_state();
        let provider = make_provider("p-cb-open", ProviderKind::OpenaiChat, base);
        state.cb.force_open(&provider.id);
        let epick = make_epick_passthrough(provider.clone());
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        match result {
            PickResult::CircuitSkip { provider_id } => {
                assert_eq!(provider_id, "p-cb-open");
            }
            other => panic!("expected CircuitSkip, got {}", variant_name(&other)),
        }
        // Brief grace period in case a stray request was issued.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            counter.load(AtomicOrdering::SeqCst),
            0,
            "upstream must not be hit when CB is open"
        );
    }

    // ── 12. Auth resolve failure — retryable ────────────────────────────

    #[tokio::test]
    async fn pick_retries_when_auth_resolve_fails() {
        let base = start_mock(200, r#"{"ok":true}"#).await;
        let state = make_state();
        let provider = make_provider("p-auth-fail", ProviderKind::OpenaiChat, base);
        // env:VIBE_RACE_TEST_DOES_NOT_EXIST_XYZ resolves to an unset env var,
        // which secrets::resolve treats as an error.
        let epick = make_epick_with_auth_ref(provider, "env:VIBE_RACE_TEST_DOES_NOT_EXIST_XYZ");
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        assert!(
            matches!(result, PickResult::Retry { .. }),
            "expected Retry on auth resolve failure, got {}",
            variant_name(&result)
        );
    }

    // ── 13. Anthropic 200 — wire-specific success path ──────────────────

    #[tokio::test]
    async fn pick_returns_final_on_anthropic_200() {
        let base = start_mock(
            200,
            r#"{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-test","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#,
        )
        .await;
        let state = make_state();
        let provider = make_provider("p-anthr", ProviderKind::Anthropic, base);
        let epick = make_epick_passthrough(provider);
        let ctx = make_ctx(Wire::Anthropic, anthropic_body());

        let result = try_one_pick(state, epick, 1, ctx).await;
        match result {
            PickResult::Final(resp) => assert_eq!(resp.status(), StatusCode::OK),
            other => panic!("expected Final(200), got {}", variant_name(&other)),
        }
    }

    // ── 14. run_wave: first Final wins, slow loser cancelled ────────────

    #[tokio::test]
    async fn run_wave_first_final_wins_others_cancelled() {
        let fast_base = start_mock(200, r#"{"id":"fast"}"#).await;
        // Slow racer takes 2 s — well past the moment the fast one returns.
        let slow_base = start_mock_with_delay(2_000, 200, r#"{"id":"slow"}"#).await;
        let state = make_state();

        let p_fast = make_provider("p-fast", ProviderKind::OpenaiChat, fast_base);
        let p_slow = make_provider("p-slow", ProviderKind::OpenaiChat, slow_base);
        let wave = vec![
            make_epick_passthrough(p_fast),
            make_epick_passthrough(p_slow),
        ];
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let start = Instant::now();
        let outcome = run_wave(state, wave, ctx, 0).await;
        let elapsed = start.elapsed();

        match outcome {
            WaveOutcome::Final(resp) => assert_eq!(resp.status(), StatusCode::OK),
            WaveOutcome::AllNonTerminal { .. } => panic!("expected Final, got AllNonTerminal"),
        }
        // Should resolve well under the slow racer's 2 s delay.
        assert!(
            elapsed < Duration::from_millis(1_500),
            "race took {:?}; expected < 1.5s",
            elapsed
        );
    }

    // ── 15. run_wave: every racer retryable → AllNonTerminal ────────────

    #[tokio::test]
    async fn run_wave_all_retryable_yields_all_non_terminal() {
        let fail_base_1 = start_mock(500, "boom").await;
        let fail_base_2 = start_mock(500, "boom").await;
        let state = make_state();

        let wave = vec![
            make_epick_passthrough(make_provider(
                "p-fail-1",
                ProviderKind::OpenaiChat,
                fail_base_1,
            )),
            make_epick_passthrough(make_provider(
                "p-fail-2",
                ProviderKind::OpenaiChat,
                fail_base_2,
            )),
        ];
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let outcome = run_wave(state, wave, ctx, 0).await;
        match outcome {
            WaveOutcome::AllNonTerminal {
                retry_count,
                cb_skip_provider_ids,
                routing_notes,
                last_error,
            } => {
                assert_eq!(retry_count, 2);
                assert!(cb_skip_provider_ids.is_empty());
                assert_eq!(routing_notes.len(), 2);
                assert!(
                    last_error.is_some(),
                    "last_error should be populated by 500s"
                );
            }
            WaveOutcome::Final(_) => panic!("expected AllNonTerminal, got Final"),
        }
    }

    // ── 16. run_wave: CB-skipped picks are reported separately ──────────

    #[tokio::test]
    async fn run_wave_mixed_cb_open_and_500_reports_both() {
        let cb_open_base = start_mock(200, "should not be called").await;
        let live_500_base = start_mock(500, "boom").await;
        let state = make_state();

        let p_open = make_provider("p-cb-open", ProviderKind::OpenaiChat, cb_open_base);
        state.cb.force_open(&p_open.id);
        let p_live = make_provider("p-live", ProviderKind::OpenaiChat, live_500_base);

        let wave = vec![
            make_epick_passthrough(p_open),
            make_epick_passthrough(p_live),
        ];
        let ctx = make_ctx(Wire::OpenaiChat, openai_chat_body());

        let outcome = run_wave(state, wave, ctx, 0).await;
        match outcome {
            WaveOutcome::AllNonTerminal {
                retry_count,
                cb_skip_provider_ids,
                routing_notes,
                ..
            } => {
                assert_eq!(retry_count, 1, "only the 500 pick issued an upstream call");
                assert_eq!(cb_skip_provider_ids, vec!["p-cb-open".to_string()]);
                assert_eq!(
                    routing_notes.len(),
                    2,
                    "both the retry and the CB-skip should produce a routing note"
                );
            }
            WaveOutcome::Final(_) => panic!("expected AllNonTerminal, got Final"),
        }
    }

    // ── debug helpers ───────────────────────────────────────────────────

    fn variant_name(r: &PickResult) -> &'static str {
        match r {
            PickResult::Final(_) => "Final",
            PickResult::Retry { .. } => "Retry",
            PickResult::CircuitSkip { .. } => "CircuitSkip",
            PickResult::RaceAborted { .. } => "RaceAborted",
        }
    }

    // Silence unused-import warning for the tests-only Credential symbol; it
    // exists so future tests can attach a Credential to an ExpandedPick.
    #[allow(dead_code)]
    fn _ensure_credential_in_scope(_c: Credential) {}
}
