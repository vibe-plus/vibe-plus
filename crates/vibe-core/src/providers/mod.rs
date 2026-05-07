//! Provider adapters. Each adapter knows how to translate a client-shaped
//! request into an upstream request, and how to extract usage from the response.

pub mod anthropic;
pub mod gemini;
pub mod openai;

use anyhow::Result;
use bytes::Bytes;
use reqwest::RequestBuilder;
use vibe_protocol::{Provider, ProviderKind};

use crate::usage::Usage;

/// The wire protocol shape of an incoming request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// Body is OpenAI Chat Completions JSON → upstream /v1/chat/completions.
    OpenaiChat,
    /// Body is OpenAI Responses JSON → upstream /v1/responses.
    OpenaiResponses,
    /// Body is Anthropic Messages JSON → upstream /v1/messages.
    Anthropic,
    /// Body is Gemini Native JSON; upstream path supplied separately.
    GeminiNative,
}

pub fn select(provider: &Provider) -> Box<dyn Adapter + Send + Sync> {
    match provider.kind {
        ProviderKind::Anthropic => Box::new(anthropic::AnthropicAdapter),
        ProviderKind::OpenaiCompat | ProviderKind::OpenaiResponses => {
            Box::new(openai::OpenaiAdapter)
        }
        ProviderKind::GeminiNative => Box::new(gemini::GeminiAdapter),
    }
}

pub trait Adapter {
    /// Build the upstream HTTP request.
    ///
    /// `upstream_path` is only set for `Wire::GeminiNative` and carries the
    /// full relative path (e.g. `/v1beta/models/gemini-2.5-pro:generateContent`).
    fn build(
        &self,
        provider: &Provider,
        secret: Option<&str>,
        client: &reqwest::Client,
        wire: Wire,
        body: &[u8],
        upstream_path: Option<&str>,
    ) -> Result<RequestBuilder>;

    /// Parses usage from a non-streaming JSON body.
    fn parse_usage_body(&self, wire: Wire, body: &[u8]) -> Usage;

    /// Receives one SSE event line ("data: …\n"); accumulates into Usage.
    fn parse_usage_stream_event(&self, wire: Wire, line: &str, acc: &mut Usage);

    /// Picks the upstream model id from the requested model + provider aliases.
    fn pick_upstream_model(&self, provider: &Provider, requested: &str) -> String {
        for a in &provider.model_aliases {
            if a.alias == requested {
                return a.upstream_model.clone();
            }
        }
        requested.to_string()
    }

    /// Rewrites the request body to use `upstream_model` instead of the client model.
    /// Gemini override returns body unchanged (model is in the URL, not body).
    fn rewrite_body_model(&self, body: &[u8], upstream_model: &str) -> Result<Bytes> {
        let mut v: serde_json::Value = serde_json::from_slice(body)?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert("model".into(), serde_json::Value::String(upstream_model.into()));
        }
        Ok(Bytes::from(serde_json::to_vec(&v)?))
    }
}
