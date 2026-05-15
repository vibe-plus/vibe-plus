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
//! 3. **Route `target_provider_id`** — still narrows/retargets the model, but does
//!    not force a provider to the front; load balancing stays random in `forward`.
//! Provider `priority` is deliberately ignored for request distribution.

use crate::providers::Wire;
use anyhow::{Context, Result};
use vibe_protocol::{Provider, ProviderKind, Route};

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
        Wire::OpenaiChat => &[ProviderKind::OpenaiChat, ProviderKind::OpenaiResponses],
        // Codex CLI uses the OpenAI Responses API (/v1/responses).
        // Any OpenAI-compat provider can serve this wire — the adapter will call
        // <base_url>/v1/responses on it. Providers that only expose Chat Completions
        // should be registered as OpenaiCompat, and if their upstream supports
        // /v1/responses it will work; if not, the error comes from upstream (not a 503).
        Wire::OpenaiResponses => &[ProviderKind::OpenaiResponses, ProviderKind::OpenaiChat],
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

pub fn matching_route<'a>(routes: &'a [Route], requested_model: &str) -> Option<&'a Route> {
    routes.iter().find(|r| r.match_model == requested_model)
}

pub fn candidates_with_routes(
    providers: &[Provider],
    routes: &[Route],
    wire: Wire,
    requested_model: &str,
) -> (Option<Route>, Vec<Pick>) {
    let Some(route) = matching_route(routes, requested_model) else {
        return (None, candidates(providers, wire, requested_model));
    };
    let routed_model = route.target_model.as_deref().unwrap_or(requested_model);
    // `target_provider_id` is a route hint from the old priority/failover model.
    // Keep the model rewrite behavior, but do not let it override randomized
    // load balancing in the forwarding layer.
    let picks = candidates(providers, wire, routed_model);
    (Some(route.clone()), picks)
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
    use vibe_protocol::{ModelAlias, ProviderKind, Route, RouteTier};

    fn sample_codex_provider() -> Provider {
        Provider {
            id: "codex-1".into(),
            name: "Codex".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://chatgpt.com/backend-api/codex".into(),
            protocols: vec![],
            host: None,auth_ref: None,
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
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.deepseek.com".into(),
            protocols: vec![],
            host: None,auth_ref: None,
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
    fn candidates_with_routes_keeps_provider_order_and_ignores_target_provider_priority_hint() {
        let pri_a = Provider {
            id: "a".into(),
            name: "A".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://api.openai.com/v1".into(),
            protocols: vec![],
            host: None,auth_ref: None,
            enabled: true,
            priority: 5,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let pri_b = Provider {
            id: "b".into(),
            name: "B".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://example.com".into(),
            protocols: vec![],
            host: None,auth_ref: None,
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let r = Route {
            id: "r1".into(),
            name: "r".into(),
            match_model: "gpt-x".into(),
            target_provider_id: Some("b".into()),
            target_model: None,
            tier: RouteTier::Default,
            priority: 0,
        };
        let list = vec![pri_a.clone(), pri_b.clone()];
        let (_, picks) = candidates_with_routes(&list, &[r], Wire::OpenaiResponses, "gpt-x");
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[0].provider.id, "a");
        assert_eq!(picks[1].provider.id, "b");
    }

    #[test]
    fn candidates_with_routes_unknown_target_provider_keeps_full_candidate_order() {
        let pri_a = Provider {
            id: "a".into(),
            name: "A".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://api.openai.com/v1".into(),
            protocols: vec![],
            host: None,auth_ref: None,
            enabled: true,
            priority: 5,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let pri_b = Provider {
            id: "b".into(),
            name: "B".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://example.com".into(),
            protocols: vec![],
            host: None,auth_ref: None,
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let r = Route {
            id: "r1".into(),
            name: "r".into(),
            match_model: "gpt-x".into(),
            target_provider_id: Some("no-such-provider".into()),
            target_model: None,
            tier: RouteTier::Default,
            priority: 0,
        };
        let list = vec![pri_a.clone(), pri_b.clone()];
        let (_, picks) = candidates_with_routes(&list, &[r], Wire::OpenaiResponses, "gpt-x");
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[0].provider.id, "a");
        assert_eq!(picks[1].provider.id, "b");
    }

    #[test]
    fn non_codex_still_requires_alias_when_aliases_nonempty() {
        let ds = Provider {
            id: "ds".into(),
            name: "deepseek".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.deepseek.com".into(),
            protocols: vec![],
            host: None,auth_ref: None,
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
}
