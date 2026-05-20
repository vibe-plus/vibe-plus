use super::*;
use crate::stream_trace::StreamTraceStats;

pub(super) async fn codex_responses_handler(
    ws_upgrade: Option<WebSocketUpgrade>,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(upgrade) = ws_upgrade {
        state.codex_transport.ws_opened();
        let mut ws_headers = headers.clone();
        ws_headers.insert(
            HeaderName::from_static("x-vibe-client-transport"),
            HeaderValue::from_static("ws"),
        );
        upgrade.on_upgrade(move |socket| codex_ws_bridge(socket, state, ws_headers))
    } else {
        // Plain HTTP POST — Codex may send the WS-envelope format even over HTTP.
        // Strip the {"type":"response.create",...} envelope so forward() sees a
        // clean Responses API body with a top-level "model" field.
        let streaming = request_body_streams(&body);
        let client_transport = if streaming { "http-sse" } else { "http" };
        state.codex_transport.http_response_request(streaming);
        let mut headers = headers;
        headers.insert(
            HeaderName::from_static("x-vibe-client-transport"),
            HeaderValue::from_static(client_transport),
        );
        let stripped = transforms::strip_ws_envelope(&body);
        let thread_source = codex_summary::thread_source_from_headers(&headers)
            .or_else(|| codex_summary::thread_source_from_request(&body))
            .or_else(|| codex_summary::thread_source_from_request(&stripped));
        // Begin-slot is meant for user-facing thread display. Subagent
        // threads run as background workers but Codex Desktop renders their
        // assistant messages inline with the main thread, so emitting a
        // begin slot for each subagent request causes "multiple begin slots
        // on the main side" (see CLAUDE.md → Codex Protocol → Thread).
        let is_subagent = thread_source == Some(codex_summary::CodexThreadSource::Subagent);
        let should_show_status =
            !is_subagent && transforms::responses_input_ends_with_user_message(&stripped);
        let turn_id = codex_summary::turn_id_from_headers(&headers)
            .or_else(|| codex_summary::turn_id_from_request(&body))
            .or_else(|| codex_summary::turn_id_from_request(&stripped));
        let thread_id = codex_summary::thread_id_from_headers(&headers)
            .or_else(|| codex_summary::thread_id_from_request(&body))
            .or_else(|| codex_summary::thread_id_from_request(&stripped));
        let request_started_instant = Instant::now();
        let upstream = forward::forward(
            state.clone(),
            Wire::OpenaiResponses,
            None,
            headers,
            stripped,
            Some("codex-v1".into()),
        )
        .await;
        codex_plain_http_maybe_chat_to_responses_sse(
            state,
            upstream,
            request_started_instant,
            should_show_status,
            turn_id,
            thread_id,
        )
        .await
    }
}

pub(super) fn request_body_streams(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/stream")
                .or_else(|| v.pointer("/response/stream"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CodexHttpSseMode {
    Undecided,
    Passthrough,
    C2r,
}

#[derive(Clone, Debug)]
pub(super) struct CodexStatusInjection {
    visual: codex_visual::CodexVisualContext,
    ttfs_ms: i64,
    emitted: bool,
    suppress_status: bool,
    client_kind: codex_summary::CodexClientKind,
}

impl CodexStatusInjection {
    pub(super) fn new(
        visual: Option<codex_visual::CodexVisualContext>,
        ttfs_ms: i64,
        suppress_status: bool,
        client_kind: codex_summary::CodexClientKind,
    ) -> Option<Self> {
        visual.map(|visual| Self {
            visual,
            ttfs_ms,
            emitted: false,
            suppress_status,
            client_kind,
        })
    }

    pub(super) fn next_frames(&mut self, response_id: &str) -> Vec<String> {
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
                self.client_kind,
            ));
        }
        frames
    }
}

pub(super) fn codex_status_dedupe_key(
    turn_id: Option<&str>,
    visual: &codex_visual::CodexVisualContext,
) -> String {
    format!(
        "{}|{}",
        turn_id.unwrap_or("__unknown_turn__"),
        codex_visual::route_signature(visual)
    )
}

pub(super) fn codex_status_ttl(turn_id: Option<&str>) -> Duration {
    if turn_id.is_some() {
        Duration::from_secs(30 * 60)
    } else {
        Duration::from_secs(90)
    }
}

pub(super) fn should_emit_codex_route_status(
    state: &AppState,
    turn_id: Option<&str>,
    visual: &codex_visual::CodexVisualContext,
) -> bool {
    state.remember_codex_status_key(
        codex_status_dedupe_key(turn_id, visual),
        codex_status_ttl(turn_id),
    )
}

#[cfg(test)]
mod codex_status_tests {
    use super::*;

    fn visual(upstream_model: &str) -> codex_visual::CodexVisualContext {
        codex_visual::CodexVisualContext {
            provider_id: "p1".into(),
            credential_id: Some("cred-1".into()),
            requested_model: "gpt-5.5".into(),
            upstream_model: upstream_model.into(),
            ..Default::default()
        }
    }

    #[test]
    fn extracts_turn_id_from_ws_envelope_client_metadata() {
        let body = serde_json::json!({
            "type": "response.create",
            "response": {
                "model": "gpt-5.5",
                "client_metadata": {
                    "x-codex-turn-metadata": "{\"turn_id\":\"turn-123\",\"turn_started_at_unix_ms\":1}"
                }
            }
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        assert_eq!(
            codex_summary::turn_id_from_request(&bytes).as_deref(),
            Some("turn-123")
        );
    }

    #[test]
    fn dedupe_key_changes_when_route_changes() {
        let first = codex_status_dedupe_key(Some("turn-1"), &visual("gpt-5.5"));
        let same = codex_status_dedupe_key(Some("turn-1"), &visual("gpt-5.5"));
        let changed = codex_status_dedupe_key(Some("turn-1"), &visual("kimi-k2"));
        assert_eq!(first, same);
        assert_ne!(first, changed);
    }

    #[test]
    fn unknown_turn_uses_short_ttl_fallback() {
        assert!(codex_status_ttl(None) < codex_status_ttl(Some("turn-1")));
    }
}

pub(super) fn codex_frame_is_response_created(frame_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(frame_json)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .is_some_and(|t| t == "response.created")
}

pub(super) fn codex_sse_block_has_response_created(block: &str) -> bool {
    block.lines().any(|raw_line| {
        let line = raw_line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            return false;
        };
        codex_frame_is_response_created(payload.trim())
    })
}

/// Extract `response.id` from a `response.created` frame JSON, so that injected
/// status events use the upstream's actual response_id in Passthrough mode.
pub(super) fn extract_response_created_id(frame_json: &str) -> Option<String> {
    let v = serde_json::from_str::<serde_json::Value>(frame_json).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("response.created") {
        return None;
    }
    v.pointer("/response/id")
        .and_then(|id| id.as_str())
        .map(str::to_string)
}

pub(super) fn codex_sse_block_extract_response_id(block: &str) -> Option<String> {
    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        if let Some(id) = extract_response_created_id(payload.trim()) {
            return Some(id);
        }
    }
    None
}

/// Inspect one SSE frame (delimiter `\n\n`) and decide whether it looks like **Chat Completions** JSON.
///
/// Returns:
/// - `Some(true)`  — contains `choices` (typical upstream Chat SSE)
/// - `Some(false)` — contains structured JSON without `choices` (likely Responses-native)
/// - `None`        — heartbeat / comments / `[DONE]` / empty — stay undecided
pub(super) fn classify_codex_upstream_sse_frame(block: &str) -> Option<bool> {
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

/// Codex **`/codex/v1/responses` HTTP**: if upstream is Chat SSE, convert **SSE -> Responses SSE**.
pub(super) async fn codex_plain_http_maybe_chat_to_responses_sse(
    state: AppState,
    upstream: Response,
    request_started_instant: Instant,
    should_show_status: bool,
    summary_turn_id: Option<String>,
    summary_thread_id: Option<String>,
) -> Response {
    let (parts, body) = upstream.into_parts();
    let log_row_id = parts
        .extensions
        .get::<VibeRequestLogId>()
        .map(|x| x.0.clone());
    let visual = parts
        .extensions
        .get::<VibeCodexVisual>()
        .map(|x| x.0.clone());
    let codex_client_kind = parts
        .extensions
        .get::<VibeCodexClientKind>()
        .map(|x| x.0)
        .unwrap_or(codex_summary::CodexClientKind::Unknown);

    if !parts.status.is_success() {
        return Response::from_parts(parts, body);
    }

    let content_type_hdr = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    tracing::debug!(
        log_row_id = ?log_row_id,
        summary_turn_id = ?summary_turn_id,
        summary_thread_id = ?summary_thread_id,
        status = %parts.status,
        content_type = %content_type_hdr,
        client_kind = ?codex_client_kind,
        "codex http-sse wrapper start"
    );

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
    out_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );

    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(96);
    tokio::spawn(async move {
        let mut trace = String::new();
        let mut mode = CodexHttpSseMode::Undecided;
        let mut status_injection: Option<CodexStatusInjection> = None;
        let debug_summary_turn_id = summary_turn_id.clone();
        let debug_summary_thread_id = summary_thread_id.clone();
        let mut summary_injection = codex_summary::SummaryAccumulator::new_for_turn(
            state.codex_summary_config(),
            codex_client_kind,
            Some(state.clone()),
            summary_turn_id,
            summary_thread_id,
            visual
                .as_ref()
                .map(|v| v.upstream_model.clone())
                .unwrap_or_default(),
        );
        tracing::debug!(
            log_row_id = ?log_row_id,
            summary_turn_id = ?debug_summary_turn_id,
            summary_thread_id = ?debug_summary_thread_id,
            visual = visual.is_some(),
            "codex http-sse stream task start"
        );
        let mut trace_stats = StreamTraceStats::new("sse", "chat_to_responses");

        #[inline]
        async fn emit_raw_frame(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            trace_stats: &mut StreamTraceStats,
            started: &Instant,
            block: &str,
        ) -> bool {
            let mut chunk = block.to_owned();
            chunk.push_str("\n\n");
            append_codex_ws_client_trace(trace, chunk.trim_end());
            let bytes = chunk.len();
            if tx.send(Ok(Bytes::from(chunk))).await.is_ok() {
                trace_stats.record_client_chunk(started, bytes);
                true
            } else {
                trace_stats.finish("downstream_closed");
                false
            }
        }

        #[inline]
        async fn emit_c2r_frame(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            trace_stats: &mut StreamTraceStats,
            started: &Instant,
            frame_json: &str,
        ) -> bool {
            append_codex_ws_client_trace(trace, frame_json);
            // Include the SSE `event:` line so Codex parses it the same as
            // upstream Passthrough events. Without it, frames default to the
            // "message" event type and stricter SSE clients may drop them.
            let event_type = serde_json::from_str::<serde_json::Value>(frame_json)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string));
            let sse_line = match event_type {
                Some(t) => format!("event: {t}\ndata: {frame_json}\n\n"),
                None => format!("data: {frame_json}\n\n"),
            };
            let bytes = sse_line.len();
            if tx.send(Ok(Bytes::from(sse_line))).await.is_ok() {
                trace_stats.record_client_chunk(started, bytes);
                true
            } else {
                trace_stats.finish("downstream_closed");
                false
            }
        }

        #[inline]
        async fn flush_one_sse_block(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            trace_stats: &mut StreamTraceStats,
            started: &Instant,
            mode: &mut CodexHttpSseMode,
            event_block: &str,
            session_id: &str,
            item_id: &str,
            accumulator: &mut transforms::ChatCompletionsC2rAccumulator,
            terminal_done: &mut bool,
            status_injection: &mut Option<CodexStatusInjection>,
            summary_injection: &mut codex_summary::SummaryAccumulator,
        ) -> bool {
            loop {
                match *mode {
                    CodexHttpSseMode::Undecided => {
                        match classify_codex_upstream_sse_frame(event_block) {
                            Some(true) => {
                                *mode = CodexHttpSseMode::C2r;
                                continue;
                            }
                            Some(false) => {
                                *mode = CodexHttpSseMode::Passthrough;
                                continue;
                            }
                            None => {
                                return emit_raw_frame(
                                    tx,
                                    trace,
                                    trace_stats,
                                    started,
                                    event_block,
                                )
                                .await;
                            }
                        }
                    }
                    CodexHttpSseMode::Passthrough => {
                        let block_to_forward = summary_injection
                            .maybe_append_to_sse_block(
                                event_block,
                                started.elapsed().as_millis() as i64,
                            )
                            .unwrap_or_else(|| event_block.to_owned());
                        if block_to_forward != event_block {
                            tracing::debug!(
                                mode = "passthrough",
                                original_len = event_block.len(),
                                forwarded_len = block_to_forward.len(),
                                contains_vibe = block_to_forward.contains("Vibe+"),
                                contains_summary_delta = block_to_forward
                                    .contains("\"type\":\"response.output_text.delta\"")
                                    || block_to_forward
                                        .contains("\"type\":\"response.output_text.delta\""),
                                event_preview = %block_to_forward
                                    .chars()
                                    .take(240)
                                    .collect::<String>(),
                                "codex http-sse summary transformed block"
                            );
                        }
                        if !emit_raw_frame(tx, trace, trace_stats, started, &block_to_forward).await
                        {
                            return false;
                        }
                        if codex_sse_block_has_response_created(event_block) {
                            let effective_id = codex_sse_block_extract_response_id(event_block)
                                .unwrap_or_else(|| session_id.to_string());
                            if let Some(injection) = status_injection.as_mut() {
                                for frame in injection.next_frames(&effective_id) {
                                    trace_stats.mark_status_injected();
                                    if !emit_c2r_frame(tx, trace, trace_stats, started, &frame)
                                        .await
                                    {
                                        return false;
                                    }
                                }
                            }
                        }
                        return true;
                    }
                    CodexHttpSseMode::C2r => {
                        for ws_frame in summary_injection.maybe_append_to_frame_batch(
                            codex_sse_block_to_ws_frames(
                                event_block,
                                session_id,
                                item_id,
                                accumulator,
                                terminal_done,
                            ),
                            started.elapsed().as_millis() as i64,
                        ) {
                            if ws_frame.contains("Vibe+")
                                || ws_frame.contains("\"type\":\"response.output_text.delta\"")
                            {
                                tracing::debug!(
                                    mode = "c2r",
                                    frame_len = ws_frame.len(),
                                    frame_preview = %ws_frame
                                        .chars()
                                        .take(240)
                                        .collect::<String>(),
                                    "codex http-sse summary emitted frame"
                                );
                            }
                            if !emit_c2r_frame(tx, trace, trace_stats, started, &ws_frame).await {
                                return false;
                            }
                            if codex_frame_is_response_created(&ws_frame) {
                                if let Some(injection) = status_injection.as_mut() {
                                    for frame in injection.next_frames(session_id) {
                                        trace_stats.mark_status_injected();
                                        if !emit_c2r_frame(tx, trace, trace_stats, started, &frame)
                                            .await
                                        {
                                            return false;
                                        }
                                    }
                                }
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
                Ok(bytes) => {
                    trace_stats.record_upstream_chunk(&request_started_instant, bytes.len());
                    if status_injection.is_none() {
                        status_injection = CodexStatusInjection::new(
                            visual.clone(),
                            request_started_instant.elapsed().as_millis() as i64,
                            !should_show_status || !state.codex_route_status_enabled(),
                            codex_client_kind,
                        );
                    }
                    buf.push_str(&String::from_utf8_lossy(&bytes));
                }
                Err(_) => {
                    trace_stats.finish_error("upstream_read_error", "body stream read error");
                    stream_broken = true;
                    break;
                }
            }

            while let Some(end) = buf.find("\n\n") {
                let block = buf[..end].to_string();
                buf.drain(..end + 2);
                trace_stats.record_sse_block(&request_started_instant, &block);
                if !flush_one_sse_block(
                    &tx,
                    &mut trace,
                    &mut trace_stats,
                    &request_started_instant,
                    &mut mode,
                    &block,
                    &session_id,
                    &item_id,
                    &mut accumulator,
                    &mut terminal_done,
                    &mut status_injection,
                    &mut summary_injection,
                )
                .await
                {
                    drop(tx);
                    persist_codex_client_response_body(
                        &state,
                        log_row_id,
                        trace,
                        Some(trace_stats),
                        debug_summary_turn_id,
                        debug_summary_thread_id,
                    )
                    .await;
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
                trace_stats.record_sse_block(&request_started_instant, &block);
                let _ = flush_one_sse_block(
                    &tx,
                    &mut trace,
                    &mut trace_stats,
                    &request_started_instant,
                    &mut mode,
                    &block,
                    &session_id,
                    &item_id,
                    &mut accumulator,
                    &mut terminal_done,
                    &mut status_injection,
                    &mut summary_injection,
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
            trace_stats.mark_terminal_injected();
            let bytes = sse_line.len();
            if tx.send(Ok(Bytes::from(sse_line))).await.is_ok() {
                trace_stats.record_client_chunk(&request_started_instant, bytes);
            }
        }

        if trace_stats.terminal_seen() {
            trace_stats.finish("completed");
        } else if trace_stats.end_reason().is_none() {
            trace_stats.finish("upstream_eof");
        }
        drop(tx);
        tracing::debug!(
            log_row_id = ?log_row_id,
            trace_len = trace.len(),
            terminal_seen = trace_stats.terminal_seen(),
            end_reason = ?trace_stats.end_reason(),
            "codex http-sse stream task finished, persisting client trace"
        );
        persist_codex_client_response_body(
            &state,
            log_row_id,
            trace,
            Some(trace_stats),
            debug_summary_turn_id,
            debug_summary_thread_id,
        )
        .await;
    });

    let mut out = Response::new(Body::from_stream(ReceiverStream::new(rx)));
    *out.status_mut() = parts.status;
    *out.version_mut() = parts.version;
    *out.headers_mut() = out_headers;
    out
}

/// One SSE event block (text between `\n\n`) → Codex WebSocket text frames.
/// Accumulate the full JSON-Lines trace forwarded to Codex so `client_response_body` can be fully persisted without truncation.
pub(super) fn append_codex_ws_client_trace(acc: &mut String, json_line: &str) {
    if !acc.is_empty() {
        acc.push('\n');
    }
    acc.push_str(json_line);
}

pub(super) async fn persist_codex_client_response_body(
    state: &AppState,
    row_id: Option<String>,
    trace: String,
    stats: Option<StreamTraceStats>,
    turn_id: Option<String>,
    thread_id: Option<String>,
) {
    let Some(row_id) = row_id else {
        return;
    };
    let mut log = vibe_protocol::RequestLog {
        id: row_id.clone(),
        started_at: 0,
        app: None,
        provider_id: None,
        thread_id,
        turn_id,
        trace_id: None,
        session_id: None,
        requested_model: None,
        upstream_model: None,
        status_code: None,
        error: None,
        latency_ms: None,
        first_token_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        reasoning_tokens: 0,
        cache_creation_5m_tokens: 0,
        cache_creation_1h_tokens: 0,
        audio_input_tokens: 0,
        audio_output_tokens: 0,
        accepted_prediction_tokens: 0,
        rejected_prediction_tokens: 0,
        cost_items: None,
        estimated_cost_usd: "0".into(),
        wire: None,
        route_prefix: None,
        credential_id: None,
        cb_key: None,
        upstream_http_status: None,
        upstream_error_preview: None,
        dedupe_key: None,
        client_transport: None,
        request_headers: None,
        request_body: None,
        response_body: None,
        client_response_body: (!trace.is_empty()).then_some(trace),
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
    if let Some(stats) = stats {
        stats.apply_to_log(&mut log);
    }
    log.status_code = Some(200);
    log.upstream_http_status = Some(200);
    tracing::debug!(
        row_id = %row_id,
        turn_id = ?log.turn_id,
        thread_id = ?log.thread_id,
        trace_len = log.client_response_body.as_ref().map(|s| s.len()).unwrap_or(0),
        stream_kind = ?log.stream_kind,
        terminal_seen = ?log.stream_terminal_seen,
        end_reason = ?log.stream_end_reason,
        client_chunks = log.client_chunk_count,
        client_bytes = log.client_bytes,
        "persist codex client response trace begin"
    );
    let Some(observability) = state.observability.clone() else {
        tracing::debug!(
            row_id = %row_id,
            "skip codex client response trace persistence: observability store unavailable"
        );
        return;
    };
    for attempt in 0..20 {
        let store = observability.clone();
        let log = log.clone();
        let result = tokio::task::spawn_blocking(move || store.update_request_client_trace(&log))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("join error: {e}")));
        match result {
            Ok(()) => {
                tracing::debug!(
                    row_id = %row_id,
                    attempt,
                    "persist codex client response trace ok"
                );
                return;
            }
            Err(e) if attempt < 19 && e.to_string().contains("no row for id") => {
                tracing::debug!(
                    row_id = %row_id,
                    attempt,
                    error = %e,
                    "persist codex client response trace waiting for request log row"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => {
                tracing::warn!(row_id = %row_id, ?e, "persist codex client response trace failed");
                return;
            }
        }
    }
    tracing::warn!(
        row_id = %row_id,
        "persist codex client response trace gave up waiting for request log row"
    );
}

pub(super) fn codex_sse_block_to_ws_frames(
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
                transforms::chat_event_to_responses_events(data, session_id, item_id, accumulator)
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
