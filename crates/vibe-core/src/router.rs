//! Picks providers for an incoming request.
//!
//! `candidates` returns all matching providers in database order. The forwarding
//! layer randomizes healthy/authenticated candidates per request so provider
//! priority no longer controls load balancing. `pick` delegates to `candidates`
//! and returns the first match, kept for tests.
//!
//! Selection strategy:
//! 1. **ChatGPT Codex (OAuth) official endpoint** — `openai-responses` whose
//!    `base_url` points at `chatgpt.com/.../backend-api/codex` always **passes
//!    through** the requested model id. The server decides validity; you should
//!    not need a local alias table to mirror every new official model name.
//! 2. Otherwise, alias match → upstream model from the alias table; if the alias
//!    list is empty → pass through requested model as-is.
//! Provider `priority` is deliberately ignored for request distribution.

use crate::providers::Wire;
use anyhow::{Context, Result};
use vibe_protocol::{Provider, ProviderKind};

pub struct Pick {
    pub provider: Provider,
    pub upstream_model: String,
}

/// Official ChatGPT Codex OAuth backend (Codex CLI after browser login).
/// Must accept **any** client-supplied model slug — same as not using a local
/// proxy — so we do not require per-model entries in `model_aliases`.
pub fn provider_is_chatgpt_codex_official(p: &Provider) -> bool {
    p.kind == ProviderKind::OpenaiResponses && {
        let u = p.base_url.to_ascii_lowercase();
        u.contains("chatgpt.com") && u.contains("backend-api") && u.contains("codex")
    }
}

/// Returns every enabled provider that matches `wire` in database order.
/// Each entry has its upstream model resolved from the alias table if possible.
pub fn candidates(providers: &[Provider], wire: Wire, requested_model: &str) -> Vec<Pick> {
    let kinds: &[ProviderKind] = match wire {
        Wire::Anthropic => &[ProviderKind::Anthropic],
        Wire::OpenaiChat => &[ProviderKind::OpenaiChat],
        // Codex CLI uses the OpenAI Responses API (/v1/responses).
        // Vibe Plus intentionally has no Responses→Chat bridge: Chat-only
        // providers must not receive Codex traffic, because many OpenAI-chat
        // gateways return 404 for Responses-shaped requests and should remain
        // visible only to chat clients such as OpenCode.
        Wire::OpenaiResponses => &[ProviderKind::OpenaiResponses],
        Wire::GeminiNative => &[ProviderKind::GeminiNative],
    };

    providers
        .iter()
        .filter(|p| p.enabled && provider_supports_wire_kinds(p, kinds))
        .filter_map(|p| {
            let routed = provider_for_wire_kinds(p, kinds)?;
            if provider_is_chatgpt_codex_official(&routed) {
                return Some(Pick {
                    provider: routed,
                    upstream_model: requested_model.to_string(),
                });
            }
            if routed.model_aliases.is_empty() {
                Some(Pick {
                    provider: routed,
                    upstream_model: requested_model.to_string(),
                })
            } else {
                let upstream = routed
                    .model_aliases
                    .iter()
                    .find(|a| a.alias == requested_model || a.upstream_model == requested_model)
                    .map(|a| a.upstream_model.clone())?;
                Some(Pick {
                    provider: routed,
                    upstream_model: upstream,
                })
            }
        })
        .collect()
}

fn provider_supports_wire_kinds(p: &Provider, kinds: &[ProviderKind]) -> bool {
    p.effective_protocols()
        .iter()
        .any(|proto| kinds.contains(&proto.kind))
}

fn provider_for_wire_kinds(p: &Provider, kinds: &[ProviderKind]) -> Option<Provider> {
    for proto in p.effective_protocols() {
        if kinds.contains(&proto.kind) {
            return Some(p.with_protocol(&proto));
        }
    }
    None
}

/// Returns the single best provider (first of `candidates`).
pub fn pick(providers: &[Provider], wire: Wire, requested_model: &str) -> Result<Pick> {
    candidates(providers, wire, requested_model)
        .into_iter()
        .next()
        .context("no enabled provider matches request shape")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibe_protocol::{ModelAlias, ProviderKind};

    fn sample_codex_provider() -> Provider {
        Provider {
            id: "codex-1".into(),
            name: "Codex".into(),
            group_name: None,
            avatar_url: None,
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://chatgpt.com/backend-api/codex".into(),
            protocols: vec![],
            host: None,
            auth_ref: None,
            enabled: true,
            priority: 10,
            supports_websocket: Some(true),
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![ModelAlias {
                alias: "gpt-5.4".into(),
                upstream_model: "gpt-5.4".into(),
            }],
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn codex_official_passes_through_unknown_model_without_alias() {
        let p = sample_codex_provider();
        let steal = Provider {
            id: "ds".into(),
            name: "deepseek".into(),
            group_name: None,
            avatar_url: None,
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.deepseek.com".into(),
            protocols: vec![],
            host: None,
            auth_ref: None,
            enabled: true,
            priority: 40,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![ModelAlias {
                alias: "gpt-5.5".into(),
                upstream_model: "deepseek-chat".into(),
            }],
            created_at: 0,
            updated_at: 0,
        };
        let list = vec![p.clone(), steal];
        let picks = candidates(&list, Wire::OpenaiResponses, "gpt-5.5-new-official");
        assert_eq!(picks.len(), 1);
        assert_eq!(picks[0].provider.id, "codex-1");
        assert_eq!(picks[0].upstream_model, "gpt-5.5-new-official");
    }

    #[test]
    fn non_codex_still_requires_alias_when_aliases_nonempty() {
        let ds = Provider {
            id: "ds".into(),
            name: "deepseek".into(),
            group_name: None,
            avatar_url: None,
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.deepseek.com".into(),
            protocols: vec![],
            host: None,
            auth_ref: None,
            enabled: true,
            priority: 40,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![ModelAlias {
                alias: "gpt-5.5".into(),
                upstream_model: "deepseek-chat".into(),
            }],
            created_at: 0,
            updated_at: 0,
        };
        let list = vec![ds.clone()];
        assert!(
            candidates(&list, Wire::OpenaiResponses, "gpt-unknown").is_empty(),
            "compat provider with restricted aliases must not match arbitrary models"
        );
    }

    #[test]
    fn chat_wire_does_not_include_responses_only_providers() {
        let codex = sample_codex_provider();
        let list = vec![codex];
        assert!(candidates(&list, Wire::OpenaiChat, "gpt-4o").is_empty());
    }
}
