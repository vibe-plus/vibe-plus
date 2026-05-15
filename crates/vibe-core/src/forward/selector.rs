//! Candidate selection: expand providers to per-credential picks, shuffle,
//! and apply sticky routing.
//!
//! All functions are pure (no I/O, no AppState), making them easy to unit-test.

use crate::circuit_breaker::CircuitBreakers;
use crate::router;
use crate::state::CodexStickyRoute;
use std::collections::HashMap;
use vibe_protocol::{Credential, CredentialPlanSnapshot};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CredOAuth {
    pub access_token: String,
    pub expires_at: Option<i64>,
}

#[derive(Debug)]
pub struct ExpandedPick {
    pub provider: vibe_protocol::Provider,
    pub upstream_model: String,
    /// Circuit-breaker + rate-limit key: credential_id when present, else provider_id.
    pub cb_key: String,
    /// auth_ref scheme (keyring:, env:, passthrough, …). Mutually exclusive with `oauth`.
    pub auth_ref: Option<String>,
    /// OAuth direct-storage tokens. Mutually exclusive with `auth_ref`.
    pub oauth: Option<CredOAuth>,
    pub credential_id: Option<String>,
    pub credential: Option<Credential>,
}

// ---------------------------------------------------------------------------
// Candidate expansion
// ---------------------------------------------------------------------------

/// Expand provider-level picks into credential-level picks.
///
/// - Providers with enabled credentials: each credential becomes one entry.
/// - Providers with **no** enabled credentials: included only when
///   `provider.auth_ref` is non-empty (provider-level auth). Providers
///   that have no credentials AND no auth_ref would always 401 — they are
///   skipped entirely so they don't pollute the attempt trace.
/// - Rate-limited credentials (upstream `Retry-After` window still active) are
///   **skipped entirely** — retrying them would just 429 again.
/// - Codex-plan-exhausted credentials are deferred to end as a last resort.
pub fn expand_picks(
    picks: Vec<router::Pick>,
    creds_by_provider: &HashMap<String, Vec<Credential>>,
    plan_by_cred: &HashMap<String, CredentialPlanSnapshot>,
    counter: usize,
) -> Vec<ExpandedPick> {
    let now = chrono::Utc::now().timestamp();
    let mut out = Vec::new();
    let mut deferred: Vec<ExpandedPick> = Vec::new();

    for pick in picks {
        let creds = creds_by_provider
            .get(&pick.provider.id)
            .filter(|v| !v.is_empty());

        match creds {
            Some(creds) => {
                let n = creds.len();
                let start = if n > 0 { counter % n } else { 0 };
                for i in 0..n {
                    let c = &creds[(start + i) % n];
                    let (auth_ref, oauth) = if c.oauth_access_token.is_some() {
                        (
                            None,
                            Some(CredOAuth {
                                access_token: c.oauth_access_token.clone().unwrap(),
                                expires_at: c.oauth_expires_at,
                            }),
                        )
                    } else {
                        (c.auth_ref.clone(), None)
                    };
                    let epick = ExpandedPick {
                        provider: pick.provider.clone(),
                        upstream_model: pick.upstream_model.clone(),
                        cb_key: c.id.clone(),
                        auth_ref,
                        oauth,
                        credential_id: Some(c.id.clone()),
                        credential: Some(c.clone()),
                    };
                    let rate_limited = cred_is_rate_limited(c, now);
                    let defer_plan = !rate_limited
                        && router::provider_is_chatgpt_codex_official(&pick.provider)
                        && plan_by_cred
                            .get(&c.id)
                            .is_some_and(credential_plan_display_exhausted);
                    if rate_limited {
                        // Upstream has told us the reset window — retrying will just 429 again.
                        tracing::debug!(
                            cred_id = %c.id, label = %c.label,
                            "skipping credential: active rate-limit window"
                        );
                    } else if defer_plan {
                        tracing::debug!(
                            cred_id = %c.id, label = %c.label,
                            "deferring credential: Codex plan snapshot exhausted"
                        );
                        deferred.push(epick);
                    } else {
                        out.push(epick);
                    }
                }
            }
            None => {
                // Only include if the provider itself carries an auth_ref; if all
                // credentials are disabled and there is no provider-level auth,
                // any request will 401 — skip it to avoid polluting the attempt trace.
                let has_provider_auth = pick
                    .provider
                    .auth_ref
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty());
                if has_provider_auth {
                    out.push(ExpandedPick {
                        cb_key: pick.provider.id.clone(),
                        auth_ref: pick.provider.auth_ref.clone(),
                        oauth: None,
                        provider: pick.provider,
                        upstream_model: pick.upstream_model,
                        credential_id: None,
                        credential: None,
                    });
                } else {
                    tracing::debug!(
                        provider_id = %pick.provider.id,
                        provider_name = %pick.provider.name,
                        "skipping provider: no enabled credentials and no provider-level auth_ref"
                    );
                }
            }
        }
    }
    out.extend(deferred);
    out
}

// ---------------------------------------------------------------------------
// Shuffling
// ---------------------------------------------------------------------------

/// Randomly shuffle all available (non-circuit-open) candidates so every
/// healthy provider is equally likely to be selected. Circuit-open providers
/// are moved to the end as a last-resort fallback. Provider `priority` is
/// intentionally ignored.
pub fn shuffle_candidates(
    candidates: Vec<router::Pick>,
    cb: &CircuitBreakers,
) -> Vec<router::Pick> {
    if candidates.len() <= 1 {
        return candidates;
    }
    use rand::seq::SliceRandom as _;
    let mut rng = rand::thread_rng();
    let (mut available, blocked): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|p| !cb.is_blocking(&p.provider.id));
    available.shuffle(&mut rng);
    available.extend(blocked);
    available
}

// ---------------------------------------------------------------------------
// Sticky routing
// ---------------------------------------------------------------------------

/// Re-order `picks` so that the sticky provider/credential comes first.
/// This is pure — resolve the route from AppState before calling this.
pub fn apply_sticky_priority(
    route: Option<&CodexStickyRoute>,
    picks: Vec<ExpandedPick>,
) -> Vec<ExpandedPick> {
    let Some(route) = route else {
        return picks;
    };
    let (sticky, rest): (Vec<_>, Vec<_>) = picks
        .into_iter()
        .partition(|p| pick_matches_sticky(p, route));
    if sticky.is_empty() {
        return rest;
    }
    sticky.into_iter().chain(rest).collect()
}

fn pick_matches_sticky(pick: &ExpandedPick, route: &CodexStickyRoute) -> bool {
    pick.provider.id == route.provider_id && pick.credential_id == route.credential_id
}

// ---------------------------------------------------------------------------
// Sticky key extraction from request body (pure, no I/O)
// ---------------------------------------------------------------------------

/// Extract a sticky routing key from the request body.
/// Returns `"body:<pointer>:<value>"` for session/thread/conversation IDs,
/// or `"turn:<turn_id>"` for Codex turn metadata.
pub fn sticky_key_from_body(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;

    for pointer in [
        "/thread_id",
        "/response/thread_id",
        "/session_id",
        "/response/session_id",
        "/conversation_id",
        "/response/conversation_id",
        "/client_metadata/thread_id",
        "/response/client_metadata/thread_id",
        "/client_metadata/session_id",
        "/response/client_metadata/session_id",
        "/client_metadata/conversation_id",
        "/response/client_metadata/conversation_id",
        "/previous_response_id",
        "/response/previous_response_id",
    ] {
        if let Some(id) = v.pointer(pointer).and_then(|x| x.as_str()) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(format!("body:{pointer}:{trimmed}"));
            }
        }
    }

    crate::codex_summary::turn_id_from_value(&v).map(|t| format!("turn:{t}"))
}

// ---------------------------------------------------------------------------
// Rate-limit and plan helpers
// ---------------------------------------------------------------------------

pub fn cred_is_rate_limited(c: &Credential, now_secs: i64) -> bool {
    let req_exhausted = c.rl_requests_remaining == Some(0)
        && c.rl_requests_reset_at.map_or(false, |r| r > now_secs);
    let tok_exhausted =
        c.rl_tokens_remaining == Some(0) && c.rl_tokens_reset_at.map_or(false, |r| r > now_secs);
    req_exhausted || tok_exhausted
}

/// Matches the website `primaryPlanPercent` display: primary → 5h → 7d, clamped to `[0, 100]`.
pub fn credential_plan_display_percent(snap: &CredentialPlanSnapshot) -> Option<f64> {
    fn clamp_pct(v: Option<f64>) -> Option<f64> {
        let v = v?;
        if v.is_nan() {
            return None;
        }
        Some(v.clamp(0.0, 100.0))
    }
    clamp_pct(snap.codex_primary_used_percent)
        .or_else(|| clamp_pct(snap.codex_5h_used_percent))
        .or_else(|| clamp_pct(snap.codex_7d_used_percent))
}

pub fn credential_plan_display_exhausted(snap: &CredentialPlanSnapshot) -> bool {
    credential_plan_display_percent(snap).is_some_and(|p| p >= 100.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use vibe_protocol::{Credential, CredentialPlanSnapshot, Provider, ProviderKind};

    fn make_provider(id: &str, auth_ref: Option<&str>) -> Provider {
        Provider {
            id: id.into(),
            name: id.into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://example.com".into(),
            auth_ref: auth_ref.map(str::to_string),
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: vec![],
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }

    fn make_pick(provider_id: &str, auth_ref: Option<&str>) -> router::Pick {
        router::Pick {
            provider: make_provider(provider_id, auth_ref),
            upstream_model: "gpt-4".into(),
        }
    }

    fn make_epick(provider_id: &str, cred_id: Option<&str>) -> ExpandedPick {
        ExpandedPick {
            provider: make_provider(provider_id, None),
            upstream_model: "m".into(),
            cb_key: cred_id.unwrap_or(provider_id).to_string(),
            auth_ref: None,
            oauth: None,
            credential_id: cred_id.map(str::to_string),
            credential: None,
        }
    }

    fn snap(pp: Option<f64>, h5: Option<f64>, h7: Option<f64>) -> CredentialPlanSnapshot {
        CredentialPlanSnapshot {
            id: "i".into(),
            credential_id: "c".into(),
            captured_at: 0,
            codex_5h_used_percent: h5,
            codex_7d_used_percent: h7,
            codex_5h_reset_after_seconds: None,
            codex_7d_reset_after_seconds: None,
            codex_primary_used_percent: pp,
            codex_secondary_used_percent: None,
            summary: None,
            source: "t".into(),
        }
    }

    // --- plan percent ---

    #[test]
    fn display_pct_follows_primary_then_5h_then_7d() {
        assert_eq!(
            credential_plan_display_percent(&snap(Some(12.0), Some(100.0), None)),
            Some(12.0)
        );
        assert_eq!(
            credential_plan_display_percent(&snap(None, Some(100.0), Some(50.0))),
            Some(100.0)
        );
    }

    #[test]
    fn exhausted_when_display_hundred() {
        assert!(credential_plan_display_exhausted(&snap(
            None,
            Some(100.0),
            None
        )));
        assert!(!credential_plan_display_exhausted(&snap(
            None,
            Some(99.0),
            None
        )));
    }

    // --- expand_picks: no-auth filtering ---

    #[test]
    fn expand_skips_provider_with_no_creds_and_no_auth_ref() {
        let result = expand_picks(
            vec![make_pick("p1", None)],
            &HashMap::new(),
            &HashMap::new(),
            0,
        );
        assert!(
            result.is_empty(),
            "should skip provider with no credentials and no auth_ref"
        );
    }

    #[test]
    fn expand_includes_provider_with_auth_ref_but_no_credentials() {
        let result = expand_picks(
            vec![make_pick("p2", Some("env:MY_KEY"))],
            &HashMap::new(),
            &HashMap::new(),
            0,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].auth_ref.as_deref(), Some("env:MY_KEY"));
        assert!(result[0].credential_id.is_none());
    }

    #[test]
    fn expand_skips_whitespace_only_auth_ref() {
        let mut p = make_provider("p3", Some("   "));
        p.auth_ref = Some("   ".into());
        let pick = router::Pick {
            provider: p,
            upstream_model: "m".into(),
        };
        let result = expand_picks(vec![pick], &HashMap::new(), &HashMap::new(), 0);
        assert!(result.is_empty());
    }

    fn make_cred(id: &str, provider_id: &str, auth_ref: &str) -> Credential {
        Credential {
            id: id.into(),
            provider_id: provider_id.into(),
            label: id.into(),
            auth_ref: Some(auth_ref.into()),
            plan_type: None,
            notes: None,
            enabled: true,
            priority: 0,
            oauth_access_token: None,
            oauth_has_refresh: false,
            oauth_expires_at: None,
            rl_requests_limit: None,
            rl_requests_remaining: None,
            rl_requests_reset_at: None,
            rl_tokens_limit: None,
            rl_tokens_remaining: None,
            rl_tokens_reset_at: None,
            last_used_at: None,
            last_error: None,
            consecutive_failures: 0,
            created_at: 0,
            updated_at: 0,
            auth_fingerprint: None,
            oauth_account_email: None,
            oauth_account_subject: None,
            oauth_chatgpt_plan_slug: None,
        }
    }

    #[test]
    fn expand_rotates_credentials_by_counter() {
        let mut creds_map = HashMap::new();
        let creds: Vec<Credential> = (0..3)
            .map(|i| make_cred(&format!("c{i}"), "p1", &format!("env:KEY{i}")))
            .collect();
        creds_map.insert("p1".to_string(), creds);

        let picks_at = |counter: usize| -> Vec<String> {
            expand_picks(
                vec![make_pick("p1", None)],
                &creds_map,
                &HashMap::new(),
                counter,
            )
            .iter()
            .map(|p| p.credential_id.clone().unwrap())
            .collect()
        };

        // counter=0 → starts at c0; counter=1 → starts at c1
        let order0 = picks_at(0);
        let order1 = picks_at(1);
        assert_eq!(order0[0], "c0");
        assert_eq!(order1[0], "c1");
        assert_eq!(order0.len(), 3);
    }

    #[test]
    fn expand_skips_rate_limited_credential_entirely() {
        let future = chrono::Utc::now().timestamp() + 3600;
        let mut rl_cred = make_cred("rl", "p1", "env:KEY");
        rl_cred.rl_requests_remaining = Some(0);
        rl_cred.rl_requests_reset_at = Some(future);

        let ok_cred = make_cred("ok", "p1", "env:KEY2");

        let mut creds_map = HashMap::new();
        creds_map.insert("p1".to_string(), vec![rl_cred, ok_cred]);

        let result = expand_picks(vec![make_pick("p1", None)], &creds_map, &HashMap::new(), 0);
        assert_eq!(result.len(), 1, "rate-limited credential must be skipped");
        assert_eq!(result[0].credential_id.as_deref(), Some("ok"));
    }

    #[test]
    fn expand_skips_provider_entirely_when_all_creds_rate_limited() {
        let future = chrono::Utc::now().timestamp() + 3600;
        let mut rl_cred = make_cred("rl", "p1", "env:KEY");
        rl_cred.rl_requests_remaining = Some(0);
        rl_cred.rl_requests_reset_at = Some(future);

        let mut creds_map = HashMap::new();
        creds_map.insert("p1".to_string(), vec![rl_cred]);

        let result = expand_picks(vec![make_pick("p1", None)], &creds_map, &HashMap::new(), 0);
        assert!(
            result.is_empty(),
            "provider with all creds rate-limited should yield no picks"
        );
    }

    // --- sticky key extraction ---

    #[test]
    fn sticky_key_prefers_thread_over_previous_response() {
        let body = br#"{"thread_id": "t-1", "previous_response_id": "r-1"}"#;
        assert_eq!(
            sticky_key_from_body(body).as_deref(),
            Some("body:/thread_id:t-1")
        );
    }

    #[test]
    fn sticky_key_falls_back_to_previous_response() {
        let body = br#"{"previous_response_id":"resp-456"}"#;
        assert_eq!(
            sticky_key_from_body(body).as_deref(),
            Some("body:/previous_response_id:resp-456")
        );
    }

    #[test]
    fn sticky_key_falls_back_to_turn_id() {
        let body = br#"{"client_metadata":{"turn_id":"turn-123"}}"#;
        assert_eq!(sticky_key_from_body(body).as_deref(), Some("turn:turn-123"));
    }

    #[test]
    fn sticky_key_returns_none_for_empty_body() {
        assert!(sticky_key_from_body(b"{}").is_none());
        assert!(sticky_key_from_body(b"").is_none());
    }

    // --- apply_sticky_priority ---

    #[test]
    fn apply_sticky_moves_matching_pick_to_front() {
        let picks = vec![
            make_epick("p-a", Some("c-a")),
            make_epick("p-b", Some("c-b")),
            make_epick("p-c", None),
        ];
        let route = CodexStickyRoute {
            provider_id: "p-b".into(),
            credential_id: Some("c-b".into()),
        };
        let result = apply_sticky_priority(Some(&route), picks);
        assert_eq!(result[0].provider.id, "p-b");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn apply_sticky_no_match_returns_original_order() {
        let picks = vec![
            make_epick("p-a", Some("c-a")),
            make_epick("p-b", Some("c-b")),
        ];
        let route = CodexStickyRoute {
            provider_id: "p-x".into(),
            credential_id: Some("c-x".into()),
        };
        let result = apply_sticky_priority(Some(&route), picks);
        assert_eq!(result[0].provider.id, "p-a");
    }

    #[test]
    fn apply_sticky_none_route_returns_unchanged() {
        let picks = vec![make_epick("p-a", None), make_epick("p-b", None)];
        let result = apply_sticky_priority(None, picks);
        assert_eq!(result[0].provider.id, "p-a");
    }

    // --- shuffle_candidates (CB awareness) ---

    #[test]
    fn shuffle_puts_cb_blocked_providers_last() {
        use crate::config::FailoverConfig;
        let cb = CircuitBreakers::new(FailoverConfig {
            failure_threshold: 1,
            success_threshold: 1,
            open_timeout_secs: 9999,
            inject_cache: false,
        });
        cb.record_failure("p-bad");

        let p_good = make_provider("p-good", None);
        let p_bad = make_provider("p-bad", None);
        let picks = vec![
            router::Pick {
                provider: p_bad.clone(),
                upstream_model: "m".into(),
            },
            router::Pick {
                provider: p_good.clone(),
                upstream_model: "m".into(),
            },
        ];

        let result = shuffle_candidates(picks, &cb);
        assert_eq!(result.last().unwrap().provider.id, "p-bad");
        assert_eq!(result[0].provider.id, "p-good");
    }
}
