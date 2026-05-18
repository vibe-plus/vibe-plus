use super::*;
use crate::stream_trace::StreamTraceStats;

pub(super) struct CodexWsActiveGuard {
    state: AppState,
}

impl Drop for CodexWsActiveGuard {
    fn drop(&mut self) {
        self.state.codex_transport.ws_closed();
    }
}

pub(super) async fn codex_ws_bridge(mut socket: WebSocket, state: AppState, ws_headers: HeaderMap) {
    let _active_guard = CodexWsActiveGuard {
        state: state.clone(),
    };
    // Codex keeps the WS connection alive across multiple turns (tool execution
    // cycles).  We loop here to handle each `response.create` message that
    // arrives on the same connection.
    loop {
        // 1. Wait for the next request message from Codex.
        let body_bytes: Bytes = loop {
            match socket.recv().await {
                Some(Ok(Message::Text(t))) => break Bytes::from(t.into_bytes()),
                Some(Ok(Message::Binary(b))) => break Bytes::from(b),
                Some(Ok(Message::Close(_))) | None => return,
                Some(Ok(_)) => continue, // ping/pong — ignore
                Some(Err(_)) => return,
            }
        };
        state.codex_transport.ws_request();

        // 2. Strip the WS envelope: {"type":"response.create", ...} → {...}
        {
            let preview = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(300)]);
            tracing::debug!(preview = %preview, "codex ws body (first 300 bytes)");
        }
        let stripped = transforms::strip_ws_envelope(&body_bytes);
        let thread_source = codex_summary::thread_source_from_headers(&ws_headers)
            .or_else(|| codex_summary::thread_source_from_request(&body_bytes))
            .or_else(|| codex_summary::thread_source_from_request(&stripped));
        // Suppress the begin slot for subagent requests so they don't bleed
        // into the main thread's chat display. See CLAUDE.md → Codex
        // Protocol → Thread.
        let is_subagent = thread_source == Some(codex_summary::CodexThreadSource::Subagent);
        let should_show_status =
            !is_subagent && transforms::responses_input_ends_with_user_message(&stripped);
        let turn_id = codex_summary::turn_id_from_headers(&ws_headers)
            .or_else(|| codex_summary::turn_id_from_request(&body_bytes))
            .or_else(|| codex_summary::turn_id_from_request(&stripped));
        let thread_id = codex_summary::thread_id_from_headers(&ws_headers)
            .or_else(|| codex_summary::thread_id_from_request(&body_bytes))
            .or_else(|| codex_summary::thread_id_from_request(&stripped));

        match crate::codex_upstream_ws::try_forward_official_codex_ws(
            &mut socket,
            state.clone(),
            ws_headers.clone(),
            body_bytes.clone(),
            StatusDecision {
                should_show_status,
                turn_id: turn_id.clone(),
                thread_id: thread_id.clone(),
                is_failover: false,
            },
        )
        .await
        {
            UpstreamWsOutcome::Forwarded => continue,
            UpstreamWsOutcome::Fallback(body) => {
                tracing::debug!(
                    bytes = body.len(),
                    "codex ws selected non-official upstream; using HTTP bridge fallback"
                );
            }
        }

        // For WS mode, always request streaming from the upstream.
        let http_body: Bytes = {
            let mut val: serde_json::Value = serde_json::from_slice(&stripped)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            if let Some(obj) = val.as_object_mut() {
                obj.insert("stream".into(), serde_json::Value::Bool(true));
            }
            serde_json::to_vec(&val)
                .map(Bytes::from)
                .unwrap_or(stripped.clone())
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
        let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());

        // 3. Build minimal headers for the forward call.
        let mut req_headers = ws_headers.clone();
        req_headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        // 4. Forward to upstream; get back an axum Response.
        let request_started_instant = Instant::now();
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
        let stream_log_row_id: Option<String> = None;
        let visual = parts
            .extensions
            .get::<VibeCodexVisual>()
            .map(|x| x.0.clone());
        let codex_client_kind = parts
            .extensions
            .get::<VibeCodexClientKind>()
            .map(|x| x.0)
            .unwrap_or(codex_summary::CodexClientKind::Unknown);
        let suppress_status = !state.codex_route_status_enabled()
            || visual
                .as_ref()
                .map(|visual| {
                    !should_show_status
                        || !should_emit_codex_route_status(&state, turn_id.as_deref(), visual)
                })
                .unwrap_or(false);
        let mut client_ws_trace = String::new();
        let mut trace_stats = StreamTraceStats::new("websocket", "responses_to_ws");
        let mut summary_injection = codex_summary::SummaryAccumulator::new_for_turn(
            state.codex_summary_config(),
            codex_client_kind,
            Some(state.clone()),
            turn_id.clone(),
            thread_id.clone(),
            visual
                .as_ref()
                .map(|v| v.upstream_model.clone())
                .unwrap_or_default(),
        );

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
            trace_stats.mark_terminal_injected();
            if socket.send(Message::Text(payload.clone())).await.is_ok() {
                trace_stats.record_client_chunk(&request_started_instant, payload.len());
            } else {
                trace_stats.finish("downstream_closed");
            }
            trace_stats.finish("completed");
            persist_codex_client_response_body(
                &state,
                stream_log_row_id,
                client_ws_trace,
                Some(trace_stats),
            )
            .await;
            continue;
        }

        let content_type = parts
            .headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let is_sse = content_type.contains("event-stream");

        if is_sse {
            // 6a. Streaming SSE: parse each event and emit as Responses API WS messages.
            use futures_util::StreamExt as _;
            let mut stream = body.into_data_stream();
            let mut buf = String::new();
            let mut accumulator = transforms::ChatCompletionsC2rAccumulator::default();
            let mut terminal_done = false;
            let mut stream_broken = false;
            let mut status_injection: Option<CodexStatusInjection> = None;

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        trace_stats.record_upstream_chunk(&request_started_instant, bytes.len());
                        if status_injection.is_none() {
                            status_injection = CodexStatusInjection::new(
                                visual.clone(),
                                request_started_instant.elapsed().as_millis() as i64,
                                suppress_status,
                            );
                        }
                        // Never drop bytes on a non-UTF8 chunk boundary (reqwest can split codepoints).
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Err(_) => {
                        trace_stats.finish_error("upstream_read_error", "body stream read error");
                        stream_broken = true;
                        break;
                    }
                }

                // Consume complete SSE events (terminated by blank line).
                while let Some(event_end) = buf.find("\n\n") {
                    let event_block = buf[..event_end].to_string();
                    buf.drain(..event_end + 2);
                    trace_stats.record_sse_block(&request_started_instant, &event_block);
                    for event_str in summary_injection.maybe_append_to_frame_batch(
                        codex_sse_block_to_ws_frames(
                            &event_block,
                            &session_id,
                            &item_id,
                            &mut accumulator,
                            &mut terminal_done,
                        ),
                        request_started_instant.elapsed().as_millis() as i64,
                    ) {
                        tracing::debug!(event = %&event_str[..event_str.len().min(200)], "codex ws → client event");
                        append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                        let is_created = codex_frame_is_response_created(&event_str);
                        if socket.send(Message::Text(event_str.clone())).await.is_err() {
                            trace_stats.finish("downstream_closed");
                            persist_codex_client_response_body(
                                &state,
                                stream_log_row_id,
                                client_ws_trace,
                                Some(trace_stats),
                            )
                            .await;
                            return; // client disconnected
                        } else {
                            trace_stats
                                .record_client_chunk(&request_started_instant, event_str.len());
                        }
                        if is_created {
                            let effective_id = extract_response_created_id(&event_str)
                                .unwrap_or_else(|| session_id.clone());
                            if let Some(injection) = status_injection.as_mut() {
                                for injected in injection.next_frames(&effective_id) {
                                    tracing::debug!(event = %&injected[..injected.len().min(200)], "codex ws injected status → client event");
                                    append_codex_ws_client_trace(&mut client_ws_trace, &injected);
                                    trace_stats.mark_status_injected();
                                    if socket.send(Message::Text(injected.clone())).await.is_err() {
                                        trace_stats.finish("downstream_closed");
                                        persist_codex_client_response_body(
                                            &state,
                                            stream_log_row_id,
                                            client_ws_trace,
                                            Some(trace_stats),
                                        )
                                        .await;
                                        return;
                                    } else {
                                        trace_stats.record_client_chunk(
                                            &request_started_instant,
                                            injected.len(),
                                        );
                                    }
                                }
                            }
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
                    trace_stats.record_sse_block(&request_started_instant, &event_block);
                    for event_str in summary_injection.maybe_append_to_frame_batch(
                        codex_sse_block_to_ws_frames(
                            &event_block,
                            &session_id,
                            &item_id,
                            &mut accumulator,
                            &mut terminal_done,
                        ),
                        request_started_instant.elapsed().as_millis() as i64,
                    ) {
                        tracing::debug!(event = %&event_str[..event_str.len().min(200)], "codex ws flush → client event");
                        append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                        let is_created = codex_frame_is_response_created(&event_str);
                        if socket.send(Message::Text(event_str.clone())).await.is_err() {
                            trace_stats.finish("downstream_closed");
                            persist_codex_client_response_body(
                                &state,
                                stream_log_row_id,
                                client_ws_trace,
                                Some(trace_stats),
                            )
                            .await;
                            return;
                        } else {
                            trace_stats
                                .record_client_chunk(&request_started_instant, event_str.len());
                        }
                        if is_created {
                            let effective_id = extract_response_created_id(&event_str)
                                .unwrap_or_else(|| session_id.clone());
                            if let Some(injection) = status_injection.as_mut() {
                                for injected in injection.next_frames(&effective_id) {
                                    tracing::debug!(event = %&injected[..injected.len().min(200)], "codex ws injected flush status → client event");
                                    append_codex_ws_client_trace(&mut client_ws_trace, &injected);
                                    trace_stats.mark_status_injected();
                                    if socket.send(Message::Text(injected.clone())).await.is_err() {
                                        trace_stats.finish("downstream_closed");
                                        persist_codex_client_response_body(
                                            &state,
                                            stream_log_row_id,
                                            client_ws_trace,
                                            Some(trace_stats),
                                        )
                                        .await;
                                        return;
                                    } else {
                                        trace_stats.record_client_chunk(
                                            &request_started_instant,
                                            injected.len(),
                                        );
                                    }
                                }
                            }
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
                trace_stats.mark_terminal_injected();
                if socket.send(Message::Text(payload.clone())).await.is_err() {
                    trace_stats.finish("downstream_closed");
                    persist_codex_client_response_body(
                        &state,
                        stream_log_row_id,
                        client_ws_trace,
                        Some(trace_stats),
                    )
                    .await;
                    return;
                } else {
                    trace_stats.record_client_chunk(&request_started_instant, payload.len());
                }
            }
        } else {
            // 6b. Non-SSE: upstream can still return full Chat JSON. Codex WS only accepts event sequences with `type`,
            //     so do not send a raw `response` object directly (see transforms::chat_completion_non_stream_to_ws_events).
            if let Ok(bytes) = axum::body::to_bytes(body, 8 * 1024 * 1024).await {
                trace_stats.record_upstream_chunk(&request_started_instant, bytes.len());
                if bytes.windows(9).any(|w| w == b"\"choices\"") {
                    match transforms::chat_completion_non_stream_to_ws_events(
                        &bytes,
                        &session_id,
                        &item_id,
                    ) {
                        Ok(frames) => {
                            let mut status_injection = CodexStatusInjection::new(
                                visual.clone(),
                                request_started_instant.elapsed().as_millis() as i64,
                                suppress_status,
                            );
                            for event_str in summary_injection.maybe_append_to_frame_batch(
                                frames,
                                request_started_instant.elapsed().as_millis() as i64,
                            ) {
                                tracing::debug!(
                                    event = %&event_str[..event_str.len().min(200)],
                                    "codex ws non-sse → client event"
                                );
                                append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                                let is_created = codex_frame_is_response_created(&event_str);
                                trace_stats.record_ws_text(&request_started_instant, &event_str);
                                if socket.send(Message::Text(event_str.clone())).await.is_err() {
                                    trace_stats.finish("downstream_closed");
                                    persist_codex_client_response_body(
                                        &state,
                                        stream_log_row_id,
                                        client_ws_trace,
                                        Some(trace_stats),
                                    )
                                    .await;
                                    return;
                                } else {
                                    trace_stats.record_client_chunk(
                                        &request_started_instant,
                                        event_str.len(),
                                    );
                                }
                                if is_created {
                                    let effective_id = extract_response_created_id(&event_str)
                                        .unwrap_or_else(|| session_id.clone());
                                    if let Some(injection) = status_injection.as_mut() {
                                        for injected in injection.next_frames(&effective_id) {
                                            append_codex_ws_client_trace(
                                                &mut client_ws_trace,
                                                &injected,
                                            );
                                            trace_stats.mark_status_injected();
                                            if socket
                                                .send(Message::Text(injected.clone()))
                                                .await
                                                .is_err()
                                            {
                                                trace_stats.finish("downstream_closed");
                                                persist_codex_client_response_body(
                                                    &state,
                                                    stream_log_row_id,
                                                    client_ws_trace,
                                                    Some(trace_stats),
                                                )
                                                .await;
                                                return;
                                            } else {
                                                trace_stats.record_client_chunk(
                                                    &request_started_instant,
                                                    injected.len(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(()) => {
                            let payload = transforms::codex_response_proxy_fault_event(
                                &session_id,
                                "upstream_invalid_chat_completion_json",
                                "upstream returned a non-stream body that is not valid Chat Completions JSON",
                            );
                            trace_stats.mark_terminal_injected();
                            if socket.send(Message::Text(payload.clone())).await.is_err() {
                                trace_stats.finish("downstream_closed");
                                persist_codex_client_response_body(
                                    &state,
                                    stream_log_row_id,
                                    client_ws_trace,
                                    Some(trace_stats),
                                )
                                .await;
                                return;
                            } else {
                                trace_stats
                                    .record_client_chunk(&request_started_instant, payload.len());
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
                    trace_stats.mark_terminal_injected();
                    if socket.send(Message::Text(payload.clone())).await.is_err() {
                        trace_stats.finish("downstream_closed");
                        persist_codex_client_response_body(
                            &state,
                            stream_log_row_id,
                            client_ws_trace,
                            Some(trace_stats),
                        )
                        .await;
                        return;
                    } else {
                        trace_stats.record_client_chunk(&request_started_instant, payload.len());
                    }
                }
            }
        }
        if trace_stats.terminal_seen() {
            trace_stats.finish("completed");
        } else if trace_stats.end_reason().is_none() {
            trace_stats.finish("upstream_eof");
        }
        persist_codex_client_response_body(
            &state,
            stream_log_row_id,
            client_ws_trace,
            Some(trace_stats),
        )
        .await;
        // Loop back to wait for the next response.create on this same connection.
    }
}
