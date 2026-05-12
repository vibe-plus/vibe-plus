//! OpenAI adapter (Chat Completions and Responses).
//!
//! When a `Wire::OpenaiResponses` request is routed to an `OpenaiChat` provider
//! (which only supports `/v1/chat/completions`), this adapter automatically:
//!   1. Converts the Responses API body → Chat Completions body
//!   2. Calls `/v1/chat/completions` instead of `/v1/responses`
//!
//! `OpenaiResponses`-kind providers receive the original body on `/v1/responses`,
//! except for the ChatGPT Codex OAuth endpoint (chatgpt.com/backend-api/codex)
//! which uses `/responses` without the `/v1/` prefix.

use super::{Adapter, Wire};
use crate::transforms;
use crate::usage::Usage;
use anyhow::Result;
use reqwest::{Client, RequestBuilder};
use vibe_protocol::{Provider, ProviderKind};

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
        // When an OpenaiChat provider is asked to serve a Responses-wire request,
        // downconvert to Chat Completions (the only endpoint it exposes).
        let (path, effective_body): (&str, std::borrow::Cow<[u8]>) =
            if wire == Wire::OpenaiResponses && provider.kind == ProviderKind::OpenaiChat {
                let converted = transforms::responses_to_chat(body);
                (
                    "/v1/chat/completions",
                    std::borrow::Cow::Owned(converted.to_vec()),
                )
            } else {
                let p = match wire {
                    Wire::OpenaiChat => "/v1/chat/completions",
                    Wire::OpenaiResponses => {
                        // ChatGPT Codex OAuth endpoint omits the /v1/ prefix:
                        //   https://chatgpt.com/backend-api/codex/responses
                        // All other OpenAI-compat upstreams use /v1/responses.
                        if provider.base_url.contains("chatgpt.com/backend-api") {
                            "/responses"
                        } else {
                            "/v1/responses"
                        }
                    }
                    Wire::Anthropic | Wire::GeminiNative => {
                        anyhow::bail!("openai adapter cannot serve {:?} wire", wire)
                    }
                };
                (p, std::borrow::Cow::Borrowed(body))
            };

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
        }
    }
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
}
