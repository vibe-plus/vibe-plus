//! Google Gemini Native API adapter.
//!
//! Passes the request body through unchanged. The upstream path
//! (e.g. `/v1beta/models/gemini-2.5-pro:generateContent`) is provided by the
//! server handler via `upstream_path` and appended to the provider base URL.
//!
//! Auth: Gemini uses `?key=<api_key>` query parameter (not Bearer token).
//! If `auth_ref` resolves to a key, it is appended as `?key=`.

use super::{Adapter, Wire};
use crate::usage::{estimate_output_tokens, Usage};
use anyhow::Result;
use reqwest::{Client, RequestBuilder};
use vibe_protocol::Provider;

pub struct GeminiAdapter;

impl Adapter for GeminiAdapter {
    fn build(
        &self,
        provider: &Provider,
        secret: Option<&str>,
        client: &Client,
        _wire: Wire,
        body: &[u8],
        upstream_path: Option<&str>,
    ) -> Result<RequestBuilder> {
        let rel = upstream_path.unwrap_or("/v1beta/models/gemini-pro:generateContent");
        let mut url = format!("{}{}", provider.base_url.trim_end_matches('/'), rel);
        if let Some(key) = secret {
            url.push_str("?key=");
            url.push_str(key);
        }
        Ok(client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_vec()))
    }

    fn parse_usage_body(&self, _wire: Wire, body: &[u8]) -> Usage {
        let mut u = Usage::default();
        let v: Result<serde_json::Value, _> = serde_json::from_slice(body);
        if let Ok(v) = v {
            if let Some(meta) = v.get("usageMetadata") {
                u.input_tokens = meta
                    .get("promptTokenCount")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0);
                u.output_tokens = meta
                    .get("candidatesTokenCount")
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
        if let Ok(v) = v {
            if let Some(meta) = v.get("usageMetadata") {
                if let Some(n) = meta.get("promptTokenCount").and_then(|x| x.as_i64()) {
                    acc.input_tokens = n;
                }
                if let Some(n) = meta.get("candidatesTokenCount").and_then(|x| x.as_i64()) {
                    acc.output_tokens = n;
                    return;
                }
            }
            if let Some(candidates) = v.get("candidates").and_then(|x| x.as_array()) {
                for candidate in candidates {
                    if let Some(parts) = candidate
                        .pointer("/content/parts")
                        .and_then(|x| x.as_array())
                    {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(|x| x.as_str()) {
                                acc.output_tokens += estimate_output_tokens(text);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Gemini model is embedded in the URL path, not the body — skip body rewrite.
    fn rewrite_body_model(&self, body: &[u8], _upstream_model: &str) -> Result<bytes::Bytes> {
        Ok(bytes::Bytes::copy_from_slice(body))
    }
}
