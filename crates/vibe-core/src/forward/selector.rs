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
    /// auth_ref scheme (keyring:, env:, passthrough, …). Mutually exclusive with `oauth`.
    pub auth_ref: Option<String>,
    /// OAuth direct-storage tokens. Mutually exclusive with `auth_ref`.
    pub oauth: Option<CredOAuth>,
    pub credential_id: Option<String>,
    pub credential: Option<Credential>,
    /// Runtime upstream unit synthesized from provider endpoint + credential.
    /// `provider` remains the UI/profile object; gateway selection operates on this unit.
    pub upstream: vibe_protocol::Upstream,
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
                    if c.disabled_reason.is_some() {
                        // A credential that carries an auto-disable reason should
                        // never participate, even if an older DB row still has
                        // `enabled = 1`.
                        tracing::debug!(
                            cred_id = %c.id, label = %c.label,
                            "skipping credential: auto-disabled reason present"
                        );
                        continue;
                    }
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
                    let upstream = runtime_upstream_for_credential(&pick.provider, c);
                    let epick = ExpandedPick {
                        provider: pick.provider.clone(),
                        upstream_model: pick.upstream_model.clone(),
                        auth_ref,
                        oauth,
                        credential_id: Some(c.id.clone()),
                        credential: Some(c.clone()),
                        upstream,
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
                    let upstream = runtime_upstream_for_provider_auth(&pick.provider);
                    out.push(ExpandedPick {
                        auth_ref: pick.provider.auth_ref.clone(),
                        oauth: None,
                        provider: pick.provider,
                        upstream_model: pick.upstream_model,
                        credential_id: None,
                        credential: None,
                        upstream,
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

fn runtime_upstream_for_credential(
    provider: &vibe_protocol::Provider,
    c: &Credential,
) -> vibe_protocol::Upstream {
    vibe_protocol::Upstream {
        id: format!("{}:{}", provider.id, c.id),
        provider_id: provider.id.clone(),
        kind: provider.kind,
        base_url: provider.base_url.clone(),
        credential_id: Some(c.id.clone()),
        cb_key: c.id.clone(),
        enabled: provider.enabled && c.enabled,
        priority: provider.priority + c.priority,
    }
}

fn runtime_upstream_for_provider_auth(
    provider: &vibe_protocol::Provider,
) -> vibe_protocol::Upstream {
    vibe_protocol::Upstream {
        id: format!("{}:provider", provider.id),
        provider_id: provider.id.clone(),
        kind: provider.kind,
        base_url: provider.base_url.clone(),
        credential_id: None,
        cb_key: provider.id.clone(),
        enabled: provider.enabled,
        priority: provider.priority,
    }
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
// Wave bucketing (123 race schedule)
// ---------------------------------------------------------------------------

/// Maximum number of scheduler waves for the 123 policy.
///
/// Wave 0 sends one pick only so load-balancing / sticky routing decides the
/// first upstream. Wave 1 races up to two picks (prefer 1 sick + 1 healthy).
/// Wave 2 races up to three picks (prefer 1 sick + 2 healthy). Any remaining
/// candidates are intentionally not dispatched; the forwarder will aggregate
/// the collected retry / circuit-skip failures into its normal 503.
const MAX_123_WAVES: usize = 3;

/// Build scheduler waves using Vibe Plus's 123 policy.
///
/// The input is already ordered by routing, CB-aware shuffling, credential
/// expansion, and sticky priority. We therefore run the first healthy pick as
/// the solo first wave so load-balancing / sticky routing decides the first
/// upstream. Remaining picks are split into sick (CB Open or HalfOpen, or
/// persisted failures) and healthy queues while preserving relative order
/// inside each queue.
///
/// Wave 1 prefers `1 sick + 1 healthy`; wave 2 prefers `1 sick + 2 healthy`.
/// If a preferred bucket is short, the wave is backfilled from whatever
/// remaining candidates are available, up to the wave's cap. No wave is
/// generated after wave 2.
pub fn build_waves(picks: Vec<ExpandedPick>, cb: &CircuitBreakers) -> Vec<Vec<ExpandedPick>> {
    use std::collections::VecDeque;

    let mut iter = picks.into_iter();
    let Some(first) = iter.next() else {
        return Vec::new();
    };

    let mut sick = VecDeque::new();
    let mut healthy = VecDeque::new();
    if pick_is_sick(&first, cb) {
        sick.push_back(first);
    } else {
        healthy.push_back(first);
    }
    for pick in iter {
        if pick_is_sick(&pick, cb) {
            sick.push_back(pick);
        } else {
            healthy.push_back(pick);
        }
    }

    let mut waves = Vec::with_capacity(MAX_123_WAVES);
    let mut first_wave = Vec::with_capacity(2);
    take_from_queue(&mut first_wave, &mut healthy, 1, 1);
    if first_wave.is_empty() {
        take_from_queue(&mut first_wave, &mut sick, 1, 1);
    }
    if !first_wave.is_empty() {
        waves.push(first_wave);
    }

    push_123_wave(&mut waves, &mut sick, &mut healthy, 1, 1, 2);
    push_123_wave(&mut waves, &mut sick, &mut healthy, 1, 2, 3);
    waves
}

fn pick_is_sick(pick: &ExpandedPick, cb: &CircuitBreakers) -> bool {
    use crate::circuit_breaker::State;

    matches!(
        cb.state_of(&pick.upstream.cb_key),
        State::Open | State::HalfOpen
    ) || pick
        .credential
        .as_ref()
        .is_some_and(|c| c.consecutive_failures > 0)
}

fn push_123_wave(
    waves: &mut Vec<Vec<ExpandedPick>>,
    sick: &mut std::collections::VecDeque<ExpandedPick>,
    healthy: &mut std::collections::VecDeque<ExpandedPick>,
    preferred_sick: usize,
    preferred_healthy: usize,
    cap: usize,
) {
    let mut wave = Vec::with_capacity(cap);

    take_from_queue(&mut wave, sick, preferred_sick, cap);
    take_from_queue(&mut wave, healthy, preferred_healthy, cap);

    // Backfill shortages without exceeding the cap. Prefer sick first here so
    // an all-sick pool still uses the full wave width, then fill any remaining
    // slots with healthy picks.
    take_from_queue(&mut wave, sick, cap, cap);
    take_from_queue(&mut wave, healthy, cap, cap);

    if !wave.is_empty() {
        waves.push(wave);
    }
}

fn take_from_queue(
    wave: &mut Vec<ExpandedPick>,
    queue: &mut std::collections::VecDeque<ExpandedPick>,
    count: usize,
    cap: usize,
) {
    for _ in 0..count {
        if wave.len() >= cap {
            return;
        }
        let Some(pick) = queue.pop_front() else {
            return;
        };
        wave.push(pick);
    }
}

// ---------------------------------------------------------------------------
// Sticky routing
// ---------------------------------------------------------------------------

/// Re-order `picks` so that the sticky provider/credential comes first,
/// unless that pick is currently blocked by the circuit breaker.
pub fn apply_sticky_priority(
    route: Option<&CodexStickyRoute>,
    picks: Vec<ExpandedPick>,
    cb: &CircuitBreakers,
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
    let (healthy, blocked): (Vec<_>, Vec<_>) = sticky
        .into_iter()
        .partition(|p| !cb.is_blocking(&p.upstream.cb_key));
    if healthy.is_empty() {
        return rest.into_iter().chain(blocked).collect();
    }
    healthy.into_iter().chain(rest).chain(blocked).collect()
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

    if let Some(thread_id) = crate::codex_summary::thread_id_from_value(&v) {
        let trimmed = thread_id.trim();
        if !trimmed.is_empty() {
            return Some(format!("body:codex_thread_id:{trimmed}"));
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
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://example.com".into(),
            protocols: vec![],
            host: None,
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
            auth_ref: None,
            oauth: None,
            credential_id: cred_id.map(str::to_string),
            credential: None,
            upstream: vibe_protocol::Upstream {
                id: format!("{provider_id}:{}", cred_id.unwrap_or("provider")),
                provider_id: provider_id.into(),
                kind: ProviderKind::OpenaiChat,
                base_url: "https://example.com".into(),
                credential_id: cred_id.map(str::to_string),
                cb_key: cred_id.unwrap_or(provider_id).into(),
                enabled: true,
                priority: 10,
            },
        }
    }

    fn make_epick_with_failures(provider_id: &str, cred_id: &str, failures: i32) -> ExpandedPick {
        let mut pick = make_epick(provider_id, Some(cred_id));
        let mut credential = make_cred(cred_id, provider_id, "env:KEY");
        credential.consecutive_failures = failures;
        pick.credential = Some(credential);
        pick
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
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            balance: None,
            usage: None,
            balance_fetched_at: None,
            upstream_vendor: None,
            upstream_username: None,
            upstream_has_session: false,
            upstream_session_expires_at: None,
            upstream_group: None,
            price_multiplier: 1.0,
            windows: Vec::new(),
            disabled_reason: None,
            disabled_at: None,
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
    fn expand_skips_auto_disabled_credential_even_if_enabled() {
        let mut disabled = make_cred("disabled", "p1", "env:KEY");
        disabled.disabled_reason = Some("HTTP 403 from upstream".into());
        disabled.disabled_at = Some(123);
        disabled.enabled = true;

        let ok_cred = make_cred("ok", "p1", "env:KEY2");

        let mut creds_map = HashMap::new();
        creds_map.insert("p1".to_string(), vec![disabled, ok_cred]);

        let result = expand_picks(vec![make_pick("p1", None)], &creds_map, &HashMap::new(), 0);
        assert_eq!(result.len(), 1);
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
    fn sticky_key_prefers_codex_metadata_thread_over_turn() {
        let body =
            br#"{"client_metadata":{"x-codex-turn-metadata":"{\"thread_id\":\"thread-123\",\"turn_id\":\"turn-123\"}"}}"#;
        assert_eq!(
            sticky_key_from_body(body).as_deref(),
            Some("body:codex_thread_id:thread-123")
        );
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
        let result = apply_sticky_priority(Some(&route), picks, &cb_for_tests());
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
        let result = apply_sticky_priority(Some(&route), picks, &cb_for_tests());
        assert_eq!(result[0].provider.id, "p-a");
    }

    #[test]
    fn apply_sticky_none_route_returns_unchanged() {
        let picks = vec![make_epick("p-a", None), make_epick("p-b", None)];
        let result = apply_sticky_priority(None, picks, &cb_for_tests());
        assert_eq!(result[0].provider.id, "p-a");
    }

    #[test]
    fn apply_sticky_does_not_promote_blocked_pick_ahead_of_healthy_candidates() {
        let cb = cb_for_tests();
        cb.force_open("c-b");
        let picks = vec![
            make_epick("p-b", Some("c-b")),
            make_epick("p-a", Some("c-a")),
        ];
        let route = CodexStickyRoute {
            provider_id: "p-b".into(),
            credential_id: Some("c-b".into()),
        };
        let result = apply_sticky_priority(Some(&route), picks, &cb);
        assert_eq!(result[0].provider.id, "p-a");
        assert_eq!(result[1].provider.id, "p-b");
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

    // ─── build_waves: 123 race schedule ────────────────────────────────

    fn cb_for_tests() -> CircuitBreakers {
        use crate::config::FailoverConfig;
        CircuitBreakers::new(FailoverConfig {
            failure_threshold: 1,
            success_threshold: 1,
            open_timeout_secs: 9999,
            inject_cache: false,
        })
    }

    fn cb_for_half_open_tests() -> CircuitBreakers {
        use crate::config::FailoverConfig;
        CircuitBreakers::new(FailoverConfig {
            failure_threshold: 1,
            success_threshold: 1,
            open_timeout_secs: 0,
            inject_cache: false,
        })
    }

    fn ids(wave: &[ExpandedPick]) -> Vec<String> {
        wave.iter().map(|p| p.provider.id.clone()).collect()
    }

    fn wave_ids(waves: &[Vec<ExpandedPick>]) -> Vec<Vec<String>> {
        waves.iter().map(|w| ids(w)).collect()
    }

    #[test]
    fn build_waves_empty_input_returns_empty() {
        let cb = cb_for_tests();
        assert!(build_waves(Vec::new(), &cb).is_empty());
    }

    #[test]
    fn build_waves_one_healthy_pick_is_one_wave_of_one() {
        let cb = cb_for_tests();
        let waves = build_waves(vec![make_epick("p", None)], &cb);
        assert_eq!(wave_ids(&waves), vec![vec!["p".to_string()]]);
    }

    #[test]
    fn build_waves_healthy_only_uses_1_2_3_then_drops_rest() {
        let cb = cb_for_tests();
        let picks: Vec<ExpandedPick> = (0..8)
            .map(|i| make_epick(&format!("h-{i}"), None))
            .collect();
        let waves = build_waves(picks, &cb);
        assert_eq!(
            wave_ids(&waves),
            vec![
                vec!["h-0".to_string()],
                vec!["h-1".to_string(), "h-2".to_string()],
                vec!["h-3".to_string(), "h-4".to_string(), "h-5".to_string()],
            ]
        );
    }

    #[test]
    fn build_waves_mixed_respects_first_pick_then_1_sick_1_healthy_then_1_sick_2_healthy() {
        let cb = cb_for_tests();
        cb.force_open("s-0");
        cb.force_open("s-1");
        cb.force_open("s-2");
        let picks = vec![
            make_epick("h-first", None),
            make_epick("s-0", None),
            make_epick("h-0", None),
            make_epick("s-1", None),
            make_epick("h-1", None),
            make_epick("h-2", None),
            make_epick("s-2", None),
            make_epick("h-dropped", None),
        ];

        let waves = build_waves(picks, &cb);

        assert_eq!(
            wave_ids(&waves),
            vec![
                vec!["h-first".to_string()],
                vec!["s-0".to_string(), "h-0".to_string()],
                vec!["s-1".to_string(), "h-1".to_string(), "h-2".to_string()],
            ]
        );
    }

    #[test]
    fn build_waves_treats_persisted_failures_as_sick() {
        let cb = cb_for_tests();
        let picks = vec![
            make_epick_with_failures("p-bad", "c-bad", 12),
            make_epick_with_failures("p-good", "c-good", 0),
            make_epick_with_failures("p-another-good", "c-another-good", 0),
        ];

        let waves = build_waves(picks, &cb);

        assert_eq!(
            wave_ids(&waves),
            vec![
                vec!["p-good".to_string()],
                vec!["p-bad".to_string(), "p-another-good".to_string()],
            ]
        );
    }

    #[test]
    fn build_waves_backfills_when_preferred_bucket_is_short() {
        let cb = cb_for_tests();
        cb.force_open("s-0");
        cb.force_open("s-1");
        cb.force_open("s-2");
        cb.force_open("s-3");
        let picks = vec![
            make_epick("h-first", None),
            make_epick("s-0", None),
            make_epick("s-1", None),
            make_epick("s-2", None),
            make_epick("s-3", None),
        ];

        let waves = build_waves(picks, &cb);

        assert_eq!(
            wave_ids(&waves),
            vec![
                vec!["h-first".to_string()],
                vec!["s-0".to_string(), "s-1".to_string()],
                vec!["s-2".to_string(), "s-3".to_string()],
            ]
        );
    }

    #[test]
    fn build_waves_treats_open_and_half_open_as_sick() {
        let cb = cb_for_half_open_tests();
        cb.force_open("s-open");
        cb.force_open("s-half");
        assert!(cb.allow("s-half"), "expected HalfOpen probe to be allowed");
        let picks = vec![
            make_epick("h-first", None),
            make_epick("s-open", None),
            make_epick("h-0", None),
            make_epick("s-half", None),
            make_epick("h-1", None),
        ];

        let waves = build_waves(picks, &cb);

        assert_eq!(
            wave_ids(&waves),
            vec![
                vec!["h-first".to_string()],
                vec!["s-open".to_string(), "h-0".to_string()],
                vec!["s-half".to_string(), "h-1".to_string()],
            ]
        );
    }

    #[test]
    fn build_waves_prefers_healthy_over_sick_for_first_wave() {
        let cb = cb_for_tests();
        cb.force_open("s-0");
        let picks = vec![
            make_epick("s-0", None),
            make_epick("h-0", None),
            make_epick("h-1", None),
        ];

        let waves = build_waves(picks, &cb);

        assert_eq!(
            wave_ids(&waves),
            vec![
                vec!["h-0".to_string()],
                vec!["s-0".to_string(), "h-1".to_string()],
            ]
        );
    }

    #[test]
    fn build_waves_never_generates_fourth_wave() {
        let cb = cb_for_tests();
        let picks: Vec<ExpandedPick> = (0..20)
            .map(|i| make_epick(&format!("p-{i}"), None))
            .collect();
        let waves = build_waves(picks, &cb);
        assert_eq!(waves.len(), 3);
        assert_eq!(
            waves.iter().map(|w| w.len()).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}
