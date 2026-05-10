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
                ("/v1/chat/completions", std::borrow::Cow::Owned(converted.to_vec()))
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
