//! Claude Messages completion-summary rendering and SSE injection.

use crate::codex_summary::{render_summary, SummaryMetrics};
use crate::config::CodexSummaryConfig;
use crate::state::AppState;
use crate::usage::Usage;
use axum::http::HeaderMap;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeClientKind {
    App,
    Cli,
    Unknown,
}

#[derive(Clone)]
pub struct ClaudeSummaryAccumulator {
    cfg: CodexSummaryConfig,
    client: ClaudeClientKind,
    state: Option<AppState>,
    request_id: Option<String>,
    usage: Usage,
    first_token_ms: Option<i64>,
    next_content_index: i64,
    emitted: bool,
}

impl ClaudeSummaryAccumulator {
    pub fn new_for_request(
        cfg: CodexSummaryConfig,
        client: ClaudeClientKind,
        state: Option<AppState>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            cfg,
            client,
            state,
            request_id,
            usage: Usage::default(),
            first_token_ms: None,
            next_content_index: 0,
            emitted: false,
        }
    }

    pub fn before_forwarding_sse_block(&mut self, block: &str, latency_ms: i64) -> Option<String> {
        for raw_line in block.lines() {
            let line = raw_line.trim_end_matches('\r');
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let data = payload.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Some(injected) = self.before_forwarding_frame(data, latency_ms) {
                return Some(injected);
            }
        }
        None
    }

    pub fn before_forwarding_frame(&mut self, frame_json: &str, latency_ms: i64) -> Option<String> {
        let Ok(v) = serde_json::from_str::<Value>(frame_json) else {
            return None;
        };
        self.record_frame(&v, latency_ms);
        if self.emitted || !message_delta_is_final_text_turn(&v) {
            return None;
        }
        let metrics =
            SummaryMetrics::from_usage(self.usage, Some(latency_ms.max(0)), self.first_token_ms);
        let text = render_summary(&self.cfg, self.client.into(), metrics)?;
        if let Some(state) = &self.state {
            if !reserve_summary_slot(state, self.request_id.as_deref(), self.client) {
                self.emitted = true;
                return None;
            }
        }
        self.emitted = true;
        Some(summary_sse_block(self.next_content_index.max(0), &text))
    }

    fn record_frame(&mut self, v: &Value, latency_ms: i64) {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("message_start") => {
                apply_message_usage(v.pointer("/message/usage"), &mut self.usage);
                if let Some(content_len) = v
                    .pointer("/message/content")
                    .and_then(|content| content.as_array())
                    .map(|content| content.len() as i64)
                {
                    self.next_content_index = self.next_content_index.max(content_len);
                }
            }
            Some("content_block_start") => {
                if let Some(index) = v.get("index").and_then(|x| x.as_i64()) {
                    self.next_content_index = self.next_content_index.max(index + 1);
                }
            }
            Some("content_block_delta") => {
                let has_text = v
                    .pointer("/delta/text")
                    .and_then(|text| text.as_str())
                    .map(|text| !text.is_empty())
                    .unwrap_or(false);
                if has_text && self.first_token_ms.is_none() {
                    self.first_token_ms = Some(latency_ms.max(0));
                }
                if let Some(index) = v.get("index").and_then(|x| x.as_i64()) {
                    self.next_content_index = self.next_content_index.max(index + 1);
                }
            }
            Some("content_block_stop") => {
                if let Some(index) = v.get("index").and_then(|x| x.as_i64()) {
                    self.next_content_index = self.next_content_index.max(index + 1);
                }
            }
            Some("message_delta") => {
                apply_message_usage(v.get("usage"), &mut self.usage);
            }
            _ => {}
        }
    }
}

impl From<ClaudeClientKind> for crate::codex_summary::CodexClientKind {
    fn from(value: ClaudeClientKind) -> Self {
        match value {
            ClaudeClientKind::App => Self::App,
            ClaudeClientKind::Cli => Self::Cli,
            ClaudeClientKind::Unknown => Self::Unknown,
        }
    }
}

impl ClaudeClientKind {
    fn as_str(self) -> &'static str {
        match self {
            ClaudeClientKind::App => "app",
            ClaudeClientKind::Cli => "cli",
            ClaudeClientKind::Unknown => "unknown",
        }
    }
}

pub fn detect_client(headers: &HeaderMap, route_prefix: Option<&str>) -> ClaudeClientKind {
    let originator = header_value(headers, "originator").to_ascii_lowercase();
    let ua = header_value(headers, "user-agent");
    let ua_l = ua.to_ascii_lowercase();

    if originator.contains("claude desktop") || ua_l.contains("claude desktop") {
        ClaudeClientKind::App
    } else if route_prefix == Some("claude-v1")
        || ua_l.contains("claude-code")
        || ua_l.contains("claude_cli")
        || ua_l.starts_with("claude/")
        || originator.contains("claude")
    {
        ClaudeClientKind::Cli
    } else {
        ClaudeClientKind::Unknown
    }
}

pub fn request_id_from_headers(headers: &HeaderMap) -> Option<String> {
    for name in ["x-request-id", "request-id", "anthropic-request-id"] {
        if let Some(value) = headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(value.to_owned());
        }
    }
    None
}

pub fn append_summary_to_message_body(
    body: &[u8],
    cfg: &CodexSummaryConfig,
    client: ClaudeClientKind,
    metrics: SummaryMetrics,
) -> Option<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(body).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("message") {
        return None;
    }
    if v.get("stop_reason").and_then(|x| x.as_str()) != Some("end_turn") {
        return None;
    }
    let text = render_summary(cfg, client.into(), metrics)?;
    let content = v.get_mut("content")?.as_array_mut()?;
    content.push(serde_json::json!({
        "type": "text",
        "text": format!("\n\n{text}")
    }));
    serde_json::to_vec(&v).ok()
}

fn message_delta_is_final_text_turn(v: &Value) -> bool {
    if v.get("type").and_then(|t| t.as_str()) != Some("message_delta") {
        return false;
    }
    matches!(
        v.pointer("/delta/stop_reason").and_then(|x| x.as_str()),
        Some("end_turn")
    )
}

fn apply_message_usage(usage: Option<&Value>, acc: &mut Usage) {
    let Some(usage) = usage else {
        return;
    };
    if let Some(n) = usage.get("input_tokens").and_then(|x| x.as_i64()) {
        acc.input_tokens = n;
    }
    if let Some(n) = usage.get("output_tokens").and_then(|x| x.as_i64()) {
        acc.output_tokens = n;
    }
    if let Some(n) = usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.get("cache_read_tokens"))
        .and_then(|x| x.as_i64())
    {
        acc.cache_read_tokens = n;
    }
    if let Some(n) = usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.get("cache_creation_tokens"))
        .and_then(|x| x.as_i64())
    {
        acc.cache_creation_tokens = n;
    }
}

fn summary_sse_block(index: i64, text: &str) -> String {
    let start = serde_json::json!({
        "type": "content_block_start",
        "index": index,
        "content_block": {
            "type": "text",
            "text": ""
        }
    });
    let delta = serde_json::json!({
        "type": "content_block_delta",
        "index": index,
        "delta": {
            "type": "text_delta",
            "text": format!("\n\n{text}")
        }
    });
    let stop = serde_json::json!({
        "type": "content_block_stop",
        "index": index
    });
    format!(
        "event: content_block_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\nevent: content_block_stop\ndata: {}\n\n",
        start, delta, stop
    )
}

pub fn summary_slot_key(request_id: Option<&str>, client: ClaudeClientKind) -> String {
    format!(
        "{}|{}",
        request_id.unwrap_or("__unknown_request__"),
        client.as_str()
    )
}

fn summary_slot_ttl(request_id: Option<&str>) -> Duration {
    if request_id.is_some() {
        Duration::from_secs(30 * 60)
    } else {
        Duration::from_secs(90)
    }
}

pub fn reserve_summary_slot(
    state: &AppState,
    request_id: Option<&str>,
    client: ClaudeClientKind,
) -> bool {
    state.remember_claude_summary_key(
        summary_slot_key(request_id, client),
        summary_slot_ttl(request_id),
    )
}

fn header_value(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CodexSummaryConfig, CodexSummaryStyle};
    use axum::http::HeaderValue;

    #[test]
    fn detects_claude_route_as_cli() {
        let headers = HeaderMap::new();
        assert_eq!(
            detect_client(&headers, Some("claude-v1")),
            ClaudeClientKind::Cli
        );
    }

    #[test]
    fn detects_claude_user_agent_as_cli() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("claude-code/2.0"));
        assert_eq!(detect_client(&headers, None), ClaudeClientKind::Cli);
    }

    #[test]
    fn injects_summary_before_end_turn_delta() {
        let mut cfg = CodexSummaryConfig::default();
        cfg.clients.cli.style = CodexSummaryStyle::AsciiPlain;
        let mut acc =
            ClaudeSummaryAccumulator::new_for_request(cfg, ClaudeClientKind::Cli, None, None);

        assert!(acc
            .before_forwarding_frame(
                r#"{"type":"message_start","message":{"usage":{"input_tokens":100,"cache_read_input_tokens":20}}}"#,
                10,
            )
            .is_none());
        assert!(acc
            .before_forwarding_frame(
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#,
                100,
            )
            .is_none());
        let injected = acc
            .before_forwarding_frame(
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":50}}"#,
                1100,
            )
            .expect("summary");
        assert!(injected.contains("event: content_block_start"));
        assert!(injected.contains("data: {\"delta\":{\"text\":\"\\n\\nspeed 50.0/s"));
        assert!(injected.contains("\"index\":1"));
    }

    #[test]
    fn skips_tool_use_stop_reason() {
        let cfg = CodexSummaryConfig::default();
        let mut acc =
            ClaudeSummaryAccumulator::new_for_request(cfg, ClaudeClientKind::Cli, None, None);
        assert!(acc
            .before_forwarding_frame(
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":50}}"#,
                1100,
            )
            .is_none());
    }

    #[test]
    fn appends_summary_to_non_streaming_message() {
        let mut cfg = CodexSummaryConfig::default();
        cfg.clients.cli.style = CodexSummaryStyle::AsciiPlain;
        let body = br#"{"type":"message","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":80,"output_tokens":20}}"#;
        let out = append_summary_to_message_body(
            body,
            &cfg,
            ClaudeClientKind::Cli,
            SummaryMetrics {
                input_tokens: 80,
                output_tokens: 20,
                cache_tokens: 0,
                latency_ms: Some(1000),
                first_token_ms: None,
            },
        )
        .expect("summary");
        let v: Value = serde_json::from_slice(&out).unwrap();
        let content = v.get("content").and_then(|x| x.as_array()).unwrap();
        assert_eq!(content.len(), 2);
        assert!(content[1]["text"]
            .as_str()
            .unwrap()
            .contains("speed 20.0/s"));
    }
}
