use std::time::Instant;

use vibe_protocol::RequestLog;

#[derive(Clone, Debug)]
pub(crate) struct StreamTraceStats {
    stream_kind: &'static str,
    bridge_mode: &'static str,
    terminal_seen: bool,
    terminal_type: Option<String>,
    end_reason: Option<String>,
    error_detail: Option<String>,
    upstream_first_byte_ms: Option<i64>,
    client_first_write_ms: Option<i64>,
    last_upstream_event_ms: Option<i64>,
    last_client_write_ms: Option<i64>,
    upstream_chunk_count: i64,
    upstream_bytes: i64,
    client_chunk_count: i64,
    client_bytes: i64,
    sse_event_count: i64,
    sse_data_count: i64,
    sse_comment_count: i64,
    sse_keepalive_count: i64,
    sse_done_count: i64,
    parse_error_count: i64,
    first_keepalive_ms: Option<i64>,
    last_keepalive_ms: Option<i64>,
    max_gap_between_upstream_events_ms: Option<i64>,
    max_gap_between_data_events_ms: Option<i64>,
    keepalive_after_last_data_count: i64,
    last_data_event_ms: Option<i64>,
    status_injected: bool,
    terminal_injected: bool,
    last_runtime_sample_ms: Option<i64>,
    last_runtime_output_tokens: i64,
    last_runtime_upstream_bytes: i64,
    last_runtime_client_bytes: i64,
    active_output_tokens_per_sec: Option<f64>,
    active_upstream_bytes_per_sec: f64,
    active_downstream_bytes_per_sec: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RuntimeRates {
    pub(crate) active_output_tokens_per_sec: Option<f64>,
    pub(crate) active_upstream_bytes_per_sec: f64,
    pub(crate) active_downstream_bytes_per_sec: f64,
    pub(crate) active_flow_bytes_per_sec: f64,
}

impl StreamTraceStats {
    pub(crate) fn new(stream_kind: &'static str, bridge_mode: &'static str) -> Self {
        Self {
            stream_kind,
            bridge_mode,
            terminal_seen: false,
            terminal_type: None,
            end_reason: None,
            error_detail: None,
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
            status_injected: false,
            terminal_injected: false,
            last_runtime_sample_ms: None,
            last_runtime_output_tokens: 0,
            last_runtime_upstream_bytes: 0,
            last_runtime_client_bytes: 0,
            active_output_tokens_per_sec: None,
            active_upstream_bytes_per_sec: 0.0,
            active_downstream_bytes_per_sec: 0.0,
        }
    }

    pub(crate) fn record_upstream_chunk(&mut self, started: &Instant, bytes: usize) {
        let now = elapsed_ms(started);
        self.upstream_chunk_count += 1;
        self.upstream_bytes += bytes as i64;
        self.upstream_first_byte_ms.get_or_insert(now);
        self.record_upstream_event_time(now);
    }

    pub(crate) fn record_client_chunk(&mut self, started: &Instant, bytes: usize) {
        let now = elapsed_ms(started);
        self.client_chunk_count += 1;
        self.client_bytes += bytes as i64;
        self.client_first_write_ms.get_or_insert(now);
        self.last_client_write_ms = Some(now);
    }

    pub(crate) fn record_sse_block(&mut self, started: &Instant, block: &str) {
        let now = elapsed_ms(started);
        self.sse_event_count += 1;
        self.record_upstream_event_time(now);

        let mut event_name: Option<&str> = None;
        let mut saw_keepalive = false;
        for raw_line in block.lines() {
            let line = raw_line.trim_end_matches('\r');
            if line.starts_with(':') {
                self.sse_comment_count += 1;
                saw_keepalive = true;
                continue;
            }
            if let Some(name) = line.strip_prefix("event:") {
                let name = name.trim();
                event_name = Some(name);
                if name.eq_ignore_ascii_case("keepalive") {
                    saw_keepalive = true;
                }
                if is_terminal_event_name(name) {
                    self.mark_terminal(name);
                }
                continue;
            }
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            self.sse_data_count += 1;
            let data = payload.trim();
            if data == "[DONE]" {
                self.sse_done_count += 1;
                self.mark_terminal("[DONE]");
                continue;
            }
            if data.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(data) {
                Ok(v) => {
                    if v.get("type")
                        .and_then(|x| x.as_str())
                        .map(|t| t.eq_ignore_ascii_case("keepalive"))
                        .unwrap_or(false)
                    {
                        saw_keepalive = true;
                    }
                    if let Some(term) = terminal_type_from_json(&v) {
                        self.mark_terminal(term);
                    } else if !saw_keepalive {
                        self.record_data_event_time(now);
                    }
                }
                Err(_) => {
                    self.parse_error_count += 1;
                    if !saw_keepalive {
                        self.record_data_event_time(now);
                    }
                }
            }
        }

        if event_name
            .map(|name| name.eq_ignore_ascii_case("keepalive"))
            .unwrap_or(false)
        {
            saw_keepalive = true;
        }
        if saw_keepalive {
            self.record_keepalive(now);
        }
    }

    pub(crate) fn record_ws_text(&mut self, started: &Instant, text: &str) {
        let now = elapsed_ms(started);
        self.record_upstream_event_time(now);
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v) => {
                if let Some(term) = terminal_type_from_json(&v) {
                    self.mark_terminal(term);
                }
            }
            Err(_) => self.parse_error_count += 1,
        }
    }

    pub(crate) fn mark_status_injected(&mut self) {
        self.status_injected = true;
    }

    pub(crate) fn mark_terminal_injected(&mut self) {
        self.terminal_injected = true;
    }

    pub(crate) fn terminal_seen(&self) -> bool {
        self.terminal_seen
    }

    pub(crate) fn upstream_first_byte_ms(&self) -> Option<i64> {
        self.upstream_first_byte_ms
    }

    pub(crate) fn client_first_write_ms(&self) -> Option<i64> {
        self.client_first_write_ms
    }

    pub(crate) fn upstream_bytes(&self) -> i64 {
        self.upstream_bytes
    }

    pub(crate) fn client_bytes(&self) -> i64 {
        self.client_bytes
    }

    pub(crate) fn active_upstream_decode_tps(
        &self,
        output_tokens: i64,
        elapsed_ms: i64,
    ) -> Option<f64> {
        active_tokens_per_sec(output_tokens, self.upstream_first_byte_ms, elapsed_ms)
    }

    pub(crate) fn active_downstream_emit_tps(
        &self,
        output_tokens: i64,
        elapsed_ms: i64,
    ) -> Option<f64> {
        active_tokens_per_sec(output_tokens, self.client_first_write_ms, elapsed_ms)
    }

    pub(crate) fn runtime_rates(&mut self, output_tokens: i64, elapsed_ms: i64) -> RuntimeRates {
        if let Some(prev_ms) = self.last_runtime_sample_ms {
            let delta_ms = elapsed_ms - prev_ms;
            if delta_ms > 0 {
                let denom = delta_ms as f64 / 1000.0;
                let output_delta = output_tokens.saturating_sub(self.last_runtime_output_tokens);
                let upstream_delta = self
                    .upstream_bytes
                    .saturating_sub(self.last_runtime_upstream_bytes);
                let client_delta = self
                    .client_bytes
                    .saturating_sub(self.last_runtime_client_bytes);

                if output_delta > 0 {
                    self.active_output_tokens_per_sec = Some(output_delta as f64 / denom);
                }
                if upstream_delta > 0 {
                    self.active_upstream_bytes_per_sec = upstream_delta as f64 / denom;
                }
                if client_delta > 0 {
                    self.active_downstream_bytes_per_sec = client_delta as f64 / denom;
                }
            }
        }

        self.last_runtime_sample_ms = Some(elapsed_ms);
        self.last_runtime_output_tokens = output_tokens;
        self.last_runtime_upstream_bytes = self.upstream_bytes;
        self.last_runtime_client_bytes = self.client_bytes;

        RuntimeRates {
            active_output_tokens_per_sec: self.active_output_tokens_per_sec,
            active_upstream_bytes_per_sec: self.active_upstream_bytes_per_sec,
            active_downstream_bytes_per_sec: self.active_downstream_bytes_per_sec,
            active_flow_bytes_per_sec: self
                .active_upstream_bytes_per_sec
                .max(self.active_downstream_bytes_per_sec),
        }
    }

    pub(crate) fn end_reason(&self) -> Option<&str> {
        self.end_reason.as_deref()
    }

    pub(crate) fn finish(&mut self, reason: &'static str) {
        if self.end_reason.is_none() {
            self.end_reason = Some(reason.to_string());
        }
    }

    pub(crate) fn finish_error(&mut self, reason: &'static str, detail: impl Into<String>) {
        self.end_reason = Some(reason.to_string());
        self.error_detail = Some(detail.into());
    }

    pub(crate) fn apply_to_log(&self, log: &mut RequestLog) {
        log.stream_kind = Some(self.stream_kind.to_string());
        log.stream_terminal_seen = Some(self.terminal_seen);
        log.stream_end_reason = self.end_reason.clone();
        log.stream_error_detail = self.error_detail.clone();
        log.upstream_first_byte_ms = self.upstream_first_byte_ms;
        log.client_first_write_ms = self.client_first_write_ms;
        log.last_upstream_event_ms = self.last_upstream_event_ms;
        log.last_client_write_ms = self.last_client_write_ms;
        log.upstream_chunk_count = self.upstream_chunk_count;
        log.upstream_bytes = self.upstream_bytes;
        log.client_chunk_count = self.client_chunk_count;
        log.client_bytes = self.client_bytes;
        log.sse_event_count = self.sse_event_count;
        log.sse_data_count = self.sse_data_count;
        log.sse_comment_count = self.sse_comment_count;
        log.sse_keepalive_count = self.sse_keepalive_count;
        log.sse_done_count = self.sse_done_count;
        log.parse_error_count = self.parse_error_count;
        log.first_keepalive_ms = self.first_keepalive_ms;
        log.last_keepalive_ms = self.last_keepalive_ms;
        log.max_gap_between_upstream_events_ms = self.max_gap_between_upstream_events_ms;
        log.max_gap_between_data_events_ms = self.max_gap_between_data_events_ms;
        log.keepalive_after_last_data_count = self.keepalive_after_last_data_count;
        log.last_data_event_ms = self.last_data_event_ms;
        log.bridge_mode = Some(self.bridge_mode.to_string());
        log.status_injected = self.status_injected;
        log.terminal_injected = self.terminal_injected;
        log.upstream_terminal_type = self.terminal_type.clone();
    }

    fn record_upstream_event_time(&mut self, now: i64) {
        if let Some(prev) = self.last_upstream_event_ms {
            update_max_gap(&mut self.max_gap_between_upstream_events_ms, now - prev);
        }
        self.last_upstream_event_ms = Some(now);
    }

    fn record_data_event_time(&mut self, now: i64) {
        if let Some(prev) = self.last_data_event_ms {
            update_max_gap(&mut self.max_gap_between_data_events_ms, now - prev);
        }
        self.last_data_event_ms = Some(now);
    }

    fn record_keepalive(&mut self, now: i64) {
        self.sse_keepalive_count += 1;
        self.first_keepalive_ms.get_or_insert(now);
        self.last_keepalive_ms = Some(now);
        if self.last_data_event_ms.is_some() {
            self.keepalive_after_last_data_count += 1;
        }
    }

    fn mark_terminal(&mut self, terminal_type: &str) {
        self.terminal_seen = true;
        self.terminal_type
            .get_or_insert_with(|| terminal_type.to_string());
    }
}

pub(crate) fn empty_stream_fields(log: &mut RequestLog) {
    log.stream_kind = Some("none".into());
    log.stream_terminal_seen = None;
    log.stream_end_reason = None;
    log.stream_error_detail = None;
    log.upstream_first_byte_ms = None;
    log.client_first_write_ms = None;
    log.last_upstream_event_ms = None;
    log.last_client_write_ms = None;
    log.upstream_chunk_count = 0;
    log.upstream_bytes = 0;
    log.client_chunk_count = 0;
    log.client_bytes = 0;
    log.sse_event_count = 0;
    log.sse_data_count = 0;
    log.sse_comment_count = 0;
    log.sse_keepalive_count = 0;
    log.sse_done_count = 0;
    log.parse_error_count = 0;
    log.first_keepalive_ms = None;
    log.last_keepalive_ms = None;
    log.max_gap_between_upstream_events_ms = None;
    log.max_gap_between_data_events_ms = None;
    log.keepalive_after_last_data_count = 0;
    log.last_data_event_ms = None;
    log.bridge_mode = Some("none".into());
    log.status_injected = false;
    log.terminal_injected = false;
    log.upstream_terminal_type = None;
}

fn elapsed_ms(started: &Instant) -> i64 {
    started.elapsed().as_millis() as i64
}

fn active_tokens_per_sec(tokens: i64, start_ms: Option<i64>, elapsed_ms: i64) -> Option<f64> {
    if tokens <= 0 {
        return None;
    }
    let start_ms = start_ms?;
    let denom_ms = (elapsed_ms - start_ms).max(1);
    Some(tokens as f64 * 1000.0 / denom_ms as f64)
}

fn update_max_gap(slot: &mut Option<i64>, gap: i64) {
    if gap < 0 {
        return;
    }
    match slot {
        Some(current) if *current >= gap => {}
        _ => *slot = Some(gap),
    }
}

fn is_terminal_event_name(name: &str) -> bool {
    matches!(
        name,
        "response.completed" | "response.failed" | "message_stop" | "message.stop"
    )
}

fn terminal_type_from_json(v: &serde_json::Value) -> Option<&'static str> {
    if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
        match t {
            "response.completed" | "response.failed" | "message_stop" | "message.stop" => {
                return Some(match t {
                    "response.completed" => "response.completed",
                    "response.failed" => "response.failed",
                    "message_stop" | "message.stop" => "message_stop",
                    _ => unreachable!(),
                });
            }
            _ => {}
        }
    }
    if v.pointer("/delta/stop_reason").is_some()
        || v.pointer("/usage/output_tokens").is_some()
            && v.get("type").and_then(|x| x.as_str()) == Some("message_delta")
    {
        return Some("message_delta_stop");
    }
    if v.get("choices")
        .and_then(|x| x.as_array())
        .map(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("finish_reason")
                    .map(|x| !x.is_null())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
    {
        return Some("finish_reason");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> RequestLog {
        RequestLog {
            id: "x".into(),
            started_at: 0,
            app: None,
            provider_id: None,
            thread_id: None,
            turn_id: None,
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
        }
    }

    #[test]
    fn classifies_sse_keepalive_comment_done_and_data() {
        let started = Instant::now();
        let mut stats = StreamTraceStats::new("sse", "passthrough");
        stats.record_sse_block(&started, "event: keepalive\ndata: {\"type\":\"keepalive\"}");
        stats.record_sse_block(&started, ":");
        stats.record_sse_block(
            &started,
            "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}",
        );
        stats.record_sse_block(&started, "data: [DONE]");
        let mut log = log();
        stats.finish("completed");
        stats.apply_to_log(&mut log);

        assert_eq!(log.sse_keepalive_count, 2);
        assert_eq!(log.sse_comment_count, 1);
        assert_eq!(log.sse_done_count, 1);
        assert_eq!(log.stream_terminal_seen, Some(true));
        assert_eq!(log.upstream_terminal_type.as_deref(), Some("[DONE]"));
        assert!(log.last_data_event_ms.is_some());
    }
}
