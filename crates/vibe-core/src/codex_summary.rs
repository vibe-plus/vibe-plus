//! Codex completion-summary rendering and client detection.

use crate::config::{CodexSummaryClientConfig, CodexSummaryConfig, CodexSummaryStyle};
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

    pub fn before_forwarding_sse_block(&mut self, block: &str, latency_ms: i64) -> Option<String> {
        self.record_sse_block_usage(block);
        for raw_line in block.lines() {
            let line = raw_line.trim_end_matches('\r');
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            if let Some(frame) = self.before_forwarding_frame(payload.trim(), latency_ms) {
                return Some(frame);
            }
        }
        None
    }

    pub fn before_forwarding_frame(&mut self, frame_json: &str, latency_ms: i64) -> Option<String> {
        if frame_has_output_text(frame_json) {
            self.record_first_token_ms(latency_ms);
        }
        apply_usage_from_frame(frame_json, &mut self.usage);
        if self.emitted || !response_completed_is_end_turn(frame_json) {
            return None;
        }
        let response_id = response_id_from_frame(frame_json)?;
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
        Some(summary_message_done_event(&response_id, &text))
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

    Some(match client_cfg.style {
        CodexSummaryStyle::FormulaCompact => formula_compact(&labels),
        CodexSummaryStyle::PlainCompact => plain_compact(&labels),
        CodexSummaryStyle::InlineChips => inline_chips(&labels),
        CodexSummaryStyle::StatusBar => status_bar(&labels),
        CodexSummaryStyle::EnglishLight => english_light(&labels),
        CodexSummaryStyle::ChineseLight => chinese_light(&labels),
        CodexSummaryStyle::FormulaLabeled => formula_labeled(&labels),
        CodexSummaryStyle::AsciiPlain => ascii_plain(&labels),
    })
}

pub fn summary_message_done_event(response_id: &str, text: &str) -> String {
    let item_id = format!(
        "vibe_summary_{}",
        response_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    );
    serde_json::json!({
        "type": "response.output_item.done",
        "response_id": response_id,
        "output_index": 0,
        "item": {
            "id": item_id,
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        }
    })
    .to_string()
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

fn metric_labels(cfg: &CodexSummaryConfig, metrics: SummaryMetrics) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if cfg.show_speed {
        if let Some(speed) = metrics.speed() {
            out.push(("speed", format_speed(speed, cfg.speed_decimal_places)));
        }
    }
    if cfg.show_input && metrics.input_tokens > 0 {
        out.push(("in", format_token_count(metrics.input_tokens)));
    }
    if cfg.show_output && metrics.output_tokens > 0 {
        out.push(("out", format_token_count(metrics.output_tokens)));
    }
    if cfg.show_cache && metrics.cache_tokens > 0 {
        out.push(("cache", format_token_count(metrics.cache_tokens)));
    }
    if cfg.show_latency {
        if let Some(ms) = metrics.latency_ms.filter(|ms| *ms > 0) {
            out.push(("lat", format_latency(ms)));
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

fn formula_compact(labels: &[(&'static str, String)]) -> String {
    let parts = labels
        .iter()
        .map(|(k, v)| format!("\\textsf{{{}}}=\\textsf{{{}}}", k, formula_escape(v)))
        .collect::<Vec<_>>()
        .join("\\;\\cdot\\;");
    format!("$$\n\\scriptsize\n\\color{{#64748b}}{{\\textsf{{Vibe+}}\\,\\mid\\,{parts}}}\n$$")
}

fn formula_labeled(labels: &[(&'static str, String)]) -> String {
    let parts = labels
        .iter()
        .map(|(k, v)| format!("\\mathrm{{{}}}=\\textsf{{{}}}", k, formula_escape(v)))
        .collect::<Vec<_>>()
        .join("\\quad");
    format!("$$\n\\small\n\\color{{#64748b}}{{{parts}}}\n$$")
}

fn plain_compact(labels: &[(&'static str, String)]) -> String {
    format!(
        "↯ {}",
        labels
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" · ")
    )
}

fn inline_chips(labels: &[(&'static str, String)]) -> String {
    format!(
        "_↯ {}_",
        labels
            .iter()
            .map(|(k, v)| format!("{k} `{v}`"))
            .collect::<Vec<_>>()
            .join(" · ")
    )
}

fn status_bar(labels: &[(&'static str, String)]) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("`{k} {v}`"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn english_light(labels: &[(&'static str, String)]) -> String {
    format!(
        "_{}_",
        labels
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(" · ")
    )
}

fn chinese_light(labels: &[(&'static str, String)]) -> String {
    let parts = labels
        .iter()
        .map(|(k, v)| match *k {
            "speed" => format!("速度 {v}"),
            "in" => format!("输入 {v}"),
            "out" => format!("输出 {v}"),
            "cache" => format!("缓存 {v}"),
            "lat" => format!("耗时 {v}"),
            _ => format!("{k} {v}"),
        })
        .collect::<Vec<_>>()
        .join(" · ");
    format!("_本轮：{parts}_")
}

fn ascii_plain(labels: &[(&'static str, String)]) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join(" | ")
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
    fn summary_accumulator_uses_global_turn_slot_once() {
        let state = AppState::init(
            vibe_db::Db::memory().expect("db"),
            crate::config::Config::default(),
            15917,
        )
        .expect("state");
        let completed = r#"{"type":"response.completed","response":{"id":"resp_1","usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}}}"#;

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

        assert!(first.before_forwarding_frame(completed, 1_000).is_some());
        assert!(second.before_forwarding_frame(completed, 1_000).is_none());
    }
}
