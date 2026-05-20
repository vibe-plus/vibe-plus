//! OpenAI adapter (Chat Completions and Responses).
//!
//! OpenAI Chat providers receive Chat Completions traffic, and OpenAI Responses
//! providers receive Responses traffic. Vibe Plus intentionally does not bridge
//! Responses requests to Chat Completions providers.
//!
//! `OpenaiResponses`-kind providers receive the original body on `/v1/responses`,
//! except for the ChatGPT Codex OAuth endpoint (chatgpt.com/backend-api/codex)
//! which uses `/responses` without the `/v1/` prefix. Base URLs that already end
//! in `/v1` are detected automatically to avoid `/v1/v1/...` URLs.

use super::{Adapter, Wire};
use crate::usage::{estimate_output_tokens, Usage};
use anyhow::Result;
use reqwest::{Client, RequestBuilder};
use vibe_protocol::Provider;

pub struct OpenaiAdapter;

impl Adapter for OpenaiAdapter {
    fn build(
        &self,
        provider: &Provider,
        secret: Option<&str>,
        client: &Client,
        wire: Wire,
        body: &[u8],
        _upstream_path: Option<&str>,
    ) -> Result<RequestBuilder> {
        let path = openai_path(provider, wire)?;
        let effective_body = std::borrow::Cow::Borrowed(body);

        let url = format!("{}{}", provider.base_url.trim_end_matches('/'), path);
        let mut req = client.post(url).header("content-type", "application/json");
        if let Some(s) = secret {
            req = req.bearer_auth(s);
        }
        Ok(req.body(effective_body.into_owned()))
    }

    fn parse_usage_body(&self, _wire: Wire, body: &[u8]) -> Usage {
        let mut u = Usage::default();
        let v: Result<serde_json::Value, _> = serde_json::from_slice(body);
        if let Ok(v) = v {
            if let Some(usage) = v.get("usage") {
                apply_openai_usage(usage, &mut u);
            }
        }
        u
    }

    fn parse_usage_stream_event(&self, _wire: Wire, line: &str, acc: &mut Usage) {
        let payload = match line.strip_prefix("data: ") {
            Some(s) if s != "[DONE]" => s,
            _ => return,
        };
        let v: Result<serde_json::Value, _> = serde_json::from_str(payload);
        let v = match v {
            Ok(v) => v,
            Err(_) => return,
        };
        if let Some(usage) = v.get("usage").or_else(|| v.pointer("/response/usage")) {
            apply_openai_usage(usage, acc);
            return;
        }

        for text in openai_stream_text_deltas(&v) {
            acc.output_tokens += estimate_output_tokens(text);
        }
    }
}

fn base_url_has_v1_suffix(base_url: &str) -> bool {
    base_url
        .trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with("/v1")
}

fn openai_path(provider: &Provider, wire: Wire) -> Result<&'static str> {
    match wire {
        Wire::OpenaiChat => Ok(if base_url_has_v1_suffix(&provider.base_url) {
            "/chat/completions"
        } else {
            "/v1/chat/completions"
        }),
        Wire::OpenaiResponses => {
            // ChatGPT Codex OAuth endpoint omits the /v1/ prefix:
            //   https://chatgpt.com/backend-api/codex/responses
            if provider.base_url.contains("chatgpt.com/backend-api") {
                Ok("/responses")
            } else if base_url_has_v1_suffix(&provider.base_url) {
                Ok("/responses")
            } else {
                Ok("/v1/responses")
            }
        }
        Wire::Anthropic | Wire::GeminiNative => {
            anyhow::bail!("openai adapter cannot serve {:?} wire", wire)
        }
    }
}

fn openai_stream_text_deltas(v: &serde_json::Value) -> Vec<&str> {
    let mut out = Vec::new();

    // Responses API: response.output_text.delta frames carry live text in `delta`.
    if v.get("type")
        .and_then(|x| x.as_str())
        .is_some_and(|t| t.ends_with(".delta"))
    {
        if let Some(delta) = v.get("delta").and_then(|x| x.as_str()) {
            out.push(delta);
        }
    }

    // Chat Completions API: each choice carries a delta content field.
    if let Some(choices) = v.get("choices").and_then(|x| x.as_array()) {
        for choice in choices {
            if let Some(delta) = choice.pointer("/delta/content").and_then(|x| x.as_str()) {
                out.push(delta);
            }
        }
    }

    out
}

fn apply_openai_usage(usage: &serde_json::Value, acc: &mut Usage) {
    if let Some(n) = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|x| x.as_i64())
    {
        acc.input_tokens = n;
    }
    if let Some(n) = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|x| x.as_i64())
    {
        acc.output_tokens = n;
    }
    if let Some(n) = usage
        .pointer("/prompt_tokens_details/cached_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.get("cached_input_tokens"))
        .and_then(|x| x.as_i64())
    {
        acc.cache_read_tokens = n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Adapter;
    use vibe_protocol::ProviderKind;

    #[test]
    fn parses_chat_completion_usage_body() {
        let usage = OpenaiAdapter.parse_usage_body(
            Wire::OpenaiChat,
            br#"{"usage":{"prompt_tokens":11,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":3}}}"#,
        );

        assert_eq!(usage.input_tokens, 11);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.cache_read_tokens, 3);
    }

    #[test]
    fn parses_responses_usage_body() {
        let usage = OpenaiAdapter.parse_usage_body(
            Wire::OpenaiResponses,
            br#"{"usage":{"input_tokens":42,"input_tokens_details":{"cached_tokens":12},"output_tokens":5,"total_tokens":47}}"#,
        );

        assert_eq!(usage.input_tokens, 42);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 12);
    }

    #[test]
    fn parses_responses_completed_stream_usage() {
        let mut usage = Usage::default();
        OpenaiAdapter.parse_usage_stream_event(
            Wire::OpenaiResponses,
            r#"data: {"type":"response.completed","response":{"id":"resp_1","usage":{"input_tokens":9,"input_tokens_details":{"cached_tokens":4},"output_tokens":6,"output_tokens_details":{"reasoning_tokens":2},"total_tokens":15}}}"#,
            &mut usage,
        );

        assert_eq!(usage.input_tokens, 9);
        assert_eq!(usage.output_tokens, 6);
        assert_eq!(usage.cache_read_tokens, 4);
    }

    #[test]
    fn normalizes_openai_paths_with_v1_suffix() {
        let provider = Provider {
            id: "p".into(),
            name: "p".into(),
            group_name: None,
            avatar_url: None,
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.example.com/v1".into(),
            protocols: vec![],
            host: None,
            auth_ref: None,
            enabled: true,
            priority: 1,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: vec![],
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        };

        assert_eq!(
            openai_path(&provider, Wire::OpenaiChat).unwrap(),
            "/chat/completions"
        );
        assert_eq!(
            openai_path(&provider, Wire::OpenaiResponses).unwrap(),
            "/responses"
        );
    }
}
