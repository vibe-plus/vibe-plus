//! Picks providers for an incoming request.
//!
//! `candidates` returns all matching providers ordered by priority (ascending),
//! used by the retry loop in `forward`. `pick` delegates to `candidates` and
//! returns the first match, kept for tests.
//!
//! Selection strategy:
//! 1. Alias match → upstream model from the alias table.
//! 2. Kind match  → requested model passed through as-is.
//! Providers are sorted by `priority` (lower = higher priority).

use crate::providers::Wire;
use anyhow::{Context, Result};
use vibe_protocol::{Provider, ProviderKind};

pub struct Pick {
    pub provider: Provider,
    pub upstream_model: String,
}

/// Returns every enabled provider that matches `wire`, sorted by `priority`.
/// Each entry has its upstream model resolved from the alias table if possible.
pub fn candidates(providers: &[Provider], wire: Wire, requested_model: &str) -> Vec<Pick> {
    let kinds: &[ProviderKind] = match wire {
        Wire::Anthropic => &[ProviderKind::Anthropic],
        Wire::OpenaiChat => &[ProviderKind::OpenaiCompat, ProviderKind::OpenaiResponses],
        Wire::OpenaiResponses => &[ProviderKind::OpenaiResponses],
        Wire::GeminiNative => &[ProviderKind::GeminiNative],
    };

    let mut result: Vec<(i32, Pick)> = providers
        .iter()
        .filter(|p| p.enabled && kinds.contains(&p.kind))
        .map(|p| {
            let upstream_model = p
                .model_aliases
                .iter()
                .find(|a| a.alias == requested_model || a.upstream_model == requested_model)
                .map(|a| a.upstream_model.clone())
                .unwrap_or_else(|| requested_model.to_string());
            (p.priority, Pick { provider: p.clone(), upstream_model })
        })
        .collect();

    result.sort_by_key(|(priority, _)| *priority);
    result.into_iter().map(|(_, pick)| pick).collect()
}

/// Returns the single best provider (first of `candidates`).
pub fn pick(providers: &[Provider], wire: Wire, requested_model: &str) -> Result<Pick> {
    candidates(providers, wire, requested_model)
        .into_iter()
        .next()
        .context("no enabled provider matches request shape")
}
