use crate::auth_fingerprint;
use crate::codex_summary;
use crate::codex_visual;
use crate::forward::{self, PreparedForward, PreparedForwardError};
use crate::providers::Wire;
use crate::router;
use crate::state::AppState;
use crate::stream_trace::StreamTraceStats;
use crate::transforms;
use crate::usage::Usage;
use axum::extract::ws::{Message, WebSocket};
use axum::http::HeaderMap;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http::{Request, StatusCode};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as TungsteniteError;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

const WS_V2_BETA_HEADER_VALUE: &str = "responses_websockets=2026-02-06";

pub enum UpstreamWsOutcome {
    Forwarded,
    Fallback(Bytes),
}

pub struct StatusDecision {
    pub should_show_status: bool,
    pub turn_id: Option<String>,
}

pub async fn try_forward_official_codex_ws(
    socket: &mut WebSocket,
    state: AppState,
    ws_headers: HeaderMap,
    body_bytes: Bytes,
    status_decision: StatusDecision,
) -> UpstreamWsOutcome {
    let request_body = transforms::force_responses_stream_true(&body_bytes);
    let prepared = match forward::prepare_forward_once(
        &state,
        Wire::OpenaiResponses,
        None,
        &ws_headers,
        request_body,
        Some("codex-v1".into()),
        true,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(err) => {
            send_prepare_error(socket, &state, err).await;
            return UpstreamWsOutcome::Forwarded;
        }
    };

    if !router::provider_is_chatgpt_codex_official(&prepared.provider) {
        return UpstreamWsOutcome::Fallback(body_bytes);
    }

    forward::publish_request_started(
        &state,
        &prepared.log_id,
        prepared.started_at,
        &prepared.app,
        &prepared.log_ctx,
        None,
        &prepared.requested_model,
    );
    forward::persist_request_log_placeholder(
        &state,
        &prepared.log_id,
        prepared.started_at,
        &prepared.app,
        &prepared.log_ctx,
        None,
        &prepared.requested_model,
    );
    let attempt = forward::new_attempt_ctx(
        &prepared.log_id,
        1,
        prepared.started_at,
        Some(&prepared.provider.id),
        prepared.credential_id.as_deref(),
        &prepared.requested_model,
        &prepared.upstream_model,
    );
    forward::publish_upstream_attempt_started(
        &state,
        &prepared.log_ctx,
        &attempt,
        vibe_protocol::UpstreamAttemptPhase::Connecting,
    );

    let mut log = forward::request_log_from_parts(
        &prepared.log_ctx,
        &prepared.log_id,
        prepared.started_at,
        &prepared.started_instant,
        &prepared.app,
        Some(&prepared.provider.id),
        &prepared.requested_model,
        &prepared.upstream_model,
        None,
        None,
        None,
        None,
        Usage::default(),
        prepared.request_snapshot.clone(),
        None,
    );

    let response_id = response_id_from_request(&prepared.body_up)
        .unwrap_or_else(|| format!("resp-{}", uuid::Uuid::new_v4().simple()));
    let request = match build_upstream_ws_request(&prepared, &ws_headers) {
        Ok(request) => request,
        Err(err) => {
            let payload = transforms::codex_response_proxy_fault_event(
                &response_id,
                "proxy_ws_url_error",
                &err,
            );
            let _ = send_text(socket, &payload).await;
            forward::persist_upstream_attempt_log(
                &state,
                forward::attempt_log_from_parts(
                    &prepared.log_ctx,
                    &attempt,
                    vibe_protocol::UpstreamAttemptPhase::Failed,
                    vibe_protocol::UpstreamAttemptOutcome::ClientError,
                    &prepared.started_instant,
                    Some(502),
                    Some(502),
                    Some("failed to build upstream websocket request".into()),
                    Usage::default(),
                ),
            );
            finalize_log(
                &state,
                prepared,
                log,
                Some(502),
                Some("failed to build upstream websocket request".into()),
                None,
                Some(payload),
            );
            return UpstreamWsOutcome::Forwarded;
        }
    };

    let upstream = connect_async(request).await;
    let (mut upstream_ws, handshake) = match upstream {
        Ok(pair) => pair,
        Err(err) => {
            let (status, detail) = websocket_connect_error_status_body(&err);
            let payload =
                transforms::codex_response_failed_event(&response_id, status as u16, &detail);
            let _ = send_text(socket, &payload).await;
            forward::mark_provider_health(
                &state,
                &prepared.provider.id,
                false,
                prepared.started_instant.elapsed().as_millis() as i64,
                Some("websocket connect failed".into()),
            );
            forward::persist_upstream_attempt_log(
                &state,
                forward::attempt_log_from_parts(
                    &prepared.log_ctx,
                    &attempt,
                    vibe_protocol::UpstreamAttemptPhase::Failed,
                    vibe_protocol::UpstreamAttemptOutcome::TransportError,
                    &prepared.started_instant,
                    Some(status),
                    Some(status),
                    Some("websocket connect failed".into()),
                    Usage::default(),
                ),
            );
            finalize_log(
                &state,
                prepared,
                log,
                Some(status),
                Some("websocket connect failed".into()),
                Some(detail),
                Some(payload),
            );
            return UpstreamWsOutcome::Forwarded;
        }
    };

    forward::record_codex_plan_from_response_headers(
        &state,
        handshake.headers(),
        &prepared.provider,
        prepared.credential_id.as_deref(),
    )
    .await;

    if let Err(err) = upstream_ws
        .send(TungsteniteMessage::Text(
            String::from_utf8_lossy(&prepared.body_up).into_owned(),
        ))
        .await
    {
        let payload = transforms::codex_response_proxy_fault_event(
            &response_id,
            "upstream_ws_send_error",
            &err.to_string(),
        );
        let _ = send_text(socket, &payload).await;
        forward::persist_upstream_attempt_log(
            &state,
            forward::attempt_log_from_parts(
                &prepared.log_ctx,
                &attempt,
                vibe_protocol::UpstreamAttemptPhase::Failed,
                vibe_protocol::UpstreamAttemptOutcome::TransportError,
                &prepared.started_instant,
                Some(502),
                Some(502),
                Some("websocket send failed".into()),
                Usage::default(),
            ),
        );
        finalize_log(
            &state,
            prepared,
            log,
            Some(502),
            Some("websocket send failed".into()),
            None,
            Some(payload),
        );
        return UpstreamWsOutcome::Forwarded;
    }

    let mut client_trace = String::new();
    let mut upstream_trace = String::new();
    let mut usage = Usage::default();
    let mut first_token_ms: Option<i64> = None;
    let mut status_injection: Option<CodexStatusInjection> = None;
    let mut summary_injection = codex_summary::SummaryAccumulator::new_for_turn(
        state.codex_summary_config(),
        prepared.log_ctx.codex_client_kind,
        Some(state.clone()),
        status_decision.turn_id.clone(),
    );
    let mut current_response_id = response_id.clone();
    let mut terminal_seen = false;
    let mut failed_status: Option<i32> = None;
    let mut failed_error: Option<String> = None;
    let mut trace_stats = StreamTraceStats::new("websocket", "responses_to_ws");
    let mut upstream_decode_tps_peak: Option<f64> = None;
    let mut downstream_emit_tps_peak: Option<f64> = None;

    while let Some(next) = upstream_ws.next().await {
        match next {
            Ok(TungsteniteMessage::Text(text)) => {
                trace_stats.record_upstream_chunk(&prepared.started_instant, text.len());
                trace_stats.record_ws_text(&prepared.started_instant, &text);
                if first_token_ms.is_none() {
                    first_token_ms = Some(prepared.started_instant.elapsed().as_millis() as i64);
                }
                if status_injection.is_none() {
                    status_injection = CodexStatusInjection::new(
                        Some(prepared.visual.clone()),
                        prepared.started_instant.elapsed().as_millis() as i64,
                        status_decision.should_show_status,
                        status_decision.turn_id.as_deref(),
                        &state,
                    );
                }
                append_trace(&mut upstream_trace, &text);
                if let Some(status) = wrapped_error_status(&text) {
                    failed_status = Some(status);
                    failed_error = Some("upstream websocket error event".into());
                    terminal_seen = true;
                }
                if update_usage_and_terminal(&text, &mut usage) {
                    terminal_seen = true;
                }
                if let Some(message) = response_failed_message(&text) {
                    failed_status.get_or_insert(500);
                    failed_error.get_or_insert(message);
                }
                if let Some(created_id) = response_created_id(&text) {
                    current_response_id = created_id;
                }
                let is_created = codex_frame_is_response_created(&text);
                let mut emitted_frames = summary_injection.maybe_append_to_frame_batch(
                    vec![text.clone()],
                    prepared.started_instant.elapsed().as_millis() as i64,
                );
                if emitted_frames.is_empty() {
                    emitted_frames.push(text);
                }
                let mut downstream_closed = false;
                for frame in emitted_frames {
                    append_trace(&mut client_trace, &frame);
                    if send_text(socket, &frame).await.is_err() {
                        trace_stats.finish("downstream_closed");
                        downstream_closed = true;
                        break;
                    }
                    trace_stats.record_client_chunk(&prepared.started_instant, frame.len());
                }
                if downstream_closed {
                    break;
                }
                if is_created {
                    if let Some(injection) = status_injection.as_mut() {
                        for injected in injection.next_frames(&current_response_id) {
                            trace_stats.mark_status_injected();
                            append_trace(&mut client_trace, &injected);
                            if send_text(socket, &injected).await.is_err() {
                                trace_stats.finish("downstream_closed");
                                break;
                            } else {
                                trace_stats
                                    .record_client_chunk(&prepared.started_instant, injected.len());
                            }
                        }
                    }
                }
                let elapsed_ms = prepared.started_instant.elapsed().as_millis() as i64;
                let active_upstream_decode_tps =
                    trace_stats.active_upstream_decode_tps(usage.output_tokens, elapsed_ms);
                let active_downstream_emit_tps =
                    trace_stats.active_downstream_emit_tps(usage.output_tokens, elapsed_ms);
                let runtime_rates = trace_stats.runtime_rates(usage.output_tokens, elapsed_ms);
                forward::update_peak(&mut upstream_decode_tps_peak, active_upstream_decode_tps);
                forward::update_peak(&mut downstream_emit_tps_peak, active_downstream_emit_tps);
                forward::publish_runtime_stats(
                    &state,
                    &attempt.request_id,
                    Some(&attempt.attempt_id),
                    Some(&prepared.provider.id),
                    None,
                    active_upstream_decode_tps,
                    active_downstream_emit_tps,
                    runtime_rates.active_output_tokens_per_sec,
                    runtime_rates.active_upstream_bytes_per_sec,
                    runtime_rates.active_downstream_bytes_per_sec,
                    runtime_rates.active_flow_bytes_per_sec,
                    usage.output_tokens,
                    trace_stats.upstream_bytes(),
                    trace_stats.client_bytes(),
                    trace_stats.upstream_first_byte_ms(),
                    trace_stats.client_first_write_ms(),
                    true,
                );
                forward::publish_runtime_stats(
                    &state,
                    &attempt.request_id,
                    None,
                    Some(&prepared.provider.id),
                    active_upstream_decode_tps.or(runtime_rates.active_output_tokens_per_sec),
                    None,
                    None,
                    runtime_rates.active_output_tokens_per_sec,
                    runtime_rates.active_upstream_bytes_per_sec,
                    runtime_rates.active_downstream_bytes_per_sec,
                    runtime_rates.active_flow_bytes_per_sec,
                    usage.output_tokens,
                    trace_stats.upstream_bytes(),
                    trace_stats.client_bytes(),
                    trace_stats.upstream_first_byte_ms(),
                    trace_stats.client_first_write_ms(),
                    false,
                );
                if terminal_seen {
                    break;
                }
            }
            Ok(TungsteniteMessage::Binary(bytes)) => {
                trace_stats.record_upstream_chunk(&prepared.started_instant, bytes.len());
                let text = String::from_utf8_lossy(&bytes).into_owned();
                append_trace(&mut upstream_trace, &text);
                append_trace(&mut client_trace, &text);
                if socket.send(Message::Binary(bytes)).await.is_err() {
                    trace_stats.finish("downstream_closed");
                    break;
                } else {
                    trace_stats.record_client_chunk(&prepared.started_instant, text.len());
                }
            }
            Ok(TungsteniteMessage::Ping(payload)) => {
                let _ = upstream_ws.send(TungsteniteMessage::Pong(payload)).await;
            }
            Ok(TungsteniteMessage::Pong(_)) => {}
            Ok(TungsteniteMessage::Close(_)) => break,
            Ok(TungsteniteMessage::Frame(_)) => {}
            Err(err) => {
                trace_stats.finish_error("upstream_read_error", err.to_string());
                failed_status = Some(502);
                failed_error = Some(format!("upstream websocket read failed: {err}"));
                let payload = transforms::codex_response_proxy_fault_event(
                    &response_id,
                    "upstream_ws_read_error",
                    &err.to_string(),
                );
                append_trace(&mut client_trace, &payload);
                trace_stats.mark_terminal_injected();
                if send_text(socket, &payload).await.is_ok() {
                    trace_stats.record_client_chunk(&prepared.started_instant, payload.len());
                }
                terminal_seen = true;
                break;
            }
        }
    }

    if !terminal_seen {
        if trace_stats.end_reason().is_none() {
            trace_stats.finish("truncated");
        }
        failed_status = Some(502);
        failed_error = Some("upstream websocket closed before a terminal event".into());
        let payload = transforms::codex_response_proxy_fault_event(
            &response_id,
            "upstream_ws_truncated",
            "upstream websocket closed before response.completed / response.failed",
        );
        append_trace(&mut client_trace, &payload);
        trace_stats.mark_terminal_injected();
        if send_text(socket, &payload).await.is_ok() {
            trace_stats.record_client_chunk(&prepared.started_instant, payload.len());
        }
    } else if trace_stats.end_reason().is_none() {
        trace_stats.finish("completed");
    }

    log.first_token_ms = first_token_ms;
    log.input_tokens = usage.input_tokens;
    log.output_tokens = usage.output_tokens;
    log.cache_read_tokens = usage.cache_read_tokens;
    log.cache_creation_tokens = usage.cache_creation_tokens;
    log.status_code = failed_status.or(Some(200));
    log.upstream_http_status = failed_status.or(Some(101));
    log.error = failed_error;
    log.response_body = (!upstream_trace.is_empty()).then_some(upstream_trace);
    log.client_response_body = (!client_trace.is_empty()).then_some(client_trace);
    log.latency_ms = Some(prepared.started_instant.elapsed().as_millis() as i64);
    log.estimated_cost_usd = usage.estimated_cost_usd(&prepared.upstream_model);
    trace_stats.apply_to_log(&mut log);

    let mut attempt_log = forward::attempt_log_from_parts(
        &prepared.log_ctx,
        &attempt,
        if failed_status.is_none() {
            vibe_protocol::UpstreamAttemptPhase::Completed
        } else {
            vibe_protocol::UpstreamAttemptPhase::Failed
        },
        if failed_status.is_none() {
            vibe_protocol::UpstreamAttemptOutcome::Success
        } else {
            vibe_protocol::UpstreamAttemptOutcome::TransportError
        },
        &prepared.started_instant,
        log.status_code,
        log.upstream_http_status,
        log.error.clone(),
        usage,
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
    attempt_log.parse_error_count = log.parse_error_count;
    attempt_log.bridge_mode = log.bridge_mode.clone();
    attempt_log.status_injected = log.status_injected;
    attempt_log.terminal_injected = log.terminal_injected;
    attempt_log.upstream_terminal_type = log.upstream_terminal_type.clone();
    attempt_log.active_upstream_decode_tps_peak = upstream_decode_tps_peak;
    attempt_log.active_downstream_emit_tps_peak = downstream_emit_tps_peak;
    forward::persist_upstream_attempt_log(&state, attempt_log);

    forward::mark_provider_health(
        &state,
        &prepared.provider.id,
        failed_status.is_none(),
        prepared.started_instant.elapsed().as_millis() as i64,
        failed_status.map(|s| format!("websocket status {s}")),
    );
    // If this WS attempt failed, drop the sticky route so the next retry is
    // free to pick a different provider/credential instead of being locked
    // onto the same broken slot.
    if log.status_code != Some(200) {
        forward::forget_codex_sticky_route_if_present(&state, prepared.sticky_key.as_deref());
    }
    forward::persist_request_log(&state, log);

    UpstreamWsOutcome::Forwarded
}

#[derive(Clone, Debug)]
struct CodexStatusInjection {
    visual: codex_visual::CodexVisualContext,
    ttfs_ms: i64,
    emitted: bool,
    suppress_status: bool,
}

impl CodexStatusInjection {
    fn new(
        visual: Option<codex_visual::CodexVisualContext>,
        ttfs_ms: i64,
        should_show_status: bool,
        turn_id: Option<&str>,
        state: &AppState,
    ) -> Option<Self> {
        visual.map(|visual| {
            let suppress_status = !state.codex_route_status_enabled()
                || !should_show_status
                || !should_emit_codex_route_status(state, turn_id, &visual);
            Self {
                visual,
                ttfs_ms,
                emitted: false,
                suppress_status,
            }
        })
    }

    fn next_frames(&mut self, response_id: &str) -> Vec<String> {
        if self.emitted {
            return Vec::new();
        }
        self.emitted = true;
        let mut frames = Vec::new();
        if let Some(event) = codex_visual::coding_plan_rate_limit_event(&self.visual) {
            frames.push(event);
        }
        if !self.suppress_status {
            frames.extend(codex_visual::status_message_events(
                &self.visual,
                response_id,
                self.ttfs_ms,
            ));
        }
        frames
    }
}

fn codex_status_dedupe_key(
    turn_id: Option<&str>,
    visual: &codex_visual::CodexVisualContext,
) -> String {
    format!(
        "{}|{}",
        turn_id.unwrap_or("__unknown_turn__"),
        codex_visual::route_signature(visual)
    )
}

fn codex_status_ttl(turn_id: Option<&str>) -> std::time::Duration {
    if turn_id.is_some() {
        std::time::Duration::from_secs(30 * 60)
    } else {
        std::time::Duration::from_secs(90)
    }
}

fn should_emit_codex_route_status(
    state: &AppState,
    turn_id: Option<&str>,
    visual: &codex_visual::CodexVisualContext,
) -> bool {
    state.remember_codex_status_key(
        codex_status_dedupe_key(turn_id, visual),
        codex_status_ttl(turn_id),
    )
}

fn build_upstream_ws_request(
    prepared: &PreparedForward,
    client_headers: &HeaderMap,
) -> Result<Request<()>, String> {
    let mut url = prepared.provider.base_url.trim_end_matches('/').to_string();
    if !url.ends_with("/responses") {
        url.push_str("/responses");
    }
    let mut url = url
        .parse::<http::Uri>()
        .map_err(|e| format!("invalid upstream URL: {e}"))?
        .to_string();
    if let Some(rest) = url.strip_prefix("https://") {
        url = format!("wss://{rest}");
    } else if let Some(rest) = url.strip_prefix("http://") {
        url = format!("ws://{rest}");
    }

    let mut builder = Request::builder().uri(url);
    {
        let headers = builder
            .headers_mut()
            .ok_or_else(|| "failed to build websocket headers".to_string())?;
        copy_client_header(headers, client_headers, "originator");
        copy_client_header(headers, client_headers, "user-agent");
        copy_client_header(headers, client_headers, "x-client-request-id");
        copy_client_header(headers, client_headers, "session_id");
        copy_client_header(headers, client_headers, "thread_id");
        copy_client_header(headers, client_headers, "x-codex-window-id");
        copy_client_header(headers, client_headers, "x-codex-beta-features");
        if let Some(secret) = prepared.secret.as_ref() {
            let value = http::HeaderValue::from_str(&format!("Bearer {secret}"))
                .map_err(|e| format!("invalid bearer token header: {e}"))?;
            headers.insert(http::header::AUTHORIZATION, value);
            if let Some(account_id) = auth_fingerprint::chatgpt_account_id_from_access_token(secret)
            {
                if let Ok(value) = http::HeaderValue::from_str(&account_id) {
                    headers.insert("ChatGPT-Account-ID", value);
                }
            }
        }
        if !headers.contains_key("ChatGPT-Account-ID") {
            copy_client_header(headers, client_headers, "chatgpt-account-id");
        }
        headers.insert(
            "OpenAI-Beta",
            http::HeaderValue::from_static(WS_V2_BETA_HEADER_VALUE),
        );
    }
    builder.body(()).map_err(|e| e.to_string())
}

fn copy_client_header(dst: &mut http::HeaderMap, src: &HeaderMap, name: &'static str) {
    let Some(value) = src.get(name) else {
        return;
    };
    let Ok(header_name) = http::HeaderName::from_bytes(name.as_bytes()) else {
        return;
    };
    let Ok(header_value) = http::HeaderValue::from_bytes(value.as_bytes()) else {
        return;
    };
    dst.insert(header_name, header_value);
}

async fn send_prepare_error(socket: &mut WebSocket, state: &AppState, err: PreparedForwardError) {
    match err {
        PreparedForwardError::Db(message) => {
            let payload = transforms::codex_response_proxy_fault_event(
                "resp-proxy-error",
                "proxy_db_error",
                &message,
            );
            let _ = send_text(socket, &payload).await;
        }
        PreparedForwardError::NoCandidates {
            log_id,
            started_at,
            started_instant,
            app,
            requested_model,
            log_ctx,
            request_snapshot,
        } => {
            let ctx = vec![format!(
                "context · cred — · wire {} · model {}",
                forward::wire_as_str(log_ctx.wire),
                requested_model
            )];
            let message =
                forward::compose_routing_error_message("no provider matches request shape", &ctx);
            let payload =
                transforms::codex_response_failed_event("resp-proxy-error", 503, &message);
            let log = forward::request_log_from_parts(
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
                Some(message),
                Usage::default(),
                request_snapshot,
                None,
            );
            forward::persist_request_log(state, log);
            let _ = send_text(socket, &payload).await;
        }
        PreparedForwardError::Exhausted {
            log_id,
            started_at,
            started_instant,
            app,
            requested_model,
            log_ctx,
            request_snapshot,
            message,
        } => {
            let payload =
                transforms::codex_response_failed_event("resp-proxy-error", 503, &message);
            let log = forward::request_log_from_parts(
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
                Some(message),
                Usage::default(),
                request_snapshot,
                None,
            );
            forward::persist_request_log(state, log);
            let _ = send_text(socket, &payload).await;
        }
    }
}

fn finalize_log(
    state: &AppState,
    prepared: PreparedForward,
    mut log: vibe_protocol::RequestLog,
    status_code: Option<i32>,
    error: Option<String>,
    response_body: Option<String>,
    client_response_body: Option<String>,
) {
    log.status_code = status_code;
    log.upstream_http_status = status_code;
    log.error = error;
    log.response_body = response_body;
    log.client_response_body = client_response_body;
    log.latency_ms = Some(prepared.started_instant.elapsed().as_millis() as i64);
    forward::persist_request_log(state, log);
}

async fn send_text(socket: &mut WebSocket, text: &str) -> Result<(), axum::Error> {
    socket.send(Message::Text(text.to_string())).await
}

fn append_trace(acc: &mut String, line: &str) {
    if !acc.is_empty() {
        acc.push('\n');
    }
    acc.push_str(line);
}

fn response_id_from_request(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("previous_response_id")
        .or_else(|| v.pointer("/response/previous_response_id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
}

fn codex_frame_is_response_created(frame_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(frame_json)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .is_some_and(|t| t == "response.created")
}

fn response_created_id(frame_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(frame_json).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("response.created") {
        return None;
    }
    v.pointer("/response/id")
        .and_then(|id| id.as_str())
        .map(str::to_string)
}

fn update_usage_and_terminal(frame_json: &str, usage: &mut Usage) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(frame_json) else {
        return false;
    };
    let typ = v.get("type").and_then(|t| t.as_str());
    if let Some(u) = v.pointer("/response/usage").or_else(|| v.get("usage")) {
        if let Some(n) = u
            .get("input_tokens")
            .or_else(|| u.get("prompt_tokens"))
            .and_then(|x| x.as_i64())
        {
            usage.input_tokens = n;
        }
        if let Some(n) = u
            .get("output_tokens")
            .or_else(|| u.get("completion_tokens"))
            .and_then(|x| x.as_i64())
        {
            usage.output_tokens = n;
        }
        if let Some(n) = u
            .pointer("/input_tokens_details/cached_tokens")
            .or_else(|| u.pointer("/prompt_tokens_details/cached_tokens"))
            .or_else(|| u.get("cached_input_tokens"))
            .and_then(|x| x.as_i64())
        {
            usage.cache_read_tokens = n;
        }
    }
    matches!(
        typ,
        Some("response.completed" | "response.failed" | "response.done")
    )
}

fn response_failed_message(frame_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(frame_json).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("response.failed") {
        return None;
    }
    v.pointer("/response/error/message")
        .and_then(|m| m.as_str())
        .map(str::to_string)
        .or_else(|| Some("upstream response.failed".into()))
}

fn wrapped_error_status(frame_json: &str) -> Option<i32> {
    let v: serde_json::Value = serde_json::from_str(frame_json).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("error") {
        return None;
    }
    v.get("status")
        .or_else(|| v.get("status_code"))
        .and_then(|s| s.as_i64())
        .map(|s| s as i32)
        .or(Some(StatusCode::BAD_GATEWAY.as_u16() as i32))
}

fn websocket_connect_error_status_body(err: &TungsteniteError) -> (i32, String) {
    match err {
        TungsteniteError::Http(response) => {
            let status = response.status().as_u16() as i32;
            let body = response
                .body()
                .as_ref()
                .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
                .unwrap_or_else(|| format!("upstream websocket handshake failed: HTTP {status}"));
            (status, body)
        }
        _ => (502, err.to_string()),
    }
}
