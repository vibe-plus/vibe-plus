use anyhow::Result;
use serde::Deserialize;
use std::io::Read;
use vibe_core::config::{ClaudeStatusLineConfig, ClaudeStatusLineStyle, Config};

#[derive(Debug, Deserialize, Default)]
struct StatusLineInput {
    cwd: Option<String>,
    model: Option<StatusLineModel>,
    cost: Option<StatusLineCost>,
    context_window: Option<StatusLineContextWindow>,
}

#[derive(Debug, Deserialize, Default)]
struct StatusLineModel {
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct StatusLineCost {
    total_duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct StatusLineContextWindow {
    current_usage: Option<StatusLineUsage>,
}

#[derive(Debug, Deserialize, Default)]
struct StatusLineUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

pub fn run() -> Result<()> {
    let cfg = Config::default();
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let parsed = serde_json::from_str::<StatusLineInput>(&input).unwrap_or_default();
    println!("{}", render_status_line(&cfg.claude.status_line, &parsed));
    Ok(())
}

fn render_status_line(cfg: &ClaudeStatusLineConfig, input: &StatusLineInput) -> String {
    let mut parts = vec!["Vibe+".to_string()];

    if cfg.show_provider {
        parts.push("Claude".into());
    }

    if cfg.show_model {
        if let Some(model) = model_label(input) {
            parts.push(model);
        }
    }

    if cfg.show_usage {
        if let Some(usage) = usage_label(input) {
            parts.push(usage);
        }
    }

    if cfg.style == ClaudeStatusLineStyle::Detailed {
        if let Some(cwd) = input.cwd.as_deref().and_then(last_path_segment) {
            parts.push(cwd.to_string());
        }
        if let Some(duration) = input
            .cost
            .as_ref()
            .and_then(|cost| cost.total_duration_ms)
            .map(format_duration)
        {
            parts.push(duration);
        }
    }

    parts.join(" | ")
}

fn model_label(input: &StatusLineInput) -> Option<String> {
    input
        .model
        .as_ref()
        .and_then(|model| {
            model
                .display_name
                .as_deref()
                .or(model.id.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(short_model_name)
}

fn usage_label(input: &StatusLineInput) -> Option<String> {
    let usage = input
        .context_window
        .as_ref()
        .and_then(|ctx| ctx.current_usage.as_ref())?;
    let input_tokens = usage.input_tokens.unwrap_or(0)
        + usage.cache_creation_input_tokens.unwrap_or(0)
        + usage.cache_read_input_tokens.unwrap_or(0);
    let output_tokens = usage.output_tokens.unwrap_or(0);
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }
    Some(format!(
        "{} in / {} out",
        format_tokens(input_tokens),
        format_tokens(output_tokens)
    ))
}

fn short_model_name(raw: &str) -> String {
    raw.strip_prefix("claude-")
        .unwrap_or(raw)
        .replace("-202", " 202")
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

fn last_path_segment(path: &str) -> Option<&str> {
    path.rsplit('/')
        .find(|segment| !segment.trim().is_empty())
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_compact_status_line() {
        let cfg = ClaudeStatusLineConfig::default();
        let input = StatusLineInput {
            model: Some(StatusLineModel {
                id: Some("claude-sonnet-4-5-20251001".into()),
                display_name: None,
            }),
            context_window: Some(StatusLineContextWindow {
                current_usage: Some(StatusLineUsage {
                    input_tokens: Some(1200),
                    output_tokens: Some(80),
                    cache_creation_input_tokens: Some(0),
                    cache_read_input_tokens: Some(300),
                }),
            }),
            ..StatusLineInput::default()
        };

        assert_eq!(
            render_status_line(&cfg, &input),
            "Vibe+ | Claude | sonnet-4-5 20251001 | 1.5k in / 80 out"
        );
    }
}
