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
}

#[derive(Clone)]
pub struct SummaryAccumulator {
    cfg: CodexSummaryConfig,
    client: CodexClientKind,
    state: Option<AppState>,
    turn_id: Option<String>,
    usage: Usage,
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
        Self::new_for_turn(cfg, client, None, None)
    }

    pub fn new_for_turn(
        cfg: CodexSummaryConfig,
        client: CodexClientKind,
        state: Option<AppState>,
        turn_id: Option<String>,
    ) -> Self {
        Self {
            cfg,
            client,
            state,
            turn_id,
            usage: Usage::default(),
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

            let metrics = SummaryMetrics::from_usage(
                self.usage,
                Some(latency_ms.max(0)),
                self.first_token_ms,
            );
            let Some(text) = render_summary(&self.cfg, self.client, metrics) else {
                self.flush_pending_finalization(&mut out);
                out.push(frame_json);
                continue;
            };
            if let Some(state) = &self.state {
                if !reserve_summary_slot(state, self.turn_id.as_deref(), self.client) {
                    self.emitted = true;
                    self.flush_pending_finalization(&mut out);
                    out.push(frame_json);
                    continue;
                }
            }
            self.emitted = true;

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

    fn record_frame_state(&mut self, frame_json: &str, latency_ms: i64) {
        if frame_has_output_text(frame_json) {
            self.record_first_token_ms(latency_ms);
        }
        apply_usage_from_frame(frame_json, &mut self.usage);
        self.capture_text_frame(frame_json);
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
        if frame_has_output_text(frame_json) {
            self.record_first_token_ms(latency_ms);
        }
        apply_usage_from_frame(frame_json, &mut self.usage);
        if self.emitted || !response_completed_is_end_turn(frame_json) {
            return None;
        }
        let metrics =
            SummaryMetrics::from_usage(self.usage, Some(latency_ms.max(0)), self.first_token_ms);
        let text = render_summary(&self.cfg, self.client, metrics)?;
        if let Some(state) = &self.state {
            if !reserve_summary_slot(state, self.turn_id.as_deref(), self.client) {
                self.emitted = true;
                return None;
            }
        }
        self.emitted = true;
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
        _ => key.to_string(),
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
        );
        let mut second = SummaryAccumulator::new_for_turn(
            CodexSummaryConfig::default(),
            CodexClientKind::Cli,
            Some(state),
            Some("turn-1".into()),
        );

        assert!(first.maybe_append_to_frame(completed, 1_000).is_some());
        assert!(second.maybe_append_to_frame(completed, 1_000).is_none());
    }
}
