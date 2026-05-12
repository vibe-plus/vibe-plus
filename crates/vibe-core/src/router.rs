//! Picks providers for an incoming request.
//!
//! `candidates` returns all matching providers ordered by priority (ascending),
//! used by the retry loop in `forward`. `pick` delegates to `candidates` and
//! returns the first match, kept for tests.
//!
//! Selection strategy:
//! 1. **ChatGPT Codex (OAuth) official endpoint** — `openai-responses` whose
//!    `base_url` points at `chatgpt.com/.../backend-api/codex` always **passes
//!    through** the requested model id. The server decides validity; you should
//!    not need a local alias table to mirror every new official model name.
//! 2. Otherwise, alias match → upstream model from the alias table; if the alias
//!    list is empty → pass through requested model as-is.
//! Providers are sorted by `priority` (lower = higher priority).

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

/// Returns every enabled provider that matches `wire`, sorted by `priority`.
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

    let mut result: Vec<(i32, Pick)> = providers
        .iter()
        .filter(|p| p.enabled && kinds.contains(&p.kind))
        .filter_map(|p| {
            if provider_is_chatgpt_codex_official(p) {
                return Some((
                    p.priority,
                    Pick {
                        provider: p.clone(),
                        upstream_model: requested_model.to_string(),
                    },
                ));
            }
            if p.model_aliases.is_empty() {
                // Pass-through provider: accepts any model.
                Some((
                    p.priority,
                    Pick {
                        provider: p.clone(),
                        upstream_model: requested_model.to_string(),
                    },
                ))
            } else {
                // Alias-configured provider: only match if an alias entry matches.
                p.model_aliases
                    .iter()
                    .find(|a| a.alias == requested_model || a.upstream_model == requested_model)
                    .map(|a| {
                        (
                            p.priority,
                            Pick {
                                provider: p.clone(),
                                upstream_model: a.upstream_model.clone(),
                            },
                        )
                    })
            }
        })
        .collect();

    result.sort_by_key(|(priority, _)| *priority);
    result.into_iter().map(|(_, pick)| pick).collect()
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
    let mut picks = candidates(providers, wire, routed_model);
    if let Some(provider_id) = route.target_provider_id.as_deref() {
        picks.retain(|p| p.provider.id == provider_id);
    }
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
    use vibe_protocol::{ModelAlias, ProviderKind};

    fn sample_codex_provider() -> Provider {
        Provider {
            id: "codex-1".into(),
            name: "Codex".into(),
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://chatgpt.com/backend-api/codex".into(),
            auth_ref: None,
            enabled: true,
            priority: 10,
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
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.deepseek.com".into(),
            auth_ref: None,
            enabled: true,
            priority: 40,
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
            kind: ProviderKind::OpenaiChat,
            base_url: "https://api.deepseek.com".into(),
            auth_ref: None,
            enabled: true,
            priority: 40,
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
