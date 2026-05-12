//! Anthropic Messages API adapter.

use super::{Adapter, Wire};
use crate::usage::Usage;
use anyhow::Result;
use reqwest::{Client, RequestBuilder};
use vibe_protocol::Provider;

pub struct AnthropicAdapter;

impl Adapter for AnthropicAdapter {
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
            Wire::Anthropic => "/v1/messages",
            _ => anyhow::bail!("anthropic adapter cannot serve {:?} wire", wire),
        };
        let url = format!("{}{}", provider.base_url.trim_end_matches('/'), path);
        let mut req = client.post(url).header("anthropic-version", "2023-06-01");
        if let Some(s) = secret {
            req = req.header("x-api-key", s);
        }
        Ok(req
            .header("content-type", "application/json")
            .body(body.to_vec()))
    }

    fn parse_usage_body(&self, _wire: Wire, body: &[u8]) -> Usage {
        let v: Result<serde_json::Value, _> = serde_json::from_slice(body);
        let mut u = Usage::default();
        if let Ok(v) = v {
            if let Some(usage) = v.get("usage") {
                u.input_tokens = usage
                    .get("input_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                u.output_tokens = usage
                    .get("output_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                u.cache_read_tokens = usage
                    .get("cache_read_input_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                u.cache_creation_tokens = usage
                    .get("cache_creation_input_tokens")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
            }
        }
        u
    }

    fn parse_usage_stream_event(&self, _wire: Wire, line: &str, acc: &mut Usage) {
        let payload = match line.strip_prefix("data: ") {
            Some(s) => s,
            None => return,
        };
        let v: Result<serde_json::Value, _> = serde_json::from_str(payload);
        let v = match v {
            Ok(v) => v,
            Err(_) => return,
        };
        // message_start carries input_tokens; message_delta carries output_tokens (cumulative).
        if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
            match t {
                "message_start" => {
                    if let Some(usage) = v.pointer("/message/usage") {
                        if let Some(n) = usage.get("input_tokens").and_then(|x| x.as_i64()) {
                            acc.input_tokens = n;
                        }
                        if let Some(n) = usage
                            .get("cache_read_input_tokens")
                            .and_then(|x| x.as_i64())
                        {
                            acc.cache_read_tokens = n;
                        }
                        if let Some(n) = usage
                            .get("cache_creation_input_tokens")
                            .and_then(|x| x.as_i64())
                        {
                            acc.cache_creation_tokens = n;
                        }
                    }
                }
                "message_delta" => {
                    if let Some(n) = v.pointer("/usage/output_tokens").and_then(|x| x.as_i64()) {
                        acc.output_tokens = n;
                    }
                }
                _ => {}
            }
        }
    }
}
