//! Codex completion-summary rendering and client detection.

use crate::config::{
    CodexSummaryClientConfig, CodexSummaryConfig, CodexSummaryLabelOverrides, CodexSummaryStyle,
};
use crate::state::AppState;
use crate::usage::Usage;
use axum::http::HeaderMap;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexClientKind {
    App,
    Cli,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct SummaryMetrics {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_tokens: i64,
    pub latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    /// Estimated USD cost for this turn; `None` when model is unknown.
    pub cost_usd: Option<f64>,
    /// Cumulative USD cost for the current thread (all turns); `None` when unavailable.
    pub thread_cost_usd: Option<f64>,
}

#[derive(Clone)]
pub struct SummaryAccumulator {
    cfg: CodexSummaryConfig,
    client: CodexClientKind,
    state: Option<AppState>,
    turn_id: Option<String>,
    thread_id: Option<String>,
    upstream_model: String,
    usage: Usage,
    /// SUM of input_tokens across all response.completed events this turn (for cost).
    turn_input_sum: i64,
    /// SUM of output_tokens across all response.completed events this turn (for display + cost).
    turn_output_sum: i64,
    first_token_ms: Option<i64>,
    emitted: bool,
    last_text_target: Option<TextTarget>,
    last_text: String,
    pending_finalization: Vec<String>,
}

#[derive(Clone, Debug)]
struct TextTarget {
    response_id: String,
    item_id: String,
    output_index: i64,
    content_index: i64,
}

impl SummaryMetrics {
    pub fn from_usage(usage: Usage, latency_ms: Option<i64>, first_token_ms: Option<i64>) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_tokens: usage.cache_read_tokens + usage.cache_creation_tokens,
            latency_ms,
            first_token_ms,
            cost_usd: None,
            thread_cost_usd: None,
        }
    }

    pub fn from_usage_with_model(
        usage: Usage,
        model: &str,
        latency_ms: Option<i64>,
        first_token_ms: Option<i64>,
    ) -> Self {
        Self {
            cost_usd: usage.cost_usd(model),
            ..Self::from_usage(usage, latency_ms, first_token_ms)
        }
    }

    fn has_usage(self) -> bool {
        self.input_tokens > 0 || self.output_tokens > 0 || self.cache_tokens > 0
    }

    fn speed(self) -> Option<f64> {
        if self.output_tokens <= 0 {
            return None;
        }
        let latency_ms = self.latency_ms?;
        let decode_ms = self
            .first_token_ms
            .filter(|first| latency_ms > *first)
            .map(|first| latency_ms - first)
            .unwrap_or(latency_ms);
        (decode_ms > 0).then(|| self.output_tokens as f64 * 1000.0 / decode_ms as f64)
    }
}

impl SummaryAccumulator {
    pub fn new(cfg: CodexSummaryConfig, client: CodexClientKind) -> Self {
        Self::new_for_turn(cfg, client, None, None, None, String::new())
    }

    pub fn new_for_turn(
        cfg: CodexSummaryConfig,
        client: CodexClientKind,
        state: Option<AppState>,
        turn_id: Option<String>,
        thread_id: Option<String>,
        upstream_model: String,
    ) -> Self {
        Self {
            cfg,
            client,
            state,
            turn_id,
            thread_id,
            upstream_model,
            usage: Usage::default(),
            turn_input_sum: 0,
            turn_output_sum: 0,
            first_token_ms: None,
            emitted: false,
            last_text_target: None,
            last_text: String::new(),
            pending_finalization: Vec::new(),
        }
    }

    pub fn record_first_token_ms(&mut self, ms: i64) {
        if self.first_token_ms.is_none() {
            self.first_token_ms = Some(ms.max(0));
        }
    }

    pub fn record_sse_block_usage(&mut self, block: &str) {
        for raw_line in block.lines() {
            let line = raw_line.trim_end_matches('\r');
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let data = payload.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            apply_usage_from_frame(data, &mut self.usage);
        }
    }

    pub fn maybe_append_to_frame_batch(
        &mut self,
        frames: Vec<String>,
        latency_ms: i64,
    ) -> Vec<String> {
        let mut out = Vec::with_capacity(frames.len() + 1 + self.pending_finalization.len());
        for frame_json in frames {
            self.record_frame_state(&frame_json, latency_ms);

            if self.emitted {
                self.flush_pending_finalization(&mut out);
                out.push(frame_json);
                continue;
            }

            if is_message_finalization_frame(&frame_json) {
                self.pending_finalization.push(frame_json);
                continue;
            }

            if !response_completed_is_end_turn(&frame_json) {
                self.flush_pending_finalization(&mut out);
                out.push(frame_json);
                continue;
            }

            let mut metrics = self.build_metrics(latency_ms);
            let Some(text) = render_summary(&self.cfg, self.client, metrics) else {
                self.flush_pending_finalization(&mut out);
                out.push(frame_json);
                continue;
            };
            // Pre-flight: only consume the per-turn summary slot if this
            // response actually carries a message we can attach the summary
            // to. A tool-call-only request has no assistant message in its
            // pending finalization or in `response.completed.response.output`,
            // so the append helpers would no-op anyway. Reserving the slot
            // here would silently lock out the follow-up request that DOES
            // carry the final assistant text — which is the "end slot
            // disappears on multi-turn / tool-using turn" bug.
            if !self.has_message_summary_target(&frame_json) {
                self.flush_pending_finalization(&mut out);
                out.push(frame_json);
                continue;
            }
            if let Some(state) = &self.state {
                if !reserve_summary_slot(state, self.turn_id.as_deref(), self.client) {
                    self.emitted = true;
                    self.flush_pending_finalization(&mut out);
                    out.push(frame_json);
                    continue;
                }
            }
            self.emitted = true;
            // Persist turn cost and get cumulative thread cost (after slot reserved).
            if let Some(turn_cost) = metrics.cost_usd {
                metrics.thread_cost_usd = self.persist_thread_cost(turn_cost);
            }
            // Re-render with thread cost included.
            let text = render_summary(&self.cfg, self.client, metrics).unwrap_or(text);

            let append_text = format_summary_append(&self.last_text, &text);
            if let Some(delta) = self.summary_delta_frame(&append_text) {
                out.push(delta);
            }

            for candidate in self.pending_finalization.iter_mut() {
                append_summary_to_text_done_frame(candidate, &text);
                append_summary_to_content_part_done_frame(candidate, &text);
                append_summary_to_output_item_done_frame(candidate, &text);
            }
            self.flush_pending_finalization(&mut out);

            if let Some(appended_completed) = append_summary_to_completed_frame(&frame_json, &text)
            {
                out.push(appended_completed);
            } else {
                out.push(frame_json);
            }
        }
        out
    }

    fn flush_pending_finalization(&mut self, out: &mut Vec<String>) {
        out.append(&mut self.pending_finalization);
    }

    /// True if this response carries an assistant message the summary text
    /// can be appended to (either via a buffered finalization frame or via
    /// the `response.output[]` array on the terminal frame). Used to avoid
    /// burning the per-turn dedupe slot on a tool-call-only response that
    /// has nothing to attach the summary to anyway.
    fn has_message_summary_target(&self, completed_frame: &str) -> bool {
        // output_text.done and content_part.done are only ever emitted for
        // assistant text streams — never for function_call output items.
        // Their presence in pending_finalization is a sufficient signal.
        let has_text_finalization = self.pending_finalization.iter().any(|f| {
            let Ok(v) = serde_json::from_str::<Value>(f) else {
                return false;
            };
            matches!(
                v.get("type").and_then(Value::as_str),
                Some("response.output_text.done") | Some("response.content_part.done")
            )
        });
        if has_text_finalization {
            return true;
        }
        // output_item.done can be either a message item or a function_call
        // item — only the message variant accepts a summary append.
        let has_message_item_done = self.pending_finalization.iter().any(|f| {
            let Ok(v) = serde_json::from_str::<Value>(f) else {
                return false;
            };
            v.get("type").and_then(Value::as_str) == Some("response.output_item.done")
                && v.pointer("/item/type").and_then(Value::as_str) == Some("message")
        });
        if has_message_item_done {
            return true;
        }
        // Fallback: a non-streaming response.completed may carry the full
        // message item inside `response.output[]`. `append_summary_to_
        // completed_frame` injects there as a last resort.
        let Ok(v) = serde_json::from_str::<Value>(completed_frame) else {
            return false;
        };
        v.pointer("/response/output")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("message"))
            })
            .unwrap_or(false)
    }

    fn record_frame_state(&mut self, frame_json: &str, latency_ms: i64) {
        if frame_has_output_text(frame_json) {
            self.record_first_token_ms(latency_ms);
        }
        apply_usage_from_frame(frame_json, &mut self.usage);
        self.accumulate_request_sums(frame_json);
        self.capture_text_frame(frame_json);
    }

    /// Accumulate per-request input/output from `response.completed` for cost + display SUM.
    /// Also persists to AppState keyed by turn_id so that ALL requests in a turn
    /// (including tool-call iterations handled by separate accumulators) are counted.
    fn accumulate_request_sums(&mut self, frame_json: &str) {
        let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
            return;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("response.completed") {
            return;
        }
        let Some(u) = v.pointer("/response/usage").or_else(|| v.get("usage")) else {
            return;
        };
        let input = u
            .get("input_tokens")
            .or_else(|| u.get("prompt_tokens"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        let output = u
            .get("output_tokens")
            .or_else(|| u.get("completion_tokens"))
            .and_then(|x| x.as_i64())
            .unwrap_or(0);
        if input == 0 && output == 0 {
            return;
        }
        self.turn_input_sum += input;
        self.turn_output_sum += output;
        // Persist to shared AppState so the final-request accumulator can read
        // the running total even for requests it never saw.
        if let (Some(state), Some(turn_id)) = (&self.state, &self.turn_id) {
            state.accumulate_codex_turn_io(turn_id, input, output);
        }
    }

    /// Build SummaryMetrics (no side effects — call before slot reserve).
    /// Uses AppState-accumulated turn IO when available so that all tool-call
    /// requests in the same turn contribute to the cost, not just the final one.
    fn build_metrics(&self, latency_ms: i64) -> SummaryMetrics {
        // Read aggregated (input_sum, output_sum) across ALL requests this turn.
        let (agg_input, agg_output) = match (&self.state, &self.turn_id) {
            (Some(state), Some(turn_id)) => state.get_codex_turn_io(turn_id),
            _ => (0, 0),
        };
        // Use AppState aggregates if available; fall back to local sums.
        let cost_input = agg_input
            .max(self.turn_input_sum)
            .max(self.usage.input_tokens);
        let cost_output = agg_output
            .max(self.turn_output_sum)
            .max(self.usage.output_tokens);
        // For display: `in` = context window size (MAX of input = last request's value),
        // `out` = total generation across tool-call loop (SUM = agg_output).
        let display_output = agg_output
            .max(self.turn_output_sum)
            .max(self.usage.output_tokens);
        let cost_usage = Usage {
            input_tokens: cost_input,
            output_tokens: cost_output,
            ..Usage::default()
        };
        SummaryMetrics {
            input_tokens: self.usage.input_tokens,
            output_tokens: display_output,
            cache_tokens: self.usage.cache_read_tokens + self.usage.cache_creation_tokens,
            latency_ms: Some(latency_ms.max(0)),
            first_token_ms: self.first_token_ms,
            cost_usd: cost_usage.cost_usd(&self.upstream_model),
            thread_cost_usd: None,
        }
    }

    /// Persist turn cost and return cumulative thread cost (call after slot reserve).
    fn persist_thread_cost(&self, turn_cost: f64) -> Option<f64> {
        let state = self.state.as_ref()?;
        let thread_id = self.thread_id.as_deref()?;
        Some(state.add_codex_thread_cost(thread_id, turn_cost))
    }

    fn capture_text_frame(&mut self, frame_json: &str) {
        let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
            return;
        };
        match v.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                let Some(delta) = v.get("delta").and_then(Value::as_str) else {
                    return;
                };
                if delta.is_empty() {
                    return;
                }
                if let Some(target) = text_target_from_event(&v) {
                    self.last_text_target = Some(target);
                }
                self.last_text.push_str(delta);
            }
            Some("response.output_text.done") => {
                if let Some(text) = v.get("text").and_then(Value::as_str) {
                    self.last_text = text.to_string();
                }
                if let Some(target) = text_target_from_event(&v) {
                    self.last_text_target = Some(target);
                }
            }
            Some("response.output_item.done") => {
                let Some(item) = v.get("item") else {
                    return;
                };
                if item.get("type").and_then(Value::as_str) != Some("message") {
                    return;
                }
                if let Some(text) = item
                    .get("content")
                    .and_then(Value::as_array)
                    .and_then(|parts| {
                        parts.iter().rev().find(|part| {
                            part.get("type").and_then(Value::as_str) == Some("output_text")
                        })
                    })
                    .and_then(|part| part.get("text"))
                    .and_then(Value::as_str)
                {
                    self.last_text = text.to_string();
                    self.last_text_target = Some(TextTarget {
                        response_id: v
                            .get("response_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        item_id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        output_index: v.get("output_index").and_then(Value::as_i64).unwrap_or(0),
                        content_index: 0,
                    });
                }
            }
            _ => {}
        }
    }

    fn summary_delta_frame(&self, append_text: &str) -> Option<String> {
        let target = self.last_text_target.as_ref()?;
        if target.response_id.is_empty() || target.item_id.is_empty() {
            return None;
        }
        serde_json::to_string(&serde_json::json!({
            "type": "response.output_text.delta",
            "response_id": target.response_id,
            "item_id": target.item_id,
            "output_index": target.output_index,
            "content_index": target.content_index,
            "delta": append_text,
        }))
        .ok()
    }

    /// Apply summary buffering to a Passthrough SSE block.
    ///
    /// The block looks like `event: TYPE\ndata: JSON` (one logical SSE event).
    /// `maybe_append_to_frame_batch` may buffer the data frame (returning zero
    /// frames) or expand it (returning multiple frames when previously buffered
    /// finalizations flush). Each returned frame must become a fully-formed SSE
    /// block — `event: TYPE\ndata: JSON` — derived from the frame's own JSON
    /// `type`, otherwise the stream ends up with orphan `event:` lines or
    /// `data:` lines stacked under the wrong `event:`, which makes codex-rs
    /// reject the stream with "stream closed before response.completed".
    pub fn maybe_append_to_sse_block(&mut self, block: &str, latency_ms: i64) -> Option<String> {
        let mut data_payload: Option<String> = None;
        let mut other_lines: Vec<String> = Vec::new();
        for raw_line in block.lines() {
            let line = raw_line.trim_end_matches('\r');
            if line.starts_with("event:") {
                // Drop upstream event: lines; we will reconstruct them from
                // each output frame's JSON type to keep event/data in sync.
                continue;
            }
            if let Some(payload) = line.strip_prefix("data:") {
                let trimmed = payload.trim();
                if trimmed.is_empty() || trimmed == "[DONE]" {
                    other_lines.push(raw_line.to_string());
                } else if data_payload.is_none() {
                    data_payload = Some(trimmed.to_string());
                } else {
                    other_lines.push(raw_line.to_string());
                }
                continue;
            }
            other_lines.push(raw_line.to_string());
        }

        let Some(data) = data_payload else {
            return None;
        };

        let frames = self.maybe_append_to_frame_batch(vec![data.clone()], latency_ms);
        if frames.len() == 1 && frames[0] == data {
            return None;
        }

        let mut blocks: Vec<String> = Vec::with_capacity(frames.len().max(1) + other_lines.len());
        if !other_lines.is_empty() {
            blocks.push(other_lines.join("\n"));
        }
        for frame in frames {
            let event_type = serde_json::from_str::<Value>(&frame)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string));
            let block = match event_type {
                Some(t) => format!("event: {t}\ndata: {frame}"),
                None => format!("data: {frame}"),
            };
            blocks.push(block);
        }
        // Frames may have been buffered (empty output): emit an empty string so
        // the caller's `\n\n` terminator becomes a harmless blank event rather
        // than re-forwarding the original block we meant to suppress.
        Some(blocks.join("\n\n"))
    }

    pub fn maybe_append_to_frame(&mut self, frame_json: &str, latency_ms: i64) -> Option<String> {
        self.record_frame_state(frame_json, latency_ms);
        if self.emitted || !response_completed_is_end_turn(frame_json) {
            return None;
        }
        let mut metrics = self.build_metrics(latency_ms);
        let _ = render_summary(&self.cfg, self.client, metrics)?;
        if let Some(state) = &self.state {
            if !reserve_summary_slot(state, self.turn_id.as_deref(), self.client) {
                self.emitted = true;
                return None;
            }
        }
        self.emitted = true;
        if let Some(turn_cost) = metrics.cost_usd {
            metrics.thread_cost_usd = self.persist_thread_cost(turn_cost);
        }
        let text = render_summary(&self.cfg, self.client, metrics)?;
        append_summary_to_completed_frame(frame_json, &text)
    }
}

pub fn turn_id_from_request(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    turn_id_from_value(&v)
}

pub fn turn_id_from_value(v: &Value) -> Option<String> {
    for pointer in [
        "/client_metadata/x-codex-turn-metadata",
        "/response/client_metadata/x-codex-turn-metadata",
        "/x-codex-turn-metadata",
        "/turn_metadata",
        "/client_metadata",
        "/response/client_metadata",
    ] {
        if let Some(turn_id) = v.pointer(pointer).and_then(turn_id_from_metadata_value) {
            return Some(turn_id);
        }
    }
    v.pointer("/client_metadata/turn_id")
        .or_else(|| v.pointer("/response/client_metadata/turn_id"))
        .or_else(|| v.pointer("/turn_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn turn_id_from_metadata_value(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .as_ref()
            .and_then(turn_id_from_metadata_value)
            .or_else(|| {
                let trimmed = s.trim();
                (!trimmed.is_empty() && !trimmed.starts_with('{')).then(|| trimmed.to_owned())
            }),
        Value::Object(_) => v.get("turn_id").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
}

pub fn thread_id_from_request(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    thread_id_from_value(&v)
}

/// Whether this request belongs to the user-facing main conversation or to a
/// background subagent thread, as declared by `x-codex-turn-metadata
/// .thread_source`. See CLAUDE.md → Codex Protocol → Thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexThreadSource {
    User,
    Subagent,
    /// Field present but value isn't recognised; treat conservatively.
    Other,
}

pub fn thread_source_from_request(body: &[u8]) -> Option<CodexThreadSource> {
    let v: Value = serde_json::from_slice(body).ok()?;
    thread_source_from_value(&v)
}

pub fn thread_source_from_value(v: &Value) -> Option<CodexThreadSource> {
    for pointer in [
        "/client_metadata/x-codex-turn-metadata",
        "/response/client_metadata/x-codex-turn-metadata",
        "/x-codex-turn-metadata",
        "/turn_metadata",
        "/client_metadata",
        "/response/client_metadata",
    ] {
        if let Some(src) = v
            .pointer(pointer)
            .and_then(thread_source_from_metadata_value_inner)
        {
            return Some(src);
        }
    }
    None
}

fn thread_source_from_metadata_value_inner(v: &Value) -> Option<CodexThreadSource> {
    match v {
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .as_ref()
            .and_then(thread_source_from_metadata_value_inner),
        Value::Object(_) => v
            .get("thread_source")
            .and_then(Value::as_str)
            .map(|s| match s {
                "user" => CodexThreadSource::User,
                "subagent" => CodexThreadSource::Subagent,
                _ => CodexThreadSource::Other,
            }),
        _ => None,
    }
}

pub fn thread_id_from_value(v: &Value) -> Option<String> {
    for pointer in [
        "/client_metadata/x-codex-turn-metadata",
        "/response/client_metadata/x-codex-turn-metadata",
        "/x-codex-turn-metadata",
        "/turn_metadata",
        "/client_metadata",
        "/response/client_metadata",
    ] {
        if let Some(thread_id) = v
            .pointer(pointer)
            .and_then(thread_id_from_metadata_value_inner)
        {
            return Some(thread_id);
        }
    }
    v.pointer("/client_metadata/thread_id")
        .or_else(|| v.pointer("/response/client_metadata/thread_id"))
        .or_else(|| v.pointer("/thread_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn thread_id_from_metadata_value_inner(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .as_ref()
            .and_then(thread_id_from_metadata_value_inner),
        Value::Object(_) => v
            .get("thread_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

/// Extract `turn_id` from the `x-codex-turn-metadata` HTTP header (JSON object).
pub fn turn_id_from_headers(headers: &HeaderMap) -> Option<String> {
    let s = headers.get("x-codex-turn-metadata")?.to_str().ok()?;
    serde_json::from_str::<Value>(s)
        .ok()?
        .get("turn_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Extract `thread_id` from HTTP headers — first tries the direct `thread-id` /
/// `thread_id` header, then falls back to the `x-codex-turn-metadata` JSON object.
pub fn thread_id_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(id) = headers
        .get("thread-id")
        .or_else(|| headers.get("thread_id"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    {
        return Some(id);
    }
    let s = headers.get("x-codex-turn-metadata")?.to_str().ok()?;
    serde_json::from_str::<Value>(s)
        .ok()?
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Extract `thread_source` from the `x-codex-turn-metadata` HTTP header.
pub fn thread_source_from_headers(headers: &HeaderMap) -> Option<CodexThreadSource> {
    let s = headers.get("x-codex-turn-metadata")?.to_str().ok()?;
    let v: Value = serde_json::from_str(s).ok()?;
    thread_source_from_metadata_value_inner(&v)
}

pub fn summary_slot_key(turn_id: Option<&str>, client: CodexClientKind) -> String {
    format!(
        "{}|{}",
        turn_id.unwrap_or("__unknown_turn__"),
        client.as_str()
    )
}

fn summary_slot_ttl(turn_id: Option<&str>) -> Duration {
    if turn_id.is_some() {
        Duration::from_secs(30 * 60)
    } else {
        Duration::from_secs(90)
    }
}

pub fn reserve_summary_slot(
    state: &AppState,
    turn_id: Option<&str>,
    client: CodexClientKind,
) -> bool {
    state.remember_codex_summary_key(summary_slot_key(turn_id, client), summary_slot_ttl(turn_id))
}

impl CodexClientKind {
    fn as_str(self) -> &'static str {
        match self {
            CodexClientKind::App => "app",
            CodexClientKind::Cli => "cli",
            CodexClientKind::Unknown => "unknown",
        }
    }
}

pub fn detect_client(headers: &HeaderMap) -> CodexClientKind {
    let originator = header_value(headers, "originator");
    let ua = header_value(headers, "user-agent");
    let originator_l = originator.to_ascii_lowercase();
    let ua_l = ua.to_ascii_lowercase();

    if originator.eq_ignore_ascii_case("Codex Desktop") || ua.contains("Codex Desktop/") {
        CodexClientKind::App
    } else if originator == "codex_cli_rs"
        || ua.starts_with("codex_cli_rs/")
        || ua_l == "codex-cli"
        || ua_l.contains("codex-cli")
        || originator_l == "codex_cli_rs"
    {
        CodexClientKind::Cli
    } else {
        CodexClientKind::Unknown
    }
}

pub fn render_summary(
    cfg: &CodexSummaryConfig,
    client: CodexClientKind,
    metrics: SummaryMetrics,
) -> Option<String> {
    if !cfg.enabled || !metrics.has_usage() {
        return None;
    }
    let client_cfg = client_config(cfg, client);
    if !client_cfg.enabled {
        return None;
    }
    let labels = metric_labels(cfg, metrics);
    if labels.is_empty() {
        return None;
    }

    let separator = cfg.separator.as_str();
    let prefix = client_cfg.prefix.as_deref().unwrap_or("↯ ");
    let suffix = client_cfg.suffix.as_deref().unwrap_or("");
    let rendered = match client_cfg.style {
        CodexSummaryStyle::FormulaCompact => formula_compact(&labels),
        CodexSummaryStyle::PlainCompact => plain_compact(&labels, separator, prefix),
        CodexSummaryStyle::InlineChips => inline_chips(&labels, separator, prefix),
        CodexSummaryStyle::StatusBar => status_bar(&labels, separator),
        CodexSummaryStyle::EnglishLight => english_light(&labels, separator),
        CodexSummaryStyle::ChineseLight => chinese_light(&labels, separator),
        CodexSummaryStyle::FormulaLabeled => formula_labeled(&labels),
        CodexSummaryStyle::AsciiPlain => ascii_plain(&labels, separator),
    };
    Some(format!("{rendered}{suffix}"))
}

pub fn append_summary_to_completed_frame(frame_json: &str, text: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(frame_json).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("response.completed") {
        return None;
    }
    append_summary_to_response_value(v.get_mut("response")?, text)?;
    serde_json::to_string(&v).ok()
}

pub fn append_summary_to_response_value(response: &mut Value, text: &str) -> Option<()> {
    let output = response.get_mut("output")?.as_array_mut()?;
    let Some(message) = output
        .iter_mut()
        .rev()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("message"))
    else {
        return None;
    };
    append_summary_to_message_item(message, text)
}

fn is_message_finalization_frame(frame_json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
        return false;
    };
    matches!(
        v.get("type").and_then(Value::as_str),
        Some(
            "response.output_text.done"
                | "response.content_part.done"
                | "response.output_item.done"
        )
    )
}

fn append_summary_to_text_done_frame(frame_json: &mut String, text: &str) -> bool {
    let Ok(mut v) = serde_json::from_str::<Value>(frame_json) else {
        return false;
    };
    if v.get("type").and_then(Value::as_str) != Some("response.output_text.done") {
        return false;
    }
    let Some(existing) = v.get("text").and_then(Value::as_str).map(str::to_owned) else {
        return false;
    };
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "text".into(),
            Value::String(format!(
                "{existing}{}",
                format_summary_append(&existing, text)
            )),
        );
    }
    let Ok(updated) = serde_json::to_string(&v) else {
        return false;
    };
    *frame_json = updated;
    true
}

fn append_summary_to_content_part_done_frame(frame_json: &mut String, text: &str) -> bool {
    let Ok(mut v) = serde_json::from_str::<Value>(frame_json) else {
        return false;
    };
    if v.get("type").and_then(Value::as_str) != Some("response.content_part.done") {
        return false;
    }
    let Some(part) = v.get_mut("part") else {
        return false;
    };
    if part.get("type").and_then(Value::as_str) != Some("output_text") {
        return false;
    }
    if append_summary_to_output_text_part(part, text).is_none() {
        return false;
    }
    let Ok(updated) = serde_json::to_string(&v) else {
        return false;
    };
    *frame_json = updated;
    true
}

fn text_target_from_event(v: &Value) -> Option<TextTarget> {
    Some(TextTarget {
        response_id: v.get("response_id")?.as_str()?.to_string(),
        item_id: v.get("item_id")?.as_str()?.to_string(),
        output_index: v.get("output_index").and_then(Value::as_i64).unwrap_or(0),
        content_index: v.get("content_index").and_then(Value::as_i64).unwrap_or(0),
    })
}

fn format_summary_append(existing_text: &str, summary_text: &str) -> String {
    let separator = if existing_text.trim().is_empty() {
        ""
    } else {
        "\n\n"
    };
    format!("{separator}{summary_text}")
}

fn append_summary_to_output_item_done_frame(frame_json: &mut String, text: &str) -> bool {
    let Ok(mut v) = serde_json::from_str::<Value>(frame_json) else {
        return false;
    };
    if v.get("type").and_then(Value::as_str) != Some("response.output_item.done") {
        return false;
    }
    let Some(item) = v.get_mut("item") else {
        return false;
    };
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return false;
    }
    if append_summary_to_message_item(item, text).is_none() {
        return false;
    }
    let Ok(updated) = serde_json::to_string(&v) else {
        return false;
    };
    *frame_json = updated;
    true
}

fn append_summary_to_message_item(message: &mut Value, text: &str) -> Option<()> {
    let content = message.get_mut("content")?.as_array_mut()?;
    let Some(part) = content
        .iter_mut()
        .rev()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
    else {
        return None;
    };
    append_summary_to_output_text_part(part, text)
}

fn append_summary_to_output_text_part(part: &mut Value, text: &str) -> Option<()> {
    let text_value = part.get_mut("text")?.as_str()?.to_owned();
    let append = format_summary_append(&text_value, text);
    *part.get_mut("text")? = Value::String(format!("{text_value}{append}"));
    Some(())
}

pub fn response_completed_is_end_turn(frame_json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
        return false;
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("response.completed") {
        return false;
    }
    !matches!(v.pointer("/response/end_turn"), Some(Value::Bool(false)))
}

pub fn response_id_from_frame(frame_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(frame_json).ok()?;
    v.pointer("/response/id")
        .or_else(|| v.get("response_id"))
        .or_else(|| v.get("id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
}

fn frame_has_output_text(frame_json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
        return false;
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some("response.output_text.delta") => v
            .get("delta")
            .and_then(|delta| delta.as_str())
            .map(|delta| !delta.is_empty())
            .unwrap_or(false),
        Some("response.output_text.done") => v
            .get("text")
            .and_then(|text| text.as_str())
            .map(|text| !text.is_empty())
            .unwrap_or(false),
        Some("response.output_item.done") | Some("response.output_item.added") => v
            .pointer("/item/content")
            .and_then(|content| content.as_array())
            .map(|parts| {
                parts.iter().any(|part| {
                    matches!(
                        part.get("type").and_then(|t| t.as_str()),
                        Some("output_text" | "text")
                    ) && part
                        .get("text")
                        .and_then(|text| text.as_str())
                        .map(|text| !text.is_empty())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false),
        _ => false,
    }
}

pub fn apply_usage_from_frame(frame_json: &str, usage: &mut Usage) {
    let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
        return;
    };
    let Some(u) = v.pointer("/response/usage").or_else(|| v.get("usage")) else {
        return;
    };
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
    if let Some(n) = u
        .get("cache_creation_input_tokens")
        .or_else(|| u.get("cache_creation_tokens"))
        .and_then(|x| x.as_i64())
    {
        usage.cache_creation_tokens = n;
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn client_config(cfg: &CodexSummaryConfig, client: CodexClientKind) -> &CodexSummaryClientConfig {
    match client {
        CodexClientKind::App => &cfg.clients.app,
        CodexClientKind::Cli => &cfg.clients.cli,
        CodexClientKind::Unknown => &cfg.clients.unknown,
    }
}

fn metric_labels(cfg: &CodexSummaryConfig, metrics: SummaryMetrics) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if cfg.show_speed {
        if let Some(speed) = metrics.speed() {
            out.push((
                metric_label(&cfg.label_overrides, "speed"),
                format_speed(speed, cfg.speed_decimal_places),
            ));
        }
    }
    if cfg.show_input && metrics.input_tokens > 0 {
        out.push((
            metric_label(&cfg.label_overrides, "input"),
            format_token_count(metrics.input_tokens),
        ));
    }
    if cfg.show_output && metrics.output_tokens > 0 {
        out.push((
            metric_label(&cfg.label_overrides, "output"),
            format_token_count(metrics.output_tokens),
        ));
    }
    if cfg.show_cache && metrics.cache_tokens > 0 {
        out.push((
            metric_label(&cfg.label_overrides, "cache"),
            format_token_count(metrics.cache_tokens),
        ));
    }
    if cfg.show_latency {
        if let Some(ms) = metrics.latency_ms.filter(|ms| *ms > 0) {
            out.push((
                metric_label(&cfg.label_overrides, "latency"),
                format_latency(ms),
            ));
        }
    }
    if cfg.show_first_token {
        if let Some(ms) = metrics.first_token_ms.filter(|ms| *ms > 0) {
            out.push((
                metric_label(&cfg.label_overrides, "first_token"),
                format_latency(ms),
            ));
        }
    }
    if cfg.show_cost {
        if let Some(cost) = metrics.cost_usd.filter(|c| *c > 0.0) {
            out.push((
                metric_label(&cfg.label_overrides, "cost"),
                format_cost(cost),
            ));
        }
    }
    if cfg.show_thread_cost {
        if let Some(thread_cost) = metrics.thread_cost_usd.filter(|c| *c > 0.0) {
            out.push((
                metric_label(&cfg.label_overrides, "thread_cost"),
                format_cost(thread_cost),
            ));
        }
    }
    out
}

fn format_speed(speed: f64, decimals: u8) -> String {
    let decimals = decimals.min(3) as usize;
    format!("{speed:.decimals$}/s")
}

fn format_token_count(n: i64) -> String {
    let n = n.max(0) as f64;
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}

fn format_latency(ms: i64) -> String {
    if ms >= 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

fn formula_escape(s: &str) -> String {
    s.replace('\\', "\\backslash ")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('_', "\\_")
        .replace('%', "\\%")
        .replace('&', "\\&")
        .replace('#', "\\#")
        .replace('$', "\\$")
}

fn metric_label(overrides: &CodexSummaryLabelOverrides, key: &str) -> String {
    match key {
        "speed" => overrides
            .speed
            .clone()
            .unwrap_or_else(|| "speed".to_string()),
        "input" => overrides.input.clone().unwrap_or_else(|| "in".to_string()),
        "output" => overrides
            .output
            .clone()
            .unwrap_or_else(|| "out".to_string()),
        "cache" => overrides
            .cache
            .clone()
            .unwrap_or_else(|| "cache".to_string()),
        "latency" => overrides
            .latency
            .clone()
            .unwrap_or_else(|| "lat".to_string()),
        "first_token" => overrides
            .first_token
            .clone()
            .unwrap_or_else(|| "first".to_string()),
        "cost" => overrides.cost.clone().unwrap_or_else(|| "~usd".to_string()),
        "thread_cost" => overrides
            .thread_cost
            .clone()
            .unwrap_or_else(|| "Σusd".to_string()),
        _ => key.to_string(),
    }
}

fn format_cost(usd: f64) -> String {
    if usd < 0.000_01 {
        format!("<$0.00001")
    } else if usd < 0.001 {
        format!("${usd:.5}")
    } else if usd < 1.0 {
        format!("${usd:.4}")
    } else {
        format!("${usd:.2}")
    }
}

fn formula_compact(labels: &[(String, String)]) -> String {
    let parts = labels
        .iter()
        .map(|(k, v)| format!("\\textsf{{{}}}=\\textsf{{{}}}", k, formula_escape(v)))
        .collect::<Vec<_>>()
        .join("\\;\\cdot\\;");
    format!("$$\n\\scriptsize\n\\color{{#64748b}}{{\\textsf{{Vibe+}}\\,\\mid\\,{parts}}}\n$$")
}

fn formula_labeled(labels: &[(String, String)]) -> String {
    let parts = labels
        .iter()
        .map(|(k, v)| format!("\\mathrm{{{}}}=\\textsf{{{}}}", k, formula_escape(v)))
        .collect::<Vec<_>>()
        .join("\\quad");
    format!("$$\n\\small\n\\color{{#64748b}}{{{parts}}}\n$$")
}

fn plain_compact(labels: &[(String, String)], separator: &str, prefix: &str) -> String {
    format!(
        "{prefix}{}",
        labels
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(separator)
    )
}

fn inline_chips(labels: &[(String, String)], separator: &str, prefix: &str) -> String {
    format!(
        "_{prefix}{}_",
        labels
            .iter()
            .map(|(k, v)| format!("{k} `{v}`"))
            .collect::<Vec<_>>()
            .join(separator)
    )
}

fn status_bar(labels: &[(String, String)], separator: &str) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("`{k} {v}`"))
        .collect::<Vec<_>>()
        .join(separator)
}

fn english_light(labels: &[(String, String)], separator: &str) -> String {
    format!(
        "_{}_",
        labels
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(separator)
    )
}

fn chinese_light(labels: &[(String, String)], separator: &str) -> String {
    let parts = labels
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join(separator);
    format!("_This turn: {parts}_")
}

fn ascii_plain(labels: &[(String, String)], separator: &str) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join(separator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CodexSummaryClientsConfig, CodexSummaryConfig};
    use axum::http::HeaderValue;

    fn metrics() -> SummaryMetrics {
        SummaryMetrics {
            input_tokens: 42_100,
            output_tokens: 1_900,
            cache_tokens: 18_400,
            latency_ms: Some(60_000),
            first_token_ms: Some(250),
            cost_usd: None,
            thread_cost_usd: None,
        }
    }

    #[test]
    fn detects_codex_desktop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));
        headers.insert(
            "user-agent",
            HeaderValue::from_static("Codex Desktop/0.130.0-alpha.5"),
        );
        assert_eq!(detect_client(&headers), CodexClientKind::App);
    }

    #[test]
    fn detects_codex_cli_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        headers.insert("user-agent", HeaderValue::from_static("codex_cli_rs/0.1"));
        assert_eq!(detect_client(&headers), CodexClientKind::Cli);
    }

    #[test]
    fn app_default_renders_formula() {
        let cfg = CodexSummaryConfig::default();
        let text = render_summary(&cfg, CodexClientKind::App, metrics()).unwrap();
        assert!(text.contains("\\scriptsize"));
        assert!(text.contains("\\textsf{Vibe+}"));
        assert!(text.contains("31.8/s"));
    }

    #[test]
    fn cli_default_renders_plain() {
        let cfg = CodexSummaryConfig::default();
        let text = render_summary(&cfg, CodexClientKind::Cli, metrics()).unwrap();
        assert!(text.starts_with("↯ "));
        assert!(text.contains("out 1.9k"));
        assert!(!text.contains("$$"));
    }

    #[test]
    fn unknown_default_is_disabled() {
        let cfg = CodexSummaryConfig::default();
        assert!(render_summary(&cfg, CodexClientKind::Unknown, metrics()).is_none());
    }

    #[test]
    fn no_usage_skips_summary() {
        let cfg = CodexSummaryConfig {
            clients: CodexSummaryClientsConfig::default(),
            ..CodexSummaryConfig::default()
        };
        let metrics = SummaryMetrics {
            input_tokens: 0,
            output_tokens: 0,
            cache_tokens: 0,
            latency_ms: Some(10),
            first_token_ms: None,
            cost_usd: None,
            thread_cost_usd: None,
        };
        assert!(render_summary(&cfg, CodexClientKind::App, metrics).is_none());
    }

    #[test]
    fn skips_tool_loop_completion() {
        assert!(!response_completed_is_end_turn(
            r#"{"type":"response.completed","response":{"end_turn":false}}"#
        ));
        assert!(response_completed_is_end_turn(
            r#"{"type":"response.completed","response":{"id":"r"}}"#
        ));
    }

    #[test]
    fn extracts_turn_and_thread_ids_from_nested_metadata_variants() {
        let v = serde_json::json!({
            "response": {
                "client_metadata": {
                    "x-codex-turn-metadata": "{\"turn_id\":\"turn-nested\",\"thread_id\":\"thread-nested\"}"
                }
            }
        });

        assert_eq!(turn_id_from_value(&v).as_deref(), Some("turn-nested"));
        assert_eq!(thread_id_from_value(&v).as_deref(), Some("thread-nested"));

        let direct = serde_json::json!({
            "client_metadata": { "turn_id": "turn-direct", "thread_id": "thread-direct" }
        });
        assert_eq!(turn_id_from_value(&direct).as_deref(), Some("turn-direct"));
        assert_eq!(
            thread_id_from_value(&direct).as_deref(),
            Some("thread-direct")
        );
    }

    #[test]
    fn append_summary_targets_last_message_output_text_without_touching_tool_items() {
        let mut response = serde_json::json!({
            "id": "resp-1",
            "output": [
                {
                    "id": "msg-1",
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type":"output_text","text":"first"}]
                },
                {"type":"function_call","name":"shell","arguments":"{}","call_id":"call-1"},
                {
                    "id": "msg-2",
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type":"reasoning_text","text":"hidden"},
                        {"type":"output_text","text":"second"}
                    ]
                }
            ]
        });

        append_summary_to_response_value(&mut response, "summary").expect("append summary");

        assert_eq!(response["output"][0]["content"][0]["text"], "first");
        assert_eq!(response["output"][1]["type"], "function_call");
        assert_eq!(response["output"][2]["content"][0]["text"], "hidden");
        assert_eq!(
            response["output"][2]["content"][1]["text"],
            "second\n\nsummary"
        );
    }

    #[test]
    fn apply_usage_from_frame_accepts_chat_and_responses_usage_shapes() {
        let mut usage = Usage::default();
        apply_usage_from_frame(
            r#"{"usage":{"prompt_tokens":10,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":4},"cache_creation_tokens":2}}"#,
            &mut usage,
        );
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.cache_read_tokens, 4);
        assert_eq!(usage.cache_creation_tokens, 2);

        apply_usage_from_frame(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":20,"output_tokens":5,"input_tokens_details":{"cached_tokens":6},"cache_creation_input_tokens":7}}}"#,
            &mut usage,
        );
        assert_eq!(usage.input_tokens, 20);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 6);
        assert_eq!(usage.cache_creation_tokens, 7);
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
        assert_eq!(turn_id_from_request(&bytes).as_deref(), Some("turn-123"));
    }

    #[test]
    fn extracts_turn_id_and_thread_id_from_headers() {
        let meta = serde_json::json!({
            "turn_id": "turn-abc",
            "thread_id": "thread-xyz",
            "thread_source": "user",
            "session_id": "sess-111",
            "turn_started_at_unix_ms": 1700000000000_u64
        });
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_str(&meta.to_string()).unwrap(),
        );
        assert_eq!(turn_id_from_headers(&headers).as_deref(), Some("turn-abc"));
        assert_eq!(
            thread_id_from_headers(&headers).as_deref(),
            Some("thread-xyz")
        );
        assert_eq!(
            thread_source_from_headers(&headers),
            Some(CodexThreadSource::User)
        );
    }

    #[test]
    fn thread_id_from_headers_prefers_direct_header() {
        let meta = serde_json::json!({
            "turn_id": "turn-abc",
            "thread_id": "thread-from-meta",
        });
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_str(&meta.to_string()).unwrap(),
        );
        headers.insert("thread-id", HeaderValue::from_static("thread-direct"));
        assert_eq!(
            thread_id_from_headers(&headers).as_deref(),
            Some("thread-direct")
        );
    }

    #[test]
    fn summary_appends_to_completed_response_message() {
        let mut acc = SummaryAccumulator::new(CodexSummaryConfig::default(), CodexClientKind::Cli);
        let completed = r#"{"type":"response.completed","response":{"id":"resp_1","output":[{"type":"message","content":[{"type":"output_text","text":"real final"}]}],"usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}"#;
        let out = acc.maybe_append_to_frame(completed, 1_000).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let text = v
            .pointer("/response/output/0/content/0/text")
            .and_then(Value::as_str)
            .unwrap();
        assert!(text.starts_with("real final\n\n"));
        assert!(text.contains("in"));
    }

    #[test]
    fn summary_batch_injects_visible_delta_before_completed() {
        let mut acc = SummaryAccumulator::new(CodexSummaryConfig::default(), CodexClientKind::Cli);
        let frames = vec![
            r#"{"type":"response.output_item.added","response_id":"resp_1","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[]}}"#.to_string(),
            r#"{"type":"response.content_part.added","response_id":"resp_1","item_id":"msg_1","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}"#.to_string(),
            r#"{"type":"response.output_text.delta","response_id":"resp_1","item_id":"msg_1","output_index":0,"content_index":0,"delta":"real final"}"#.to_string(),
            r#"{"type":"response.output_text.done","response_id":"resp_1","item_id":"msg_1","output_index":0,"content_index":0,"text":"real final"}"#.to_string(),
            r#"{"type":"response.output_item.done","response_id":"resp_1","output_index":0,"item":{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"real final"}]}}"#.to_string(),
            r#"{"type":"response.completed","response":{"id":"resp_1","usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}"#.to_string(),
        ];
        let out = acc.maybe_append_to_frame_batch(frames, 1_000);
        let delta = out
            .iter()
            .find_map(|frame| {
                let v: Value = serde_json::from_str(frame).ok()?;
                let delta = v.get("delta").and_then(Value::as_str)?;
                (v.get("type").and_then(Value::as_str) == Some("response.output_text.delta")
                    && delta.starts_with("\n\n"))
                .then(|| delta.to_string())
            })
            .unwrap();
        assert!(delta.contains("in"));
    }

    #[test]
    fn summary_accumulator_uses_global_turn_slot_once() {
        let state = AppState::init(
            vibe_db::Db::memory().expect("db"),
            crate::config::Config::default(),
            15917,
        )
        .expect("state");
        let completed = r#"{"type":"response.completed","response":{"id":"resp_1","output":[{"type":"message","content":[{"type":"output_text","text":"done"}]}],"usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}"#;

        let mut first = SummaryAccumulator::new_for_turn(
            CodexSummaryConfig::default(),
            CodexClientKind::Cli,
            Some(state.clone()),
            Some("turn-1".into()),
            None,
            String::new(),
        );
        let mut second = SummaryAccumulator::new_for_turn(
            CodexSummaryConfig::default(),
            CodexClientKind::Cli,
            Some(state),
            Some("turn-1".into()),
            None,
            String::new(),
        );

        assert!(first.maybe_append_to_frame(completed, 1_000).is_some());
        assert!(second.maybe_append_to_frame(completed, 1_000).is_none());
    }

    // ----- SSE block-structure regression tests -------------------------
    //
    // Background: `maybe_append_to_sse_block` is called per Passthrough-mode
    // upstream SSE block (one `event: T\ndata: J` pair). The summary
    // accumulator may BUFFER a `data:` frame (returning 0 frames) or FLUSH
    // several buffered frames at once (returning N>1 frames). A previous
    // implementation just rewrote `data:` lines in place while preserving the
    // single upstream `event:` line — which produced:
    //
    //   • orphan `event: T` blocks (data buffered → no `data:` line emitted)
    //   • blocks with one `event:` line and several stacked `data:` lines
    //     (multiple frames flushed → multiple `data:` under one `event:`)
    //
    // codex-rs then never saw a valid `response.completed`, the WS/SSE
    // stream was reported as truncated, and the Codex CLI looped through
    // reconnects ending with "stream closed before response.completed".
    //
    // The invariant these tests pin down is: any string returned by
    // `maybe_append_to_sse_block`, when split on `\n\n`, yields blocks where
    // each block has AT MOST one `event:` line AND at most one `data:` line,
    // and if a block has an `event:` line it must also have a `data:` line
    // (no orphan events). The `event:` `type` must match the `data:` JSON
    // `type` field.

    /// Validate the SSE structure invariant on a block (or concatenation of
    /// blocks separated by `\n\n`). Panics with a descriptive message on the
    /// exact violation pattern, so a regression is easy to diagnose.
    fn assert_blocks_well_formed(emitted: &str) {
        for (idx, raw_block) in emitted.split("\n\n").enumerate() {
            let trimmed = raw_block.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut event_type: Option<String> = None;
            let mut data_payloads: Vec<String> = Vec::new();
            for line in raw_block.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(rest) = line.strip_prefix("event:") {
                    assert!(
                        event_type.is_none(),
                        "block #{idx} has multiple `event:` lines:\n{raw_block}"
                    );
                    event_type = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_payloads.push(rest.trim().to_string());
                }
            }
            assert!(
                data_payloads.len() <= 1,
                "block #{idx} stacks {} `data:` lines under one event — \
                 this is the exact malformed-SSE pattern that breaks codex-rs:\n{raw_block}",
                data_payloads.len()
            );
            if let Some(t) = &event_type {
                assert!(
                    !data_payloads.is_empty(),
                    "block #{idx} has orphan `event: {t}` with no `data:` line — \
                     codex-rs sees a typed event with empty payload:\n{raw_block}"
                );
                let data = &data_payloads[0];
                let v: Value = serde_json::from_str(data)
                    .unwrap_or_else(|e| panic!("block #{idx} data is not valid JSON: {e}\n{data}"));
                let inner = v
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("block #{idx} data has no `type` field:\n{data}"));
                assert_eq!(
                    inner, t,
                    "block #{idx} `event:` type does not match `data:` JSON type:\n{raw_block}"
                );
            }
        }
    }

    /// Simulate the exact upstream sequence that caused the production bug:
    /// `output_text.done` → `content_part.done` → `output_item.done` →
    /// `response.completed`. Each comes in as its own SSE block. The first
    /// three trigger buffering; the fourth flushes them. Every emitted block
    /// must remain a well-formed `event: T\ndata: J` pair.
    #[test]
    fn buffered_finalization_flush_emits_well_formed_sse_blocks() {
        let mut acc = SummaryAccumulator::new(CodexSummaryConfig::default(), CodexClientKind::Cli);
        // Seed usage so render_summary doesn't bail out.
        acc.usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            ..Usage::default()
        };

        let blocks = [
            "event: response.output_text.done\n\
             data: {\"type\":\"response.output_text.done\",\"response_id\":\"resp_1\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"text\":\"hi\"}",
            "event: response.content_part.done\n\
             data: {\"type\":\"response.content_part.done\",\"response_id\":\"resp_1\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"hi\"}}",
            "event: response.output_item.done\n\
             data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_1\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hi\"}]}}",
            "event: response.completed\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":100,\"output_tokens\":20,\"total_tokens\":120}}}",
        ];

        // Track which upstream `type`s the gateway emits across all blocks.
        let mut all_emitted = String::new();
        let mut completed_seen = false;
        for (i, block) in blocks.iter().enumerate() {
            let out = acc.maybe_append_to_sse_block(block, 1_000);
            let emitted = out.as_deref().unwrap_or(block);
            assert_blocks_well_formed(emitted);
            if !all_emitted.is_empty() {
                all_emitted.push_str("\n\n");
            }
            all_emitted.push_str(emitted);
            if i == blocks.len() - 1 {
                completed_seen = emitted.contains("response.completed");
            }
        }

        // The final flush MUST surface a well-formed `response.completed`
        // event — that's the terminal signal codex-rs waits for.
        assert!(
            completed_seen,
            "response.completed never emitted as a typed SSE event:\n{all_emitted}"
        );

        // All four upstream event types must show up exactly once in the
        // combined output — the buffer flushes them, it doesn't drop them.
        for t in [
            "response.output_text.done",
            "response.content_part.done",
            "response.output_item.done",
            "response.completed",
        ] {
            let pattern = format!("event: {t}\n");
            assert!(
                all_emitted.contains(&pattern),
                "missing typed event `{t}` in combined stream:\n{all_emitted}"
            );
        }

        // CONTENT-LEVEL invariant: the whole point of buffering the three
        // finalization frames is so the turn-summary text can be appended
        // into their `text` payload before they're released. If summary
        // rendering silently regresses (render_summary returns None, the
        // append helpers stop firing, the modified frame gets dropped during
        // SSE reconstruction, etc.) the gateway still emits structurally
        // valid SSE — but the user-visible "end slot" disappears. Verify the
        // summary text actually rides along on the finalization frames.
        //
        // We don't pin the exact summary string (it depends on style/locale),
        // but we DO require ALL three finalization frames to carry a longer
        // `text` than the original "hi" payload so the test can't pass on a
        // technicality if rendering silently strips the suffix.
        let blocks_out: Vec<&str> = all_emitted.split("\n\n").collect();
        let parse_data = |block: &str| -> Option<Value> {
            block
                .lines()
                .find_map(|l| l.strip_prefix("data:"))
                .map(str::trim)
                .and_then(|d| serde_json::from_str::<Value>(d).ok())
        };
        let find_data_for = |kind: &str| -> Value {
            blocks_out
                .iter()
                .find_map(|b| {
                    let v = parse_data(b)?;
                    (v.get("type").and_then(Value::as_str) == Some(kind)).then_some(v)
                })
                .unwrap_or_else(|| panic!("no `{kind}` event in:\n{all_emitted}"))
        };

        let text_done = find_data_for("response.output_text.done");
        let part_done = find_data_for("response.content_part.done");
        let item_done = find_data_for("response.output_item.done");

        let text_done_text = text_done
            .pointer("/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(
            text_done_text.len() > "hi".len(),
            "response.output_text.done lost its end-slot suffix — \
             `text` is still {text_done_text:?}, summary rendering regressed:\n{text_done}"
        );

        let part_done_text = part_done
            .pointer("/part/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(
            part_done_text.len() > "hi".len(),
            "response.content_part.done lost its end-slot suffix — \
             `part.text` is still {part_done_text:?}:\n{part_done}"
        );

        let item_done_text = item_done
            .pointer("/item/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("");
        assert!(
            item_done_text.len() > "hi".len(),
            "response.output_item.done lost its end-slot suffix — \
             item content text is still {item_done_text:?}:\n{item_done}"
        );

        // And all three should agree: same suffix appended to each.
        let suffix = &text_done_text["hi".len()..];
        assert!(
            !suffix.is_empty(),
            "end-slot suffix is empty; render_summary likely returned None"
        );
        assert!(
            part_done_text.ends_with(suffix),
            "content_part.done suffix ({:?}) doesn't match output_text.done suffix ({:?})",
            &part_done_text[part_done_text.len().saturating_sub(suffix.len())..],
            suffix
        );
        assert!(
            item_done_text.ends_with(suffix),
            "output_item.done suffix ({:?}) doesn't match output_text.done suffix ({:?})",
            &item_done_text[item_done_text.len().saturating_sub(suffix.len())..],
            suffix
        );
    }

    /// Pass-through case: when a block does NOT trigger buffering or flush
    /// (e.g. `response.output_text.delta`), the function should return None
    /// and the caller forwards the upstream block unchanged. Also smoke-test
    /// that an arbitrary unchanged block stays well-formed.
    #[test]
    fn unrelated_block_is_passed_through_unchanged() {
        let mut acc = SummaryAccumulator::new(CodexSummaryConfig::default(), CodexClientKind::Cli);
        let block = "event: response.output_text.delta\n\
                     data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_1\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"hi\"}";
        assert!(
            acc.maybe_append_to_sse_block(block, 100).is_none(),
            "deltas should pass through untouched"
        );
        assert_blocks_well_formed(block);
    }

    /// The validator itself must reject the historical broken pattern,
    /// otherwise it would silently pass the very regression it's meant to
    /// catch. This locks in the meaning of "well-formed".
    #[test]
    #[should_panic(expected = "stacks 2 `data:` lines under one event")]
    fn validator_rejects_stacked_data_lines() {
        let broken = "event: response.completed\n\
                      data: {\"type\":\"response.output_item.done\"}\n\
                      data: {\"type\":\"response.completed\"}";
        assert_blocks_well_formed(broken);
    }

    #[test]
    #[should_panic(expected = "orphan `event:")]
    fn validator_rejects_orphan_event_line() {
        let broken = "event: response.output_text.done";
        assert_blocks_well_formed(broken);
    }

    /// Regression for the multi-turn "end slot disappears" bug.
    ///
    /// In a tool-using Codex turn the SAME `turn_id` is reused across multiple
    /// HTTP requests (first request returns a `function_call`, second request
    /// returns the final assistant message). The per-turn summary slot is a
    /// state-wide dedupe lock: whoever calls `reserve_summary_slot` first
    /// wins, and every subsequent accumulator silently flushes its buffered
    /// finalization frames WITHOUT injecting the summary.
    ///
    /// Bug: the tool-call request had no message-type item to attach the
    /// summary to, but still grabbed the slot — so the follow-up text
    /// request, which DID have a message, found the slot taken and emitted
    /// an end-slot-less response. Net effect: end slot vanishes on every
    /// tool-using turn.
    ///
    /// This test pins down the invariant: in a 2-request turn, the request
    /// that actually carries the assistant message MUST end up with the
    /// summary appended. It does NOT matter which request "wins" the slot,
    /// only that the message-carrying request wins.
    #[test]
    fn end_slot_survives_multi_request_turn_with_tool_call() {
        let state = AppState::init(
            vibe_db::Db::memory().expect("db"),
            crate::config::Config::default(),
            15917,
        )
        .expect("state");

        // Request 1: tool-call only. No message item, just a function_call
        // output_item.done. This is what triggers the bug — the reserve
        // would fire here even though there's nothing to attach to.
        let mut tool_call_acc = SummaryAccumulator::new_for_turn(
            CodexSummaryConfig::default(),
            CodexClientKind::Cli,
            Some(state.clone()),
            Some("turn-abc".into()),
            None,
            "gpt-5.4".into(),
        );
        // Seed usage so render_summary doesn't bail on empty metrics.
        tool_call_acc.usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            ..Usage::default()
        };
        let tool_call_blocks = [
            "event: response.output_item.done\n\
             data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_1\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"completed\",\"name\":\"shell\",\"arguments\":\"{}\",\"call_id\":\"call_1\"}}",
            "event: response.completed\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\"}],\"usage\":{\"input_tokens\":100,\"output_tokens\":20,\"total_tokens\":120}}}",
        ];
        for block in &tool_call_blocks {
            let _ = tool_call_acc.maybe_append_to_sse_block(block, 1_000);
        }

        // Request 2: same turn_id. Final assistant message — this is the one
        // the user actually reads. The summary MUST land here.
        let mut text_acc = SummaryAccumulator::new_for_turn(
            CodexSummaryConfig::default(),
            CodexClientKind::Cli,
            Some(state.clone()),
            Some("turn-abc".into()),
            None,
            "gpt-5.4".into(),
        );
        text_acc.usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            ..Usage::default()
        };
        let text_blocks = [
            "event: response.output_text.done\n\
             data: {\"type\":\"response.output_text.done\",\"response_id\":\"resp_2\",\"item_id\":\"msg_2\",\"output_index\":0,\"content_index\":0,\"text\":\"final answer\"}",
            "event: response.content_part.done\n\
             data: {\"type\":\"response.content_part.done\",\"response_id\":\"resp_2\",\"item_id\":\"msg_2\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"final answer\"}}",
            "event: response.output_item.done\n\
             data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_2\",\"output_index\":0,\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"final answer\"}]}}",
            "event: response.completed\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"output\":[],\"usage\":{\"input_tokens\":100,\"output_tokens\":20,\"total_tokens\":120}}}",
        ];
        let mut combined = String::new();
        for block in &text_blocks {
            let emitted = text_acc
                .maybe_append_to_sse_block(block, 2_000)
                .unwrap_or_else(|| block.to_string());
            if !combined.is_empty() {
                combined.push_str("\n\n");
            }
            combined.push_str(&emitted);
        }

        // Parse the emitted output_item.done and verify the assistant
        // message text was extended (end-slot suffix appended). "final
        // answer" is 12 chars; anything longer means the summary made it.
        let mut found_message_with_suffix = false;
        for block in combined.split("\n\n") {
            let data = block
                .lines()
                .find_map(|l| l.strip_prefix("data:").map(str::trim));
            let Some(data) = data else { continue };
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if v.get("type").and_then(Value::as_str) != Some("response.output_item.done") {
                continue;
            }
            let item_text = v
                .pointer("/item/content/0/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            if v.pointer("/item/type").and_then(Value::as_str) == Some("message")
                && item_text.len() > "final answer".len()
            {
                found_message_with_suffix = true;
                break;
            }
        }
        assert!(
            found_message_with_suffix,
            "end slot disappeared on the message-carrying request of a multi-step \
             tool-using turn — the per-turn summary slot was likely consumed by an \
             earlier request that had no message to attach to. Emitted output:\n{combined}"
        );
    }
}
