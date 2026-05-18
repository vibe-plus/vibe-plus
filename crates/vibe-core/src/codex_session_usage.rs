//! Codex rollout/session usage helpers.
//!
//! Codex already persists authoritative per-session token counters in
//! `~/.codex/sessions/YYYY/MM/DD/*.jsonl`.  Vibe uses those files as the
//! durable source for conversation-level usage instead of keeping a second
//! in-memory ledger that resets when the gateway restarts.

use crate::usage::Usage;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CodexSessionUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct CumulativeUsage {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
}

impl CumulativeUsage {
    fn delta_from(self, prev: Option<Self>) -> Self {
        let Some(prev) = prev else {
            return self;
        };
        Self {
            input_tokens: self.input_tokens.saturating_sub(prev.input_tokens),
            output_tokens: self.output_tokens.saturating_sub(prev.output_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_sub(prev.cache_read_tokens),
        }
    }

    fn is_zero(self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.cache_read_tokens == 0
    }
}

pub fn usage_for_session_id(session_id: &str) -> Option<CodexSessionUsage> {
    let path = find_session_file(session_id)?;
    usage_from_session_file(&path).ok().flatten()
}

pub fn find_session_file(session_id: &str) -> Option<PathBuf> {
    let root = codex_sessions_dir();
    find_session_file_in(&root, session_id)
}

fn codex_sessions_dir() -> PathBuf {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("sessions");
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("sessions")
}

pub fn find_session_file_in(root: &Path, session_id: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let matches_name = name.contains(session_id);
            let matches_meta = if matches_name {
                false
            } else {
                session_file_has_id(&path, session_id)
            };
            if !matches_name && !matches_meta {
                continue;
            }
            if best.as_ref().map(|(m, _)| modified > *m).unwrap_or(true) {
                best = Some((modified, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

fn session_file_has_id(path: &Path, session_id: &str) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let reader = BufReader::new(file);
    for line in reader.lines().take(20).flatten() {
        if !line.contains("session_meta") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(payload) = value.get("payload") else {
            continue;
        };
        if payload.get("id").and_then(Value::as_str) == Some(session_id)
            || payload.get("session_id").and_then(Value::as_str) == Some(session_id)
            || payload.get("sessionId").and_then(Value::as_str) == Some(session_id)
        {
            return true;
        }
    }
    false
}

pub fn usage_from_session_file(path: &Path) -> std::io::Result<Option<CodexSessionUsage>> {
    let file = fs::File::open(path)?;
    usage_from_reader(BufReader::new(file))
}

pub fn usage_from_reader(reader: impl BufRead) -> std::io::Result<Option<CodexSessionUsage>> {
    let mut latest_total: Option<CumulativeUsage> = None;
    let mut prev_total: Option<CumulativeUsage> = None;
    let mut current_model = String::new();
    let mut total_cost_usd = 0.0_f64;
    let mut saw_known_cost = false;
    for line in reader.lines() {
        let line = line?;
        let may_have_token_count =
            line.contains("\"token_count\"") && line.contains("total_token_usage");
        let may_have_turn_context = line.contains("\"turn_context\"");
        if !may_have_token_count && !may_have_turn_context {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(model) = value
                .pointer("/payload/model")
                .or_else(|| value.pointer("/payload/info/model"))
                .and_then(Value::as_str)
            {
                current_model = normalize_model(model);
            }
            continue;
        }
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let payload = match value.get("payload") {
            Some(payload) if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
                payload
            }
            _ => continue,
        };
        let Some(total) = payload.pointer("/info/total_token_usage") else {
            continue;
        };
        if let Some(model) = payload
            .pointer("/info/model")
            .or_else(|| payload.pointer("/info/model_name"))
            .or_else(|| payload.get("model"))
            .and_then(Value::as_str)
        {
            current_model = normalize_model(model);
        }
        if let Some(total_usage) = parse_total_token_usage(total) {
            let delta = total_usage.delta_from(prev_total);
            prev_total = Some(total_usage);
            latest_total = Some(total_usage);
            if !delta.is_zero() && !current_model.is_empty() {
                if let Some(cost) = (Usage {
                    input_tokens: delta.input_tokens,
                    output_tokens: delta.output_tokens,
                    cache_read_tokens: delta.cache_read_tokens,
                    cache_creation_tokens: 0,
                })
                .cost_usd(&current_model)
                {
                    total_cost_usd += cost;
                    saw_known_cost = true;
                }
            }
        }
    }
    Ok(latest_total.map(|u| CodexSessionUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_tokens: u.cache_read_tokens,
        total_cost_usd: saw_known_cost.then_some(total_cost_usd),
    }))
}

fn parse_total_token_usage(total: &Value) -> Option<CumulativeUsage> {
    if !total.is_object() {
        return None;
    }
    Some(CumulativeUsage {
        input_tokens: total
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        output_tokens: total
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_read_tokens: total
            .get("cached_input_tokens")
            .or_else(|| total.get("cache_read_input_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
    })
}

fn normalize_model(raw: &str) -> String {
    let mut name = raw.to_ascii_lowercase();
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }
    if name.len() > 11 {
        let suffix = &name[name.len() - 11..];
        if suffix.as_bytes()[0] == b'-'
            && suffix[1..5].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[5] == b'-'
            && suffix[6..8].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[8] == b'-'
            && suffix[9..11].chars().all(|c| c.is_ascii_digit())
        {
            name.truncate(name.len() - 11);
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_latest_total_token_usage_from_codex_jsonl() {
        let jsonl = r#"{"timestamp":"2026-05-17T17:39:49.433Z","type":"event_msg","payload":{"type":"token_count","info":null}}
{"timestamp":"2026-05-17T17:39:52.686Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":11980,"cached_input_tokens":2176,"output_tokens":164,"total_tokens":12144},"last_token_usage":{"input_tokens":11980,"cached_input_tokens":2176,"output_tokens":164,"total_tokens":12144}}}}
{"timestamp":"2026-05-17T17:39:58.384Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":24118,"cached_input_tokens":14080,"output_tokens":320,"total_tokens":24438},"last_token_usage":{"input_tokens":12138,"cached_input_tokens":11904,"output_tokens":156,"total_tokens":12294}}}}
"#;
        let usage = usage_from_reader(Cursor::new(jsonl)).unwrap().unwrap();
        assert_eq!(
            usage,
            CodexSessionUsage {
                input_tokens: 24118,
                output_tokens: 320,
                cache_read_tokens: 14080,
                total_cost_usd: None,
            }
        );
    }

    #[test]
    fn costs_each_delta_with_current_model_when_model_changes() {
        let jsonl = r#"{"type":"turn_context","payload":{"model":"gpt-5.4"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000000,"cached_input_tokens":0,"output_tokens":0}}}}
{"type":"turn_context","payload":{"model":"gpt-4o-mini"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":2000000,"cached_input_tokens":0,"output_tokens":1000000}}}}
"#;
        let usage = usage_from_reader(Cursor::new(jsonl)).unwrap().unwrap();
        let cost = usage.total_cost_usd.unwrap();
        // First delta: 1M input at gpt-5 price = $3.00.
        // Second delta: 1M input + 1M output at gpt-4o-mini = $0.75.
        assert!((cost - 3.75).abs() < 1e-9, "got {cost}");
    }
}
