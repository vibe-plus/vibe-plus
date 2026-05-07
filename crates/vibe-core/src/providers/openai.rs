//! OpenAI-compatible adapter (Chat Completions and Responses).

use super::{Adapter, Wire};
use crate::usage::Usage;
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
        let path = match wire {
            Wire::OpenaiChat => "/v1/chat/completions",
            Wire::OpenaiResponses => "/v1/responses",
            Wire::Anthropic | Wire::GeminiNative => {
                anyhow::bail!("openai adapter cannot serve {:?} wire", wire)
            }
        };
        let url = format!("{}{}", provider.base_url.trim_end_matches('/'), path);
        let mut req = client.post(url).header("content-type", "application/json");
        if let Some(s) = secret {
            req = req.bearer_auth(s);
        }
        Ok(req.body(body.to_vec()))
    }

    fn parse_usage_body(&self, _wire: Wire, body: &[u8]) -> Usage {
        let mut u = Usage::default();
        let v: Result<serde_json::Value, _> = serde_json::from_slice(body);
        if let Ok(v) = v {
            if let Some(usage) = v.get("usage") {
                u.input_tokens = usage.get("prompt_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
                u.output_tokens = usage
                    .get("completion_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                u.cache_read_tokens = usage
                    .pointer("/prompt_tokens_details/cached_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
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
        if let Some(usage) = v.get("usage") {
            if let Some(n) = usage.get("prompt_tokens").and_then(|x| x.as_i64()) {
                acc.input_tokens = n;
            }
            if let Some(n) = usage.get("completion_tokens").and_then(|x| x.as_i64()) {
                acc.output_tokens = n;
            }
            if let Some(n) = usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(|x| x.as_i64())
            {
                acc.cache_read_tokens = n;
            }
        }
    }
}
