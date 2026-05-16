//! DAOs for the four tables. Synchronous; callers use `spawn_blocking`.

use crate::Db;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use vibe_protocol::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ProviderConfig {
    #[serde(default = "default_passthrough_mode")]
    passthrough_mode: bool,
    #[serde(default)]
    group_name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    supports_websocket: Option<bool>,
    #[serde(default)]
    remote_models: Vec<String>,
    #[serde(default)]
    remote_models_fetched_at: Option<i64>,
    #[serde(default)]
    last_speedtest: Option<ProviderSpeedtestResult>,
    #[serde(default)]
    protocols: Vec<ProviderProtocol>,
    #[serde(default)]
    host: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            passthrough_mode: default_passthrough_mode(),
            group_name: None,
            avatar_url: None,
            supports_websocket: None,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            protocols: Vec::new(),
            host: None,
        }
    }
}

fn normalize_protocols(input: &ProviderInput) -> Vec<ProviderProtocol> {
    if !input.protocols.is_empty() {
        return input.protocols.clone();
    }
    vec![ProviderProtocol {
        kind: input.kind,
        base_url: input.base_url.clone(),
        model_aliases: input.model_aliases.clone(),
    }]
}

fn host_key_from_base(base_url: &str) -> Option<String> {
    vibe_protocol::canonical_provider_host(base_url)
}

fn provider_effective_host(p: &Provider) -> Option<String> {
    if let Some(ref h) = p.host {
        if let Some(key) = vibe_protocol::canonical_provider_host(h) {
            return Some(key);
        }
    }
    for proto in p.effective_protocols() {
        if let Some(key) = host_key_from_base(&proto.base_url) {
            return Some(key);
        }
    }
    host_key_from_base(&p.base_url)
}

fn merge_protocol_lists(
    existing: Vec<ProviderProtocol>,
    incoming: Vec<ProviderProtocol>,
) -> Vec<ProviderProtocol> {
    let mut out = existing;
    for proto in incoming {
        let key = format!(
            "{}::{}",
            provider_kind_to_str(proto.kind),
            proto.base_url.trim_end_matches('/').to_lowercase()
        );
        if out.iter().any(|p| {
            format!(
                "{}::{}",
                provider_kind_to_str(p.kind),
                p.base_url.trim_end_matches('/').to_lowercase()
            ) == key
        }) {
            continue;
        }
        out.push(proto);
    }
    out
}

fn primary_from_protocols(protocols: &[ProviderProtocol]) -> Option<(ProviderKind, String)> {
    protocols.first().map(|p| (p.kind, p.base_url.clone()))
}

fn default_passthrough_mode() -> bool {
    true
}

fn normalize_opt_string(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn provider_config_from_provider(p: &Provider) -> ProviderConfig {
    ProviderConfig {
        passthrough_mode: p.passthrough_mode,
        group_name: p.group_name.clone(),
        avatar_url: p.avatar_url.clone(),
        supports_websocket: p.supports_websocket,
        remote_models: p.remote_models.clone(),
        remote_models_fetched_at: p.remote_models_fetched_at,
        last_speedtest: p.last_speedtest.clone(),
        protocols: p.effective_protocols(),
        host: p.host.clone(),
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn provider_kind_to_str(k: ProviderKind) -> &'static str {
    match k {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenaiChat => "openai-chat",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GeminiNative => "gemini-native",
    }
}

fn provider_kind_from_str(s: &str) -> Result<ProviderKind> {
    Ok(match s {
        "anthropic" => ProviderKind::Anthropic,
        // "openai-compat" is the legacy DB value — map to the renamed variant.
        "openai-chat" | "openai-compat" => ProviderKind::OpenaiChat,
        "openai-responses" => ProviderKind::OpenaiResponses,
        "gemini-native" => ProviderKind::GeminiNative,
        other => anyhow::bail!("unknown provider kind: {other}"),
    })
}

fn route_tier_to_str(t: RouteTier) -> &'static str {
    match t {
        RouteTier::High => "high",
        RouteTier::Low => "low",
        RouteTier::Default => "default",
    }
}

fn _route_tier_from_str(s: &str) -> Result<RouteTier> {
    Ok(match s {
        "high" => RouteTier::High,
        "low" => RouteTier::Low,
        "default" => RouteTier::Default,
        other => anyhow::bail!("unknown route tier: {other}"),
    })
}

fn upstream_attempt_phase_to_str(p: UpstreamAttemptPhase) -> &'static str {
    match p {
        UpstreamAttemptPhase::Connecting => "connecting",
        UpstreamAttemptPhase::Streaming => "streaming",
        UpstreamAttemptPhase::Completed => "completed",
        UpstreamAttemptPhase::Failed => "failed",
        UpstreamAttemptPhase::Abandoned => "abandoned",
    }
}

fn upstream_attempt_phase_from_str(s: &str) -> Result<UpstreamAttemptPhase> {
    Ok(match s {
        "connecting" => UpstreamAttemptPhase::Connecting,
        "streaming" => UpstreamAttemptPhase::Streaming,
        "completed" => UpstreamAttemptPhase::Completed,
        "failed" => UpstreamAttemptPhase::Failed,
        "abandoned" => UpstreamAttemptPhase::Abandoned,
        other => anyhow::bail!("unknown upstream attempt phase: {other}"),
    })
}

fn upstream_attempt_outcome_to_str(o: UpstreamAttemptOutcome) -> &'static str {
    match o {
        UpstreamAttemptOutcome::Success => "success",
        UpstreamAttemptOutcome::RetryableError => "retryable-error",
        UpstreamAttemptOutcome::ClientError => "client-error",
        UpstreamAttemptOutcome::RateLimit => "rate-limit",
        UpstreamAttemptOutcome::TransportError => "transport-error",
        UpstreamAttemptOutcome::FallbackAbandon => "fallback-abandon",
        UpstreamAttemptOutcome::CircuitSkip => "circuit-skip",
    }
}

fn upstream_attempt_outcome_from_str(s: &str) -> Result<UpstreamAttemptOutcome> {
    Ok(match s {
        "success" => UpstreamAttemptOutcome::Success,
        "retryable-error" => UpstreamAttemptOutcome::RetryableError,
        "client-error" => UpstreamAttemptOutcome::ClientError,
        "rate-limit" => UpstreamAttemptOutcome::RateLimit,
        "transport-error" => UpstreamAttemptOutcome::TransportError,
        "fallback-abandon" => UpstreamAttemptOutcome::FallbackAbandon,
        "circuit-skip" => UpstreamAttemptOutcome::CircuitSkip,
        other => anyhow::bail!("unknown upstream attempt outcome: {other}"),
    })
}

#[derive(Debug, Clone)]
pub struct CredentialRollingStat {
    pub credential_id: String,
    pub requests: i64,
    pub successes: i64,
    pub failures: i64,
    pub avg_latency_ms: Option<i64>,
}

impl Db {
    // --- providers ----------------------------------------------------------

    pub fn provider_list(&self) -> Result<Vec<Provider>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, kind, base_url, auth_ref, enabled, priority,
                        model_aliases_json, config_json, created_at, updated_at
                 FROM providers ORDER BY priority ASC, created_at ASC",
            )?;
            let rows = stmt.query_map([], row_to_provider)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn provider_get(&self, id: &str) -> Result<Option<Provider>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, kind, base_url, auth_ref, enabled, priority,
                        model_aliases_json, config_json, created_at, updated_at
                 FROM providers WHERE id = ?1",
            )?;
            let r = stmt.query_row(params![id], row_to_provider).optional()?;
            Ok(r)
        })
    }

    pub fn provider_find_by_kind_and_base_url(
        &self,
        kind: ProviderKind,
        base_url: &str,
    ) -> Result<Option<Provider>> {
        fn norm(s: &str) -> String {
            s.trim().trim_end_matches('/').to_lowercase()
        }
        let want = norm(base_url);
        let list = self.provider_list()?;
        Ok(list.into_iter().find(|p| {
            p.effective_protocols()
                .iter()
                .any(|proto| proto.kind == kind && norm(&proto.base_url) == want)
                || (p.kind == kind && norm(&p.base_url) == want)
        }))
    }

    pub fn provider_find_by_host(&self, host: &str) -> Result<Option<Provider>> {
        Ok(self.provider_find_all_by_host(host)?.into_iter().next())
    }

    pub fn provider_find_all_by_host(&self, host: &str) -> Result<Vec<Provider>> {
        let want = vibe_protocol::canonical_provider_host(host).unwrap_or_default();
        if want.is_empty() {
            return Ok(Vec::new());
        }
        let list = self.provider_list()?;
        Ok(list
            .into_iter()
            .filter(|p| provider_effective_host(p).as_deref() == Some(want.as_str()))
            .collect())
    }

    /// Move credentials from duplicate providers (same host) into `keep_id`, then delete dupes.
    pub fn provider_consolidate_by_host(&self, keep_id: &str, host: &str) -> Result<()> {
        let want = vibe_protocol::canonical_provider_host(host).unwrap_or_default();
        if want.is_empty() {
            return Ok(());
        }
        let dupes: Vec<Provider> = self
            .provider_find_all_by_host(&want)?
            .into_iter()
            .filter(|p| p.id != keep_id)
            .collect();
        if dupes.is_empty() {
            return Ok(());
        }
        let now = now_secs();
        for dup in dupes {
            self.with(|c| {
                c.execute(
                    "UPDATE credentials SET provider_id = ?2, updated_at = ?3 WHERE provider_id = ?1",
                    params![dup.id, keep_id, now],
                )?;
                Ok(())
            })?;
            self.provider_delete(&dup.id)?;
        }
        Ok(())
    }

    pub fn provider_insert(&self, input: ProviderInput) -> Result<Provider> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        let protocols = normalize_protocols(&input);
        let (kind, base_url) = primary_from_protocols(&protocols)
            .map(|(k, u)| (k, u))
            .unwrap_or((input.kind, input.base_url.clone()));
        let host = input.host.clone().or_else(|| host_key_from_base(&base_url));
        let aliases_json = serde_json::to_string(&input.model_aliases)?;
        let config_json = serde_json::to_string(&ProviderConfig {
            passthrough_mode: input.passthrough_mode,
            group_name: normalize_opt_string(input.group_name),
            avatar_url: normalize_opt_string(input.avatar_url),
            supports_websocket: input.supports_websocket,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            protocols,
            host,
        })?;
        self.with(|c| {
            c.execute(
                "INSERT INTO providers (id, name, kind, base_url, auth_ref, enabled, priority,
                                        model_aliases_json, config_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    id,
                    input.name,
                    provider_kind_to_str(kind),
                    base_url,
                    input.auth_ref,
                    input.enabled as i32,
                    input.priority,
                    aliases_json,
                    config_json,
                    now,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.provider_get(&id)?
            .context("inserted provider missing on read-back")
    }

    pub fn provider_update(&self, id: &str, input: ProviderInput) -> Result<Provider> {
        let now = now_secs();
        let aliases_json = serde_json::to_string(&input.model_aliases)?;
        let existing = self.provider_get(id)?;
        let existing_cfg = existing
            .as_ref()
            .map(provider_config_from_provider)
            .unwrap_or_default();
        let incoming_protocols = normalize_protocols(&input);
        let protocols = merge_protocol_lists(existing_cfg.protocols.clone(), incoming_protocols);
        let (kind, base_url) = primary_from_protocols(&protocols)
            .map(|(k, u)| (k, u))
            .unwrap_or((input.kind, input.base_url.clone()));
        let host = input.host.clone().or_else(|| host_key_from_base(&base_url));
        let config_json = serde_json::to_string(&ProviderConfig {
            passthrough_mode: input.passthrough_mode,
            group_name: normalize_opt_string(input.group_name),
            avatar_url: normalize_opt_string(input.avatar_url),
            supports_websocket: input.supports_websocket,
            remote_models: existing_cfg.remote_models,
            remote_models_fetched_at: existing_cfg.remote_models_fetched_at,
            last_speedtest: existing_cfg.last_speedtest,
            protocols,
            host,
        })?;
        let updated = self.with(|c| {
            let n = c.execute(
                "UPDATE providers
                 SET name = ?2, kind = ?3, base_url = ?4, auth_ref = ?5,
                     enabled = ?6, priority = ?7, model_aliases_json = ?8, config_json = ?9, updated_at = ?10
                 WHERE id = ?1",
                params![
                    id,
                    input.name,
                    provider_kind_to_str(kind),
                    base_url,
                    input.auth_ref,
                    input.enabled as i32,
                    input.priority,
                    aliases_json,
                    config_json,
                    now,
                ],
            )?;
            Ok(n)
        })?;
        if updated == 0 {
            anyhow::bail!("provider {id} not found");
        }
        self.provider_get(id)?
            .context("updated provider missing on read-back")
    }

    pub fn provider_delete(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
            if n == 0 {
                anyhow::bail!("provider {id} not found");
            }
            Ok(())
        })
    }

    pub fn provider_update_remote_models(
        &self,
        id: &str,
        remote_models: Vec<String>,
        fetched_at: i64,
    ) -> Result<Provider> {
        let provider = self.provider_get(id)?.context("provider missing")?;
        let config_json = serde_json::to_string(&ProviderConfig {
            remote_models,
            remote_models_fetched_at: Some(fetched_at),
            ..provider_config_from_provider(&provider)
        })?;
        self.with(|c| {
            c.execute(
                "UPDATE providers SET config_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, config_json, now_secs()],
            )?;
            Ok(())
        })?;
        self.provider_get(id)?
            .context("updated provider missing on read-back")
    }

    pub fn provider_update_speedtest(
        &self,
        id: &str,
        result: ProviderSpeedtestResult,
    ) -> Result<Provider> {
        let provider = self.provider_get(id)?.context("provider missing")?;
        let config_json = serde_json::to_string(&ProviderConfig {
            last_speedtest: Some(result),
            ..provider_config_from_provider(&provider)
        })?;
        self.with(|c| {
            c.execute(
                "UPDATE providers SET config_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, config_json, now_secs()],
            )?;
            Ok(())
        })?;
        self.provider_get(id)?
            .context("updated provider missing on read-back")
    }

    pub fn provider_update_websocket_support(
        &self,
        id: &str,
        supports_websocket: Option<bool>,
    ) -> Result<Provider> {
        let provider = self.provider_get(id)?.context("provider missing")?;
        let config_json = serde_json::to_string(&ProviderConfig {
            supports_websocket,
            ..provider_config_from_provider(&provider)
        })?;
        self.with(|c| {
            c.execute(
                "UPDATE providers SET config_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, config_json, now_secs()],
            )?;
            Ok(())
        })?;
        self.provider_get(id)?
            .context("updated provider missing on read-back")
    }

    pub fn provider_update_group_name(
        &self,
        id: &str,
        group_name: Option<String>,
    ) -> Result<Provider> {
        let provider = self.provider_get(id)?.context("provider missing")?;
        let config_json = serde_json::to_string(&ProviderConfig {
            group_name: normalize_opt_string(group_name),
            ..provider_config_from_provider(&provider)
        })?;
        self.with(|c| {
            c.execute(
                "UPDATE providers SET config_json = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, config_json, now_secs()],
            )?;
            Ok(())
        })?;
        self.provider_get(id)?
            .context("updated provider missing on read-back")
    }

    // --- routes -------------------------------------------------------------

    pub fn route_list(&self) -> Result<Vec<Route>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, match_model, target_provider_id, target_model, tier, priority
                 FROM routes ORDER BY priority ASC",
            )?;
            let rows = stmt.query_map([], row_to_route)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn route_upsert(&self, route: Route) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO routes (id, name, match_model, target_provider_id, target_model, tier, priority)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   match_model = excluded.match_model,
                   target_provider_id = excluded.target_provider_id,
                   target_model = excluded.target_model,
                   tier = excluded.tier,
                   priority = excluded.priority",
                params![
                    route.id,
                    route.name,
                    route.match_model,
                    route.target_provider_id,
                    route.target_model,
                    route_tier_to_str(route.tier),
                    route.priority,
                ],
            )?;
            Ok(())
        })
    }

    pub fn route_get(&self, id: &str) -> Result<Option<Route>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, match_model, target_provider_id, target_model, tier, priority
                 FROM routes WHERE id = ?1",
            )?;
            let r = stmt.query_row(params![id], row_to_route).optional()?;
            Ok(r)
        })
    }

    pub fn route_insert(&self, input: RouteInput) -> Result<Route> {
        let route = Route {
            id: uuid::Uuid::new_v4().to_string(),
            name: input.name,
            match_model: input.match_model,
            target_provider_id: input.target_provider_id,
            target_model: input.target_model,
            tier: input.tier,
            priority: input.priority,
        };
        self.route_upsert(route.clone())?;
        Ok(route)
    }

    pub fn route_update(&self, id: &str, input: RouteInput) -> Result<Route> {
        if self.route_get(id)?.is_none() {
            anyhow::bail!("route {id} not found");
        }
        let route = Route {
            id: id.to_string(),
            name: input.name,
            match_model: input.match_model,
            target_provider_id: input.target_provider_id,
            target_model: input.target_model,
            tier: input.tier,
            priority: input.priority,
        };
        self.route_upsert(route.clone())?;
        Ok(route)
    }

    pub fn route_delete(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM routes WHERE id = ?1", params![id])?;
            if n == 0 {
                anyhow::bail!("route {id} not found");
            }
            Ok(())
        })
    }

    // --- request logs -------------------------------------------------------

    pub fn log_insert(&self, log: &RequestLog) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO request_logs (
                    id, started_at, app, provider_id, requested_model, upstream_model,
                    status_code, error, latency_ms, first_token_ms,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    estimated_cost_usd,
                    wire, route_prefix, credential_id, cb_key, upstream_http_status,
                    upstream_error_preview, dedupe_key,
                    client_transport, request_headers,
                    request_body, response_body, client_response_body,
                    stream_kind, stream_terminal_seen, stream_end_reason, stream_error_detail,
                    upstream_first_byte_ms, client_first_write_ms,
                    last_upstream_event_ms, last_client_write_ms,
                    upstream_chunk_count, upstream_bytes, client_chunk_count, client_bytes,
                    sse_event_count, sse_data_count, sse_comment_count, sse_keepalive_count,
                    sse_done_count, parse_error_count,
                    first_keepalive_ms, last_keepalive_ms,
                    max_gap_between_upstream_events_ms, max_gap_between_data_events_ms,
                    keepalive_after_last_data_count, last_data_event_ms,
                    bridge_mode, status_injected, terminal_injected, upstream_terminal_type
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                           ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27,
                           ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39,
                           ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51,
                           ?52, ?53, ?54, ?55)
                 ON CONFLICT(id) DO UPDATE SET
                    started_at = excluded.started_at,
                    app = excluded.app,
                    provider_id = excluded.provider_id,
                    requested_model = excluded.requested_model,
                    upstream_model = excluded.upstream_model,
                    status_code = excluded.status_code,
                    error = excluded.error,
                    latency_ms = excluded.latency_ms,
                    first_token_ms = excluded.first_token_ms,
                    input_tokens = excluded.input_tokens,
                    output_tokens = excluded.output_tokens,
                    cache_read_tokens = excluded.cache_read_tokens,
                    cache_creation_tokens = excluded.cache_creation_tokens,
                    estimated_cost_usd = excluded.estimated_cost_usd,
                    wire = excluded.wire,
                    route_prefix = excluded.route_prefix,
                    credential_id = excluded.credential_id,
                    cb_key = excluded.cb_key,
                    upstream_http_status = excluded.upstream_http_status,
                    upstream_error_preview = excluded.upstream_error_preview,
                    dedupe_key = excluded.dedupe_key,
                    client_transport = excluded.client_transport,
                    request_headers = excluded.request_headers,
                    request_body = excluded.request_body,
                    response_body = excluded.response_body,
                    client_response_body = excluded.client_response_body,
                    stream_kind = excluded.stream_kind,
                    stream_terminal_seen = excluded.stream_terminal_seen,
                    stream_end_reason = excluded.stream_end_reason,
                    stream_error_detail = excluded.stream_error_detail,
                    upstream_first_byte_ms = excluded.upstream_first_byte_ms,
                    client_first_write_ms = excluded.client_first_write_ms,
                    last_upstream_event_ms = excluded.last_upstream_event_ms,
                    last_client_write_ms = excluded.last_client_write_ms,
                    upstream_chunk_count = excluded.upstream_chunk_count,
                    upstream_bytes = excluded.upstream_bytes,
                    client_chunk_count = excluded.client_chunk_count,
                    client_bytes = excluded.client_bytes,
                    sse_event_count = excluded.sse_event_count,
                    sse_data_count = excluded.sse_data_count,
                    sse_comment_count = excluded.sse_comment_count,
                    sse_keepalive_count = excluded.sse_keepalive_count,
                    sse_done_count = excluded.sse_done_count,
                    parse_error_count = excluded.parse_error_count,
                    first_keepalive_ms = excluded.first_keepalive_ms,
                    last_keepalive_ms = excluded.last_keepalive_ms,
                    max_gap_between_upstream_events_ms = excluded.max_gap_between_upstream_events_ms,
                    max_gap_between_data_events_ms = excluded.max_gap_between_data_events_ms,
                    keepalive_after_last_data_count = excluded.keepalive_after_last_data_count,
                    last_data_event_ms = excluded.last_data_event_ms,
                    bridge_mode = excluded.bridge_mode,
                    status_injected = excluded.status_injected,
                    terminal_injected = excluded.terminal_injected,
                    upstream_terminal_type = excluded.upstream_terminal_type",
                params![
                    log.id,
                    log.started_at,
                    log.app,
                    log.provider_id,
                    log.requested_model,
                    log.upstream_model,
                    log.status_code,
                    log.error,
                    log.latency_ms,
                    log.first_token_ms,
                    log.input_tokens,
                    log.output_tokens,
                    log.cache_read_tokens,
                    log.cache_creation_tokens,
                    log.estimated_cost_usd,
                    log.wire,
                    log.route_prefix,
                    log.credential_id,
                    log.cb_key,
                    log.upstream_http_status,
                    log.upstream_error_preview,
                    log.dedupe_key,
                    log.client_transport,
                    log.request_headers,
                    log.request_body,
                    log.response_body,
                    log.client_response_body,
                    log.stream_kind,
                    log.stream_terminal_seen.map(i32::from),
                    log.stream_end_reason,
                    log.stream_error_detail,
                    log.upstream_first_byte_ms,
                    log.client_first_write_ms,
                    log.last_upstream_event_ms,
                    log.last_client_write_ms,
                    log.upstream_chunk_count,
                    log.upstream_bytes,
                    log.client_chunk_count,
                    log.client_bytes,
                    log.sse_event_count,
                    log.sse_data_count,
                    log.sse_comment_count,
                    log.sse_keepalive_count,
                    log.sse_done_count,
                    log.parse_error_count,
                    log.first_keepalive_ms,
                    log.last_keepalive_ms,
                    log.max_gap_between_upstream_events_ms,
                    log.max_gap_between_data_events_ms,
                    log.keepalive_after_last_data_count,
                    log.last_data_event_ms,
                    log.bridge_mode,
                    i32::from(log.status_injected),
                    i32::from(log.terminal_injected),
                    log.upstream_terminal_type,
                ],
            )?;
            Ok(())
        })
    }

    /// Attach gateway→client transform trace (e.g. Codex WS Responses events) after streaming ends.
    pub fn log_set_client_response_body(&self, id: &str, client_body: Option<&str>) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE request_logs SET client_response_body = ?1 WHERE id = ?2",
                params![client_body, id],
            )?;
            if n == 0 {
                anyhow::bail!("request_logs update: no row for id {id}");
            }
            Ok(())
        })
    }

    pub fn log_update_client_trace_and_stream_fields(&self, log: &RequestLog) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE request_logs SET
                    client_response_body = ?1,
                    stream_kind = ?2,
                    stream_terminal_seen = ?3,
                    stream_end_reason = ?4,
                    stream_error_detail = ?5,
                    upstream_first_byte_ms = COALESCE(upstream_first_byte_ms, ?6),
                    client_first_write_ms = ?7,
                    last_upstream_event_ms = COALESCE(last_upstream_event_ms, ?8),
                    last_client_write_ms = ?9,
                    upstream_chunk_count = CASE WHEN upstream_chunk_count > 0 THEN upstream_chunk_count ELSE ?10 END,
                    upstream_bytes = CASE WHEN upstream_bytes > 0 THEN upstream_bytes ELSE ?11 END,
                    client_chunk_count = ?12,
                    client_bytes = ?13,
                    sse_event_count = CASE WHEN sse_event_count > 0 THEN sse_event_count ELSE ?14 END,
                    sse_data_count = CASE WHEN sse_data_count > 0 THEN sse_data_count ELSE ?15 END,
                    sse_comment_count = CASE WHEN sse_comment_count > 0 THEN sse_comment_count ELSE ?16 END,
                    sse_keepalive_count = CASE WHEN sse_keepalive_count > 0 THEN sse_keepalive_count ELSE ?17 END,
                    sse_done_count = CASE WHEN sse_done_count > 0 THEN sse_done_count ELSE ?18 END,
                    parse_error_count = CASE WHEN parse_error_count > 0 THEN parse_error_count ELSE ?19 END,
                    first_keepalive_ms = COALESCE(first_keepalive_ms, ?20),
                    last_keepalive_ms = COALESCE(last_keepalive_ms, ?21),
                    max_gap_between_upstream_events_ms = COALESCE(max_gap_between_upstream_events_ms, ?22),
                    max_gap_between_data_events_ms = COALESCE(max_gap_between_data_events_ms, ?23),
                    keepalive_after_last_data_count = CASE WHEN keepalive_after_last_data_count > 0 THEN keepalive_after_last_data_count ELSE ?24 END,
                    last_data_event_ms = COALESCE(last_data_event_ms, ?25),
                    bridge_mode = ?26,
                    status_injected = ?27,
                    terminal_injected = ?28,
                    upstream_terminal_type = COALESCE(upstream_terminal_type, ?29)
                 WHERE id = ?30",
                params![
                    log.client_response_body,
                    log.stream_kind,
                    log.stream_terminal_seen.map(i32::from),
                    log.stream_end_reason,
                    log.stream_error_detail,
                    log.upstream_first_byte_ms,
                    log.client_first_write_ms,
                    log.last_upstream_event_ms,
                    log.last_client_write_ms,
                    log.upstream_chunk_count,
                    log.upstream_bytes,
                    log.client_chunk_count,
                    log.client_bytes,
                    log.sse_event_count,
                    log.sse_data_count,
                    log.sse_comment_count,
                    log.sse_keepalive_count,
                    log.sse_done_count,
                    log.parse_error_count,
                    log.first_keepalive_ms,
                    log.last_keepalive_ms,
                    log.max_gap_between_upstream_events_ms,
                    log.max_gap_between_data_events_ms,
                    log.keepalive_after_last_data_count,
                    log.last_data_event_ms,
                    log.bridge_mode,
                    i32::from(log.status_injected),
                    i32::from(log.terminal_injected),
                    log.upstream_terminal_type,
                    log.id,
                ],
            )?;
            if n == 0 {
                anyhow::bail!("request_logs update: no row for id {}", log.id);
            }
            Ok(())
        })
    }

    pub fn log_list(&self, limit: i64, offset: i64) -> Result<LogPage> {
        self.with(|c| {
            let fetch = limit + 1;
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM request_logs ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
                Self::LOG_COLS_LIST
            ))?;
            let rows = stmt.query_map(params![fetch, offset], row_to_log_list)?;
            let mut items = Vec::new();
            for r in rows {
                items.push(r?);
            }
            let has_more = items.len() > limit as usize;
            if has_more {
                items.pop();
            }
            let total = offset + items.len() as i64;
            Ok(LogPage {
                items,
                total,
                limit,
                offset,
                has_more,
            })
        })
    }

    pub fn usage_summary_last_hours(&self, hours: i64) -> Result<UsageSummary> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let (requests, input, output, cache_read, cache_create): (i64, i64, i64, i64, i64) = c
                .query_row(
                    "SELECT count(*),
                            COALESCE(sum(input_tokens), 0),
                            COALESCE(sum(output_tokens), 0),
                            COALESCE(sum(cache_read_tokens), 0),
                            COALESCE(sum(cache_creation_tokens), 0)
                     FROM request_logs WHERE started_at >= ?1",
                    params![since],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?;
            Ok(UsageSummary {
                range: format!("last_{hours}h"),
                requests,
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_creation_tokens: cache_create,
                estimated_cost_usd: "0".into(),
            })
        })
    }

    pub fn count_logs_since(&self, since: i64) -> Result<i64> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM request_logs WHERE started_at >= ?1",
                params![since],
                |r| r.get(0),
            )?;
            Ok(n)
        })
    }

    /// Returns `(ok_count, total_count)` where ok is HTTP 2xx.
    pub fn ok_total_since(&self, since: i64) -> Result<(i64, i64)> {
        self.with(|c| {
            let total: i64 = c.query_row(
                "SELECT count(*) FROM request_logs WHERE started_at >= ?1",
                params![since],
                |r| r.get(0),
            )?;
            let ok: i64 = c.query_row(
                "SELECT count(*) FROM request_logs WHERE started_at >= ?1
                   AND status_code >= 200 AND status_code < 300",
                params![since],
                |r| r.get(0),
            )?;
            Ok((ok, total))
        })
    }

    /// Filtered log list — supports optional provider_id, status bucket, model, time range.
    pub fn log_list_filtered(
        &self,
        limit: i64,
        offset: i64,
        since: Option<i64>,
        provider_id: Option<&str>,
        status_ok: Option<bool>,
    ) -> Result<LogPage> {
        self.with(|c| {
            // Build dynamic WHERE
            let mut conditions = Vec::<String>::new();
            if let Some(ts) = since {
                conditions.push(format!("started_at >= {ts}"));
            }
            if let Some(pid) = provider_id {
                conditions.push(format!("provider_id = '{pid}'"));
            }
            if let Some(ok) = status_ok {
                if ok {
                    conditions.push("status_code >= 200 AND status_code < 300".into());
                } else {
                    conditions.push("(status_code IS NULL OR status_code >= 400)".into());
                }
            }
            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };
            let fetch = limit + 1;
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM request_logs {where_clause}
                 ORDER BY started_at DESC LIMIT {fetch} OFFSET {offset}",
                Self::LOG_COLS_LIST
            ))?;
            let rows = stmt.query_map([], row_to_log_list)?;
            let mut items = Vec::new();
            for r in rows {
                items.push(r?);
            }
            let has_more = items.len() > limit as usize;
            if has_more {
                items.pop();
            }
            let total = offset + items.len() as i64;
            Ok(LogPage {
                items,
                total,
                limit,
                offset,
                has_more,
            })
        })
    }

    /// Full log row including optional raw bodies (may be large).
    pub fn log_get(&self, id: &str) -> Result<Option<RequestLog>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM request_logs WHERE id = ?1",
                Self::LOG_COLS_FULL
            ))?;
            let mut rows = stmt.query_map(params![id], row_to_log_detail)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn upstream_attempt_insert(&self, attempt: &UpstreamAttemptLog) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO upstream_attempt_logs (
                    attempt_id, request_id, attempt_index, started_at, ended_at,
                    provider_id, credential_id, wire, route_prefix, requested_model, upstream_model,
                    phase, outcome, status_code, upstream_http_status, error_summary,
                    latency_ms, first_token_ms, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens,
                    upstream_first_byte_ms, client_first_write_ms,
                    last_upstream_event_ms, last_client_write_ms,
                    upstream_chunk_count, upstream_bytes, client_chunk_count, client_bytes,
                    sse_event_count, sse_data_count, sse_comment_count, sse_keepalive_count,
                    sse_done_count, parse_error_count,
                    first_keepalive_ms, last_keepalive_ms,
                    max_gap_between_upstream_events_ms, max_gap_between_data_events_ms,
                    keepalive_after_last_data_count, last_data_event_ms,
                    bridge_mode, status_injected, terminal_injected, upstream_terminal_type,
                    active_upstream_decode_tps_peak, active_downstream_emit_tps_peak,
                    request_headers, request_body, response_headers, response_body
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9, ?10, ?11,
                    ?12, ?13, ?14, ?15, ?16,
                    ?17, ?18, ?19, ?20,
                    ?21, ?22,
                    ?23, ?24,
                    ?25, ?26,
                    ?27, ?28, ?29, ?30,
                    ?31, ?32, ?33, ?34,
                    ?35, ?36,
                    ?37, ?38,
                    ?39, ?40,
                    ?41, ?42,
                    ?43, ?44, ?45, ?46,
                    ?47, ?48, ?49, ?50, ?51, ?52
                 )",
                params![
                    attempt.attempt_id,
                    attempt.request_id,
                    attempt.attempt_index,
                    attempt.started_at,
                    attempt.ended_at,
                    attempt.provider_id,
                    attempt.credential_id,
                    attempt.wire,
                    attempt.route_prefix,
                    attempt.requested_model,
                    attempt.upstream_model,
                    upstream_attempt_phase_to_str(attempt.phase.clone()),
                    upstream_attempt_outcome_to_str(attempt.outcome.clone()),
                    attempt.status_code,
                    attempt.upstream_http_status,
                    attempt.error_summary,
                    attempt.latency_ms,
                    attempt.first_token_ms,
                    attempt.input_tokens,
                    attempt.output_tokens,
                    attempt.cache_read_tokens,
                    attempt.cache_creation_tokens,
                    attempt.upstream_first_byte_ms,
                    attempt.client_first_write_ms,
                    attempt.last_upstream_event_ms,
                    attempt.last_client_write_ms,
                    attempt.upstream_chunk_count,
                    attempt.upstream_bytes,
                    attempt.client_chunk_count,
                    attempt.client_bytes,
                    attempt.sse_event_count,
                    attempt.sse_data_count,
                    attempt.sse_comment_count,
                    attempt.sse_keepalive_count,
                    attempt.sse_done_count,
                    attempt.parse_error_count,
                    attempt.first_keepalive_ms,
                    attempt.last_keepalive_ms,
                    attempt.max_gap_between_upstream_events_ms,
                    attempt.max_gap_between_data_events_ms,
                    attempt.keepalive_after_last_data_count,
                    attempt.last_data_event_ms,
                    attempt.bridge_mode,
                    i32::from(attempt.status_injected),
                    i32::from(attempt.terminal_injected),
                    attempt.upstream_terminal_type,
                    attempt.active_upstream_decode_tps_peak,
                    attempt.active_downstream_emit_tps_peak,
                    attempt.request_headers,
                    attempt.request_body,
                    attempt.response_headers,
                    attempt.response_body,
                ],
            )?;
            Ok(())
        })
    }

    pub fn upstream_attempt_get(&self, attempt_id: &str) -> Result<Option<UpstreamAttemptLog>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM upstream_attempt_logs WHERE attempt_id = ?1",
                Self::ATTEMPT_COLS
            ))?;
            let mut rows = stmt.query_map(params![attempt_id], row_to_attempt)?;
            Ok(rows.next().transpose()?)
        })
    }

    pub fn upstream_attempts_for_request(
        &self,
        request_id: &str,
    ) -> Result<Vec<UpstreamAttemptLog>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM upstream_attempt_logs WHERE request_id = ?1 ORDER BY attempt_index ASC",
                Self::ATTEMPT_COLS
            ))?;
            let rows = stmt.query_map(params![request_id], row_to_attempt)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn upstream_attempt_list(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<UpstreamAttemptLog>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM upstream_attempt_logs ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
                Self::ATTEMPT_COLS
            ))?;
            let rows = stmt.query_map(params![limit, offset], row_to_attempt)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn app_log_insert(&self, ev: &AppLogEvent) -> Result<()> {
        let level = match ev.level {
            AppLogLevel::Debug => "debug",
            AppLogLevel::Info => "info",
            AppLogLevel::Warn => "warn",
            AppLogLevel::Error => "error",
        };
        self.with(|c| {
            c.execute(
                "INSERT INTO app_logs (ts, level, category, message, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![ev.ts, level, ev.category, ev.message, ev.detail],
            )?;
            Ok(())
        })
    }

    pub fn app_log_list(&self, limit: i64, since: Option<i64>) -> Result<Vec<AppLogEvent>> {
        self.with(|c| {
            let since_clause = since
                .map(|t| format!("WHERE ts >= {t}"))
                .unwrap_or_default();
            let mut stmt = c.prepare(&format!(
                "SELECT ts, level, category, message, detail FROM app_logs {since_clause}
                 ORDER BY ts DESC, id DESC LIMIT ?1"
            ))?;
            let rows = stmt.query_map(params![limit], |r| {
                let level_str: String = r.get(1)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    level_str,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (ts, level_str, category, message, detail) = row?;
                let level = match level_str.as_str() {
                    "warn" => AppLogLevel::Warn,
                    "error" => AppLogLevel::Error,
                    "debug" => AppLogLevel::Debug,
                    _ => AppLogLevel::Info,
                };
                out.push(AppLogEvent {
                    ts,
                    level,
                    category,
                    message,
                    detail,
                });
            }
            Ok(out)
        })
    }

    /// Per-model request count and token totals for the last N hours.
    pub fn top_models(&self, hours: i64, limit: i64) -> Result<Vec<vibe_protocol::ModelStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT COALESCE(NULLIF(TRIM(upstream_model), ''), NULLIF(TRIM(requested_model), ''), 'unknown') as model,
                        count(*) as reqs,
                        COALESCE(sum(input_tokens), 0),
                        COALESCE(sum(output_tokens), 0)
                 FROM request_logs WHERE started_at >= ?1
                 GROUP BY model ORDER BY reqs DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![since, limit], |r| {
                Ok(vibe_protocol::ModelStat {
                    model: r.get(0)?,
                    requests: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    /// Per-provider aggregated stats for the last N hours.
    pub fn per_provider_stats(&self, hours: i64) -> Result<Vec<vibe_protocol::ProviderStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.provider_id,
                        COALESCE(p.name, l.provider_id, 'unknown') as name,
                        count(*) as total,
                        sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as ok,
                        sum(CASE WHEN l.status_code IS NULL OR l.status_code >= 400 THEN 1 ELSE 0 END) as err,
                        COALESCE(avg(l.latency_ms), 0) as avg_lat,
                        COALESCE(sum(l.input_tokens), 0),
                        COALESCE(sum(l.output_tokens), 0),
                        COALESCE(sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 AND l.latency_ms > 0 THEN l.latency_ms ELSE 0 END), 0) as ok_sum_latency_ms,
                        COALESCE(sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 AND l.latency_ms IS NOT NULL AND l.first_token_ms IS NOT NULL AND l.latency_ms > l.first_token_ms THEN l.output_tokens ELSE 0 END), 0) as ok_decode_out_tokens,
                        COALESCE(sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 AND l.latency_ms IS NOT NULL AND l.first_token_ms IS NOT NULL AND l.latency_ms > l.first_token_ms THEN (l.latency_ms - l.first_token_ms) ELSE 0 END), 0) as ok_sum_decode_ms,
                        sum(CASE WHEN l.status_code = 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code = 503 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code BETWEEN 400 AND 499 AND l.status_code != 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code >= 500 AND l.status_code != 503 THEN 1 ELSE 0 END)
                 FROM request_logs l
                 LEFT JOIN providers p ON p.id = l.provider_id
                 WHERE l.started_at >= ?1 AND l.provider_id IS NOT NULL
                 GROUP BY l.provider_id",
            )?;
            let rows = stmt.query_map(params![since], |r| {
                let total: i64 = r.get(2)?;
                let ok: i64 = r.get(3)?;
                let err: i64 = r.get(4)?;
                let avg_lat: f64 = r.get(5)?;
                let ok_sum_latency_ms: i64 = r.get(8)?;
                let ok_decode_out_tokens: i64 = r.get(9)?;
                let ok_sum_decode_ms: i64 = r.get(10)?;
                let output_tokens: i64 = r.get(7)?;
                Ok(vibe_protocol::ProviderStat {
                    provider_id: r.get(0)?,
                    provider_name: r.get(1)?,
                    requests: total,
                    successes: ok,
                    failures: err,
                    success_rate: if total > 0 { ok as f64 / total as f64 } else { 1.0 },
                    avg_latency_ms: avg_lat as i64,
                    input_tokens: r.get(6)?,
                    output_tokens,
                    output_tokens_per_sec: if ok_sum_latency_ms > 0 {
                        output_tokens as f64 * 1000.0 / ok_sum_latency_ms as f64
                    } else {
                        0.0
                    },
                    decode_output_tokens_per_sec: if ok_sum_decode_ms > 0 {
                        ok_decode_out_tokens as f64 * 1000.0 / ok_sum_decode_ms as f64
                    } else {
                        0.0
                    },
                    err_429: r.get(11)?,
                    err_503: r.get(12)?,
                    err_4xx_other: r.get(13)?,
                    err_5xx_other: r.get(14)?,
                })
            })?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    /// Rolling-window stats for a single provider (gateway `request_logs`, not upstream Plan quota).
    pub fn provider_stat_single(
        &self,
        provider_id: &str,
        hours: i64,
    ) -> Result<Option<vibe_protocol::ProviderStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.provider_id,
                        COALESCE(p.name, l.provider_id, 'unknown') as name,
                        count(*) as total,
                        sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as ok,
                        sum(CASE WHEN l.status_code IS NULL OR l.status_code >= 400 THEN 1 ELSE 0 END) as err,
                        COALESCE(avg(l.latency_ms), 0) as avg_lat,
                        COALESCE(sum(l.input_tokens), 0),
                        COALESCE(sum(l.output_tokens), 0),
                        COALESCE(sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 AND l.latency_ms > 0 THEN l.latency_ms ELSE 0 END), 0) as ok_sum_latency_ms,
                        COALESCE(sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 AND l.latency_ms IS NOT NULL AND l.first_token_ms IS NOT NULL AND l.latency_ms > l.first_token_ms THEN l.output_tokens ELSE 0 END), 0) as ok_decode_out_tokens,
                        COALESCE(sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 AND l.latency_ms IS NOT NULL AND l.first_token_ms IS NOT NULL AND l.latency_ms > l.first_token_ms THEN (l.latency_ms - l.first_token_ms) ELSE 0 END), 0) as ok_sum_decode_ms,
                        sum(CASE WHEN l.status_code = 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code = 503 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code BETWEEN 400 AND 499 AND l.status_code != 429 THEN 1 ELSE 0 END),
                        sum(CASE WHEN l.status_code >= 500 AND l.status_code != 503 THEN 1 ELSE 0 END)
                 FROM request_logs l
                 LEFT JOIN providers p ON p.id = l.provider_id
                 WHERE l.started_at >= ?1 AND l.provider_id = ?2
                 GROUP BY l.provider_id",
            )?;
            let mut rows = stmt.query_map(params![since, provider_id], |r| {
                let total: i64 = r.get(2)?;
                let ok: i64 = r.get(3)?;
                let err: i64 = r.get(4)?;
                let avg_lat: f64 = r.get(5)?;
                let ok_sum_latency_ms: i64 = r.get(8)?;
                let ok_decode_out_tokens: i64 = r.get(9)?;
                let ok_sum_decode_ms: i64 = r.get(10)?;
                let output_tokens: i64 = r.get(7)?;
                Ok(vibe_protocol::ProviderStat {
                    provider_id: r.get(0)?,
                    provider_name: r.get(1)?,
                    requests: total,
                    successes: ok,
                    failures: err,
                    success_rate: if total > 0 { ok as f64 / total as f64 } else { 1.0 },
                    avg_latency_ms: avg_lat as i64,
                    input_tokens: r.get(6)?,
                    output_tokens,
                    output_tokens_per_sec: if ok_sum_latency_ms > 0 {
                        output_tokens as f64 * 1000.0 / ok_sum_latency_ms as f64
                    } else {
                        0.0
                    },
                    decode_output_tokens_per_sec: if ok_sum_decode_ms > 0 {
                        ok_decode_out_tokens as f64 * 1000.0 / ok_sum_decode_ms as f64
                    } else {
                        0.0
                    },
                    err_429: r.get(11)?,
                    err_503: r.get(12)?,
                    err_4xx_other: r.get(13)?,
                    err_5xx_other: r.get(14)?,
                })
            })?;
            Ok(rows.next().transpose()?)
        })
    }

    /// Rolling-window output speed (tokens/sec) across successful requests with latency.
    pub fn output_tokens_per_sec(&self, hours: i64) -> Result<f64> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT
                    COALESCE(sum(output_tokens), 0) as out_tokens,
                    COALESCE(sum(CASE WHEN latency_ms > 0 THEN latency_ms ELSE 0 END), 0) as sum_latency_ms
                 FROM request_logs
                 WHERE started_at >= ?1
                   AND status_code >= 200 AND status_code < 300",
            )?;
            let (out_tokens, sum_latency_ms): (i64, i64) =
                stmt.query_row(params![since], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if sum_latency_ms <= 0 {
                return Ok(0.0);
            }
            Ok(out_tokens as f64 * 1000.0 / sum_latency_ms as f64)
        })
    }

    /// Rolling-window decode-phase output speed: tokens per wall second after first token.
    pub fn decode_output_tokens_per_sec(&self, hours: i64) -> Result<f64> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT
                    COALESCE(sum(CASE WHEN status_code >= 200 AND status_code < 300 AND latency_ms IS NOT NULL AND first_token_ms IS NOT NULL AND latency_ms > first_token_ms THEN output_tokens ELSE 0 END), 0) as out_tokens,
                    COALESCE(sum(CASE WHEN status_code >= 200 AND status_code < 300 AND latency_ms IS NOT NULL AND first_token_ms IS NOT NULL AND latency_ms > first_token_ms THEN (latency_ms - first_token_ms) ELSE 0 END), 0) as sum_decode_ms
                 FROM request_logs
                 WHERE started_at >= ?1",
            )?;
            let (out_tokens, sum_decode_ms): (i64, i64) =
                stmt.query_row(params![since], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if sum_decode_ms <= 0 {
                return Ok(0.0);
            }
            Ok(out_tokens as f64 * 1000.0 / sum_decode_ms as f64)
        })
    }

    /// Rolling-window stats grouped by credential for one provider.
    pub fn credential_stats_for_provider(
        &self,
        provider_id: &str,
        hours: i64,
    ) -> Result<Vec<CredentialRollingStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.credential_id,
                        count(*) as total,
                        sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as ok,
                        sum(CASE WHEN l.status_code IS NULL OR l.status_code >= 400 THEN 1 ELSE 0 END) as err,
                        COALESCE(avg(l.latency_ms), 0) as avg_lat
                 FROM request_logs l
                 WHERE l.started_at >= ?1
                   AND l.provider_id = ?2
                   AND l.credential_id IS NOT NULL
                 GROUP BY l.credential_id",
            )?;
            let rows = stmt.query_map(params![since, provider_id], |r| {
                let avg_lat: f64 = r.get(4)?;
                Ok(CredentialRollingStat {
                    credential_id: r.get(0)?,
                    requests: r.get(1)?,
                    successes: r.get(2)?,
                    failures: r.get(3)?,
                    avg_latency_ms: Some(avg_lat as i64),
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    /// Rolling-window stats grouped by credential across all providers.
    pub fn credential_stats_all(&self, hours: i64) -> Result<Vec<CredentialRollingStat>> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.credential_id,
                        count(*) as total,
                        sum(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END) as ok,
                        sum(CASE WHEN l.status_code IS NULL OR l.status_code >= 400 THEN 1 ELSE 0 END) as err,
                        COALESCE(avg(l.latency_ms), 0) as avg_lat
                 FROM request_logs l
                 WHERE l.started_at >= ?1
                   AND l.credential_id IS NOT NULL
                 GROUP BY l.credential_id",
            )?;
            let rows = stmt.query_map(params![since], |r| {
                let avg_lat: f64 = r.get(4)?;
                Ok(CredentialRollingStat {
                    credential_id: r.get(0)?,
                    requests: r.get(1)?,
                    successes: r.get(2)?,
                    failures: r.get(3)?,
                    avg_latency_ms: Some(avg_lat as i64),
                })
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn plan_snapshot_insert(&self, snap: &vibe_protocol::CredentialPlanSnapshot) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO credential_plan_snapshots (
                    id, credential_id, captured_at,
                    codex_5h_used_percent, codex_7d_used_percent,
                    codex_5h_reset_after_seconds, codex_7d_reset_after_seconds,
                    codex_primary_used_percent, codex_secondary_used_percent,
                    summary, source
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    snap.id,
                    snap.credential_id,
                    snap.captured_at,
                    snap.codex_5h_used_percent,
                    snap.codex_7d_used_percent,
                    snap.codex_5h_reset_after_seconds,
                    snap.codex_7d_reset_after_seconds,
                    snap.codex_primary_used_percent,
                    snap.codex_secondary_used_percent,
                    snap.summary,
                    snap.source,
                ],
            )?;
            Ok(())
        })
    }

    pub fn plan_snapshot_latest(
        &self,
        credential_id: &str,
    ) -> Result<Option<vibe_protocol::CredentialPlanSnapshot>> {
        self.with(|c| {
            let snap = c
                .query_row(
                    "SELECT id, credential_id, captured_at,
                            codex_5h_used_percent, codex_7d_used_percent,
                            codex_5h_reset_after_seconds, codex_7d_reset_after_seconds,
                            codex_primary_used_percent, codex_secondary_used_percent,
                            summary, source
                     FROM credential_plan_snapshots
                     WHERE credential_id = ?1
                     ORDER BY captured_at DESC LIMIT 1",
                    params![credential_id],
                    row_to_plan_snapshot,
                )
                .optional()?;
            Ok(snap)
        })
    }

    /// Latest plan snapshot per credential id (batch). Missing ids are omitted from the map.
    pub fn plan_snapshot_latest_map(
        &self,
        credential_ids: &[String],
    ) -> Result<HashMap<String, vibe_protocol::CredentialPlanSnapshot>> {
        if credential_ids.is_empty() {
            return Ok(HashMap::new());
        }
        self.with(|c| {
            let mut out = HashMap::with_capacity(credential_ids.len());
            for id in credential_ids {
                let snap = c
                    .query_row(
                        "SELECT id, credential_id, captured_at,
                                codex_5h_used_percent, codex_7d_used_percent,
                                codex_5h_reset_after_seconds, codex_7d_reset_after_seconds,
                                codex_primary_used_percent, codex_secondary_used_percent,
                                summary, source
                         FROM credential_plan_snapshots
                         WHERE credential_id = ?1
                         ORDER BY captured_at DESC LIMIT 1",
                        params![id],
                        row_to_plan_snapshot,
                    )
                    .optional()?;
                if let Some(s) = snap {
                    out.insert(id.clone(), s);
                }
            }
            Ok(out)
        })
    }

    pub fn credential_get_by_provider_and_fingerprint(
        &self,
        provider_id: &str,
        fingerprint: &str,
    ) -> Result<Option<vibe_protocol::Credential>> {
        self.with(|c| {
            let q = format!(
                "SELECT {} FROM credentials WHERE provider_id = ?1 AND auth_fingerprint = ?2 LIMIT 1",
                Self::CRED_COLS
            );
            let r = c
                .query_row(&q, params![provider_id, fingerprint], row_to_credential)
                .optional()?;
            Ok(r)
        })
    }

    pub fn credential_count_same_fingerprint(
        &self,
        fingerprint: &str,
        exclude_id: Option<&str>,
    ) -> Result<i64> {
        self.with(|c| {
            let n = if let Some(ex) = exclude_id {
                c.query_row(
                    "SELECT count(*) FROM credentials WHERE auth_fingerprint = ?1 AND id != ?2",
                    params![fingerprint, ex],
                    |r| r.get::<_, i64>(0),
                )?
            } else {
                c.query_row(
                    "SELECT count(*) FROM credentials WHERE auth_fingerprint = ?1",
                    params![fingerprint],
                    |r| r.get::<_, i64>(0),
                )?
            };
            Ok(n)
        })
    }

    /// Whether this provider already has a credential with the same import fingerprint.
    pub fn credential_has_fingerprint_for_provider(
        &self,
        provider_id: &str,
        fingerprint: &str,
    ) -> Result<bool> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT count(*) FROM credentials WHERE provider_id = ?1 AND auth_fingerprint = ?2",
                params![provider_id, fingerprint],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    pub fn latency_percentiles(&self, hours: i64) -> Result<(i64, i64)> {
        let since = now_secs() - hours * 3600;
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT latency_ms FROM request_logs
                 WHERE started_at >= ?1 AND latency_ms IS NOT NULL
                 ORDER BY latency_ms",
            )?;
            let vals: Vec<i64> = stmt
                .query_map(params![since], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            if vals.is_empty() {
                return Ok((0, 0));
            }
            let p50 = vals[vals.len() / 2];
            let p95 = vals[(vals.len() as f64 * 0.95) as usize].min(*vals.last().unwrap());
            Ok((p50, p95))
        })
    }

    // --- provider health -----------------------------------------------------

    pub fn health_upsert(
        &self,
        provider_id: &str,
        success: bool,
        latency_ms: Option<i64>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "INSERT INTO provider_health (
                     provider_id, is_healthy, consecutive_failures,
                     total_requests, total_successes, total_failures,
                     last_success_at, last_failure_at, last_error, avg_latency_ms, updated_at
                 ) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(provider_id) DO UPDATE SET
                     total_requests = total_requests + 1,
                     total_successes = total_successes + excluded.total_successes,
                     total_failures  = total_failures  + excluded.total_failures,
                     consecutive_failures = CASE
                         WHEN excluded.total_successes > 0 THEN 0
                         ELSE consecutive_failures + 1
                     END,
                     is_healthy = excluded.is_healthy,
                     last_success_at = COALESCE(excluded.last_success_at, last_success_at),
                     last_failure_at = COALESCE(excluded.last_failure_at, last_failure_at),
                     last_error = COALESCE(excluded.last_error, last_error),
                     avg_latency_ms = CASE
                         WHEN excluded.avg_latency_ms IS NOT NULL THEN
                             COALESCE((avg_latency_ms * 9 + excluded.avg_latency_ms) / 10, excluded.avg_latency_ms)
                         ELSE avg_latency_ms
                     END,
                     updated_at = excluded.updated_at",
                params![
                    provider_id,
                    success as i64,
                    if success { 0i64 } else { 1i64 },
                    if success { 1i64 } else { 0i64 },
                    if success { 0i64 } else { 1i64 },
                    if success { Some(now) } else { None::<i64> },
                    if success { None::<i64> } else { Some(now) },
                    error,
                    latency_ms,
                    now,
                ],
            )?;
            Ok(())
        })
    }

    pub fn health_get(&self, provider_id: &str) -> Result<Option<DbHealth>> {
        self.with(|c| {
            let r = c
                .query_row(
                    "SELECT provider_id, is_healthy, consecutive_failures,
                        total_requests, total_successes, total_failures,
                        last_success_at, last_failure_at, last_error, avg_latency_ms, updated_at
                 FROM provider_health WHERE provider_id = ?1",
                    params![provider_id],
                    row_to_health,
                )
                .optional()?;
            Ok(r)
        })
    }

    pub fn health_list(&self) -> Result<Vec<DbHealth>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT provider_id, is_healthy, consecutive_failures,
                        total_requests, total_successes, total_failures,
                        last_success_at, last_failure_at, last_error, avg_latency_ms, updated_at
                 FROM provider_health",
            )?;
            let rows = stmt.query_map([], row_to_health)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    // --- credentials --------------------------------------------------------

    const CRED_COLS: &'static str = "id, provider_id, label, auth_ref, plan_type, notes,
         enabled, priority,
         rl_requests_limit, rl_requests_remaining, rl_requests_reset_at,
         rl_tokens_limit, rl_tokens_remaining, rl_tokens_reset_at,
         last_used_at, last_error, consecutive_failures, created_at, updated_at,
         oauth_access_token, oauth_refresh_token, oauth_expires_at, auth_fingerprint,
         oauth_cached_email, oauth_cached_subject, oauth_cached_plan_slug,
         remote_models_json, remote_models_fetched_at, balance_json, usage_json, balance_fetched_at,
         upstream_vendor, upstream_username, upstream_session, upstream_session_expires_at,
         upstream_group, price_multiplier, windows_json";
    // Col indices (0-based):
    // 0  id, 1  provider_id, 2  label, 3  auth_ref, 4  plan_type, 5  notes,
    // 6  enabled, 7  priority,
    // 8  rl_requests_limit, 9  rl_requests_remaining, 10 rl_requests_reset_at,
    // 11 rl_tokens_limit, 12 rl_tokens_remaining, 13 rl_tokens_reset_at,
    // 14 last_used_at, 15 last_error, 16 consecutive_failures,
    // 17 created_at, 18 updated_at,
    // 19 oauth_access_token, 20 oauth_refresh_token, 21 oauth_expires_at, 22 auth_fingerprint,
    // 23 oauth_cached_email, 24 oauth_cached_subject, 25 oauth_cached_plan_slug,
    // 26 remote_models_json, 27 remote_models_fetched_at,
    // 28 balance_json, 29 usage_json, 30 balance_fetched_at,
    // 31 upstream_vendor, 32 upstream_username, 33 upstream_session,
    // 34 upstream_session_expires_at, 35 upstream_group, 36 price_multiplier, 37 windows_json

    const LOG_COLS_LIST: &'static str =
        "id, started_at, app, provider_id, requested_model, upstream_model,
         status_code, error, latency_ms, first_token_ms,
         input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
         estimated_cost_usd,
         wire, route_prefix, credential_id, cb_key, upstream_http_status,
         upstream_error_preview, dedupe_key,
         client_transport, request_headers,
         stream_kind, stream_terminal_seen, stream_end_reason, stream_error_detail,
         upstream_first_byte_ms, client_first_write_ms,
         last_upstream_event_ms, last_client_write_ms,
         upstream_chunk_count, upstream_bytes, client_chunk_count, client_bytes,
         sse_event_count, sse_data_count, sse_comment_count, sse_keepalive_count,
         sse_done_count, parse_error_count,
         first_keepalive_ms, last_keepalive_ms,
         max_gap_between_upstream_events_ms, max_gap_between_data_events_ms,
         keepalive_after_last_data_count, last_data_event_ms,
         bridge_mode, status_injected, terminal_injected, upstream_terminal_type";

    const LOG_COLS_FULL: &'static str =
        "id, started_at, app, provider_id, requested_model, upstream_model,
         status_code, error, latency_ms, first_token_ms,
         input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
         estimated_cost_usd,
         wire, route_prefix, credential_id, cb_key, upstream_http_status,
         upstream_error_preview, dedupe_key,
         client_transport, request_headers,
         request_body, response_body, client_response_body,
         stream_kind, stream_terminal_seen, stream_end_reason, stream_error_detail,
         upstream_first_byte_ms, client_first_write_ms,
         last_upstream_event_ms, last_client_write_ms,
         upstream_chunk_count, upstream_bytes, client_chunk_count, client_bytes,
         sse_event_count, sse_data_count, sse_comment_count, sse_keepalive_count,
         sse_done_count, parse_error_count,
         first_keepalive_ms, last_keepalive_ms,
         max_gap_between_upstream_events_ms, max_gap_between_data_events_ms,
         keepalive_after_last_data_count, last_data_event_ms,
         bridge_mode, status_injected, terminal_injected, upstream_terminal_type";

    const ATTEMPT_COLS: &'static str =
        "attempt_id, request_id, attempt_index, started_at, ended_at,
         provider_id, credential_id, wire, route_prefix, requested_model, upstream_model,
         phase, outcome, status_code, upstream_http_status, error_summary,
         latency_ms, first_token_ms, input_tokens, output_tokens,
         cache_read_tokens, cache_creation_tokens,
         upstream_first_byte_ms, client_first_write_ms,
         last_upstream_event_ms, last_client_write_ms,
         upstream_chunk_count, upstream_bytes, client_chunk_count, client_bytes,
         sse_event_count, sse_data_count, sse_comment_count, sse_keepalive_count,
         sse_done_count, parse_error_count,
         first_keepalive_ms, last_keepalive_ms,
         max_gap_between_upstream_events_ms, max_gap_between_data_events_ms,
         keepalive_after_last_data_count, last_data_event_ms,
         bridge_mode, status_injected, terminal_injected, upstream_terminal_type,
         active_upstream_decode_tps_peak, active_downstream_emit_tps_peak,
         request_headers, request_body, response_headers, response_body";

    pub fn credential_list_for_provider(
        &self,
        provider_id: &str,
    ) -> Result<Vec<vibe_protocol::Credential>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM credentials WHERE provider_id = ?1 ORDER BY priority ASC, created_at ASC",
                Self::CRED_COLS,
            ))?;
            let rows = stmt.query_map(params![provider_id], row_to_credential)?;
            let mut out = Vec::new();
            for r in rows { out.push(r?); }
            Ok(out)
        })
    }

    pub fn credential_list_all(&self) -> Result<Vec<vibe_protocol::Credential>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM credentials ORDER BY provider_id, priority ASC, created_at ASC",
                Self::CRED_COLS,
            ))?;
            let rows = stmt.query_map([], row_to_credential)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            Ok(out)
        })
    }

    pub fn credential_get(&self, id: &str) -> Result<Option<vibe_protocol::Credential>> {
        self.with(|c| {
            let r = c
                .query_row(
                    &format!("SELECT {} FROM credentials WHERE id = ?1", Self::CRED_COLS),
                    params![id],
                    row_to_credential,
                )
                .optional()?;
            Ok(r)
        })
    }

    pub fn credential_insert(
        &self,
        provider_id: &str,
        input: vibe_protocol::CredentialInput,
        auth_fingerprint: Option<String>,
    ) -> Result<vibe_protocol::Credential> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_secs();
        let vendor_str = input.upstream_vendor.as_ref().map(|v| {
            serde_json::to_string(v)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string()
        });
        self.with(|c| {
            c.execute(
                "INSERT INTO credentials
                    (id, provider_id, label, auth_ref, plan_type, notes, enabled, priority,
                     oauth_access_token, oauth_refresh_token, oauth_expires_at,
                     auth_fingerprint,
                     oauth_cached_email, oauth_cached_subject, oauth_cached_plan_slug,
                     upstream_vendor, upstream_username, upstream_session,
                     upstream_session_expires_at, upstream_group, price_multiplier,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                params![
                    id,
                    provider_id,
                    input.label,
                    input.auth_ref,
                    input.plan_type,
                    input.notes,
                    input.enabled as i32,
                    input.priority,
                    input.oauth_access_token,
                    input.oauth_refresh_token,
                    input.oauth_expires_at,
                    auth_fingerprint,
                    input.oauth_cached_email,
                    input.oauth_cached_subject,
                    input.oauth_cached_plan_slug,
                    vendor_str,
                    input.upstream_username,
                    input.upstream_session,
                    input.upstream_session_expires_at,
                    input.upstream_group,
                    input.price_multiplier,
                    now,
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.credential_get(&id)?
            .context("inserted credential missing on read-back")
    }

    pub fn credential_update(
        &self,
        id: &str,
        input: vibe_protocol::CredentialInput,
        auth_fingerprint: Option<String>,
    ) -> Result<vibe_protocol::Credential> {
        let now = now_secs();
        // oauth_refresh_token is write-only: only update it when the caller provides a value.
        let vendor_str = input.upstream_vendor.as_ref().map(|v| {
            serde_json::to_string(v)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string()
        });
        let n = self.with(|c| {
            Ok(c.execute(
                "UPDATE credentials
                 SET label=?2, auth_ref=?3, plan_type=?4, notes=?5,
                     enabled=?6, priority=?7,
                     oauth_access_token=?8,
                     oauth_refresh_token=COALESCE(?9, oauth_refresh_token),
                     oauth_expires_at=?10,
                     auth_fingerprint=?11,
                     oauth_cached_email=COALESCE(?12, oauth_cached_email),
                     oauth_cached_subject=COALESCE(?13, oauth_cached_subject),
                     oauth_cached_plan_slug=COALESCE(?14, oauth_cached_plan_slug),
                     upstream_vendor=COALESCE(?16, upstream_vendor),
                     upstream_username=COALESCE(?17, upstream_username),
                     upstream_session=COALESCE(?18, upstream_session),
                     upstream_session_expires_at=COALESCE(?19, upstream_session_expires_at),
                     upstream_group=COALESCE(?20, upstream_group),
                     price_multiplier=?21,
                     updated_at=?15
                 WHERE id=?1",
                params![
                    id,
                    input.label,
                    input.auth_ref,
                    input.plan_type,
                    input.notes,
                    input.enabled as i32,
                    input.priority,
                    input.oauth_access_token,
                    input.oauth_refresh_token,
                    input.oauth_expires_at,
                    auth_fingerprint,
                    input.oauth_cached_email,
                    input.oauth_cached_subject,
                    input.oauth_cached_plan_slug,
                    now,
                    vendor_str,
                    input.upstream_username,
                    input.upstream_session,
                    input.upstream_session_expires_at,
                    input.upstream_group,
                    input.price_multiplier,
                ],
            )?)
        })?;
        if n == 0 {
            anyhow::bail!("credential {id} not found");
        }
        self.credential_get(id)?
            .context("updated credential missing on read-back")
    }

    /// Read the upstream_session token (write-sensitive, not included in Credential).
    pub fn credential_get_session(&self, id: &str) -> Result<Option<String>> {
        self.with(|c| {
            let v = c
                .query_row(
                    "SELECT upstream_session FROM credentials WHERE id = ?1",
                    params![id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?;
            Ok(v.flatten())
        })
    }

    /// Fetch only the refresh_token for a credential (write-only field, not in Credential).
    pub fn credential_get_refresh_token(&self, id: &str) -> Result<Option<String>> {
        self.with(|c| {
            let v = c
                .query_row(
                    "SELECT oauth_refresh_token FROM credentials WHERE id = ?1",
                    params![id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?;
            Ok(v.flatten())
        })
    }

    /// Called after a successful OAuth token refresh: persists fresh tokens.
    pub fn credential_update_remote_models(
        &self,
        id: &str,
        remote_models: Vec<String>,
        fetched_at: i64,
    ) -> Result<vibe_protocol::Credential> {
        let models_json = serde_json::to_string(&remote_models)?;
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET remote_models_json = ?2, remote_models_fetched_at = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, models_json, fetched_at, now],
            )?;
            Ok(())
        })?;
        self.credential_get(id)?
            .context("credential missing after remote_models update")
    }

    /// Set upstream_vendor on all credentials belonging to a provider (auto-detect result).
    pub fn credentials_set_vendor_for_provider(
        &self,
        provider_id: &str,
        vendor: &str,
    ) -> Result<usize> {
        let now = now_secs();
        self.with(|c| {
            Ok(c.execute(
                "UPDATE credentials SET upstream_vendor = ?1, updated_at = ?2 WHERE provider_id = ?3 AND (upstream_vendor IS NULL OR upstream_vendor = '')",
                params![vendor, now, provider_id],
            )?)
        })
    }

    /// Store a new upstream session token (login result).
    pub fn credential_update_session(
        &self,
        id: &str,
        session: &str,
        expires_at: Option<i64>,
    ) -> Result<vibe_protocol::Credential> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET upstream_session=?2, upstream_session_expires_at=?3, updated_at=?4 WHERE id=?1",
                params![id, session, expires_at, now],
            )?;
            Ok(())
        })?;
        self.credential_get(id)?
            .context("credential missing after session update")
    }

    /// Store rolling-window usage snapshots fetched from the upstream platform.
    pub fn credential_update_windows(
        &self,
        id: &str,
        windows: &[vibe_protocol::UsageWindow],
    ) -> Result<vibe_protocol::Credential> {
        let windows_json = serde_json::to_string(windows)?;
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET windows_json=?2, balance_fetched_at=?3, updated_at=?3 WHERE id=?1",
                params![id, windows_json, now],
            )?;
            Ok(())
        })?;
        self.credential_get(id)?
            .context("credential missing after windows update")
    }

    pub fn credential_update_financials(
        &self,
        id: &str,
        balance: Option<ProviderBalanceSnapshot>,
        usage: Option<ProviderBalanceSnapshot>,
        fetched_at: i64,
    ) -> Result<vibe_protocol::Credential> {
        let balance_json = balance.as_ref().map(serde_json::to_string).transpose()?;
        let usage_json = usage.as_ref().map(serde_json::to_string).transpose()?;
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET balance_json = ?2, usage_json = ?3, balance_fetched_at = ?4, updated_at = ?5 WHERE id = ?1",
                params![id, balance_json, usage_json, fetched_at, now],
            )?;
            Ok(())
        })?;
        self.credential_get(id)?
            .context("credential missing after financials update")
    }

    pub fn credential_update_oauth_tokens(
        &self,
        id: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: Option<i64>,
    ) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials
                 SET oauth_access_token=?2, oauth_refresh_token=?3,
                     oauth_expires_at=?4, updated_at=?5
                 WHERE id=?1",
                params![id, access_token, refresh_token, expires_at, now],
            )?;
            Ok(())
        })
    }

    pub fn credential_delete(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM credentials WHERE id=?1", params![id])?;
            if n == 0 {
                anyhow::bail!("credential {id} not found");
            }
            Ok(())
        })
    }

    pub fn credential_set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<vibe_protocol::Credential> {
        let now = now_secs();
        self.with(|c| {
            let n = c.execute(
                "UPDATE credentials SET enabled=?2, updated_at=?3 WHERE id=?1",
                params![id, enabled as i32, now],
            )?;
            if n == 0 {
                anyhow::bail!("credential {id} not found");
            }
            Ok(())
        })?;
        self.credential_get(id)?
            .context("updated credential missing on read-back")
    }

    /// Update rate-limit counters extracted from upstream response headers.
    #[allow(clippy::too_many_arguments)]
    pub fn credential_update_rate_limit(
        &self,
        id: &str,
        rl_requests_limit: Option<i64>,
        rl_requests_remaining: Option<i64>,
        rl_requests_reset_at: Option<i64>,
        rl_tokens_limit: Option<i64>,
        rl_tokens_remaining: Option<i64>,
        rl_tokens_reset_at: Option<i64>,
    ) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET
                     rl_requests_limit      = COALESCE(?2, rl_requests_limit),
                     rl_requests_remaining  = COALESCE(?3, rl_requests_remaining),
                     rl_requests_reset_at   = COALESCE(?4, rl_requests_reset_at),
                     rl_tokens_limit        = COALESCE(?5, rl_tokens_limit),
                     rl_tokens_remaining    = COALESCE(?6, rl_tokens_remaining),
                     rl_tokens_reset_at     = COALESCE(?7, rl_tokens_reset_at),
                     updated_at             = ?8
                 WHERE id = ?1",
                params![
                    id,
                    rl_requests_limit,
                    rl_requests_remaining,
                    rl_requests_reset_at,
                    rl_tokens_limit,
                    rl_tokens_remaining,
                    rl_tokens_reset_at,
                    now
                ],
            )?;
            Ok(())
        })
    }

    /// Record a successful use: update last_used_at, clear consecutive_failures and last_error.
    pub fn credential_record_success(&self, id: &str) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET last_used_at=?2, consecutive_failures=0, last_error=NULL, updated_at=?2 WHERE id=?1",
                params![id, now],
            )?;
            Ok(())
        })
    }

    /// Record a failed use: increment consecutive_failures, store error message.
    pub fn credential_record_failure(&self, id: &str, error: Option<&str>) -> Result<()> {
        let now = now_secs();
        self.with(|c| {
            c.execute(
                "UPDATE credentials SET last_used_at=?2, consecutive_failures=consecutive_failures+1,
                     last_error=COALESCE(?3, last_error), updated_at=?2
                 WHERE id=?1",
                params![id, now, error],
            )?;
            Ok(())
        })
    }
}

/// Raw health row from SQLite — circuit_state is added by the core layer.
#[derive(Debug, Clone)]
pub struct DbHealth {
    pub provider_id: String,
    pub is_healthy: bool,
    pub consecutive_failures: i32,
    pub total_requests: i64,
    pub total_successes: i64,
    pub total_failures: i64,
    pub last_success_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_error: Option<String>,
    pub avg_latency_ms: Option<i64>,
    pub updated_at: i64,
}

// --- row -> struct helpers ---------------------------------------------------

fn row_to_provider(r: &rusqlite::Row) -> rusqlite::Result<Provider> {
    let kind_s: String = r.get(2)?;
    let aliases_json: String = r.get(7)?;
    let config_json: String = r.get(8)?;
    let cfg: ProviderConfig = serde_json::from_str(&config_json).unwrap_or_default();
    let kind = provider_kind_from_str(&kind_s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
    })?;
    let base_url: String = r.get(3)?;
    let mut protocols = cfg.protocols.clone();
    if protocols.is_empty() {
        protocols.push(ProviderProtocol {
            kind,
            base_url: base_url.clone(),
            model_aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        });
    }
    let host = cfg.host.clone().or_else(|| host_key_from_base(&base_url));
    Ok(Provider {
        id: r.get(0)?,
        name: r.get(1)?,
        group_name: cfg.group_name,
        avatar_url: cfg.avatar_url,
        kind,
        base_url,
        protocols,
        host,
        auth_ref: r.get(4)?,
        enabled: r.get::<_, i64>(5)? != 0,
        priority: r.get(6)?,
        supports_websocket: cfg.supports_websocket,
        passthrough_mode: cfg.passthrough_mode,
        remote_models: cfg.remote_models,
        remote_models_fetched_at: cfg.remote_models_fetched_at,
        last_speedtest: cfg.last_speedtest,
        model_aliases: serde_json::from_str(&aliases_json).unwrap_or_default(),
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

fn row_to_route(r: &rusqlite::Row) -> rusqlite::Result<Route> {
    let tier_s: String = r.get(5)?;
    Ok(Route {
        id: r.get(0)?,
        name: r.get(1)?,
        match_model: r.get(2)?,
        target_provider_id: r.get(3)?,
        target_model: r.get(4)?,
        tier: _route_tier_from_str(&tier_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
        })?,
        priority: r.get(6)?,
    })
}

fn row_to_health(r: &rusqlite::Row) -> rusqlite::Result<DbHealth> {
    Ok(DbHealth {
        provider_id: r.get(0)?,
        is_healthy: r.get::<_, i64>(1)? != 0,
        consecutive_failures: r.get(2)?,
        total_requests: r.get(3)?,
        total_successes: r.get(4)?,
        total_failures: r.get(5)?,
        last_success_at: r.get(6)?,
        last_failure_at: r.get(7)?,
        last_error: r.get(8)?,
        avg_latency_ms: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

fn row_to_credential(r: &rusqlite::Row) -> rusqlite::Result<vibe_protocol::Credential> {
    // Columns (0-based), matching CRED_COLS order:
    // 0  id, 1  provider_id, 2  label, 3  auth_ref, 4  plan_type, 5  notes,
    let oauth_has_refresh: bool = r.get::<_, Option<String>>(20)?.is_some();
    let remote_models: Vec<String> = r
        .get::<_, String>(26)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let balance: Option<ProviderBalanceSnapshot> = r
        .get::<_, Option<String>>(28)?
        .and_then(|s| serde_json::from_str(&s).ok());
    let usage: Option<ProviderBalanceSnapshot> = r
        .get::<_, Option<String>>(29)?
        .and_then(|s| serde_json::from_str(&s).ok());
    let upstream_vendor: Option<vibe_protocol::CredentialVendor> = r
        .get::<_, Option<String>>(31)?
        .and_then(|s| serde_json::from_str(&format!(r#""{s}""#)).ok());
    let upstream_has_session: bool = r.get::<_, Option<String>>(33)?.is_some();
    let windows: Vec<vibe_protocol::UsageWindow> = r
        .get::<_, String>(37)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Ok(vibe_protocol::Credential {
        id: r.get(0)?,
        provider_id: r.get(1)?,
        label: r.get(2)?,
        auth_ref: r.get(3)?,
        plan_type: r.get(4)?,
        notes: r.get(5)?,
        enabled: r.get::<_, i64>(6)? != 0,
        priority: r.get(7)?,
        rl_requests_limit: r.get(8)?,
        rl_requests_remaining: r.get(9)?,
        rl_requests_reset_at: r.get(10)?,
        rl_tokens_limit: r.get(11)?,
        rl_tokens_remaining: r.get(12)?,
        rl_tokens_reset_at: r.get(13)?,
        last_used_at: r.get(14)?,
        last_error: r.get(15)?,
        consecutive_failures: r.get(16)?,
        created_at: r.get(17)?,
        updated_at: r.get(18)?,
        oauth_access_token: r.get(19)?,
        oauth_has_refresh,
        oauth_expires_at: r.get(21)?,
        auth_fingerprint: r.get(22)?,
        oauth_account_email: r.get(23)?,
        oauth_account_subject: r.get(24)?,
        oauth_chatgpt_plan_slug: r.get(25)?,
        remote_models,
        remote_models_fetched_at: r.get(27)?,
        balance,
        usage,
        balance_fetched_at: r.get(30)?,
        upstream_vendor,
        upstream_username: r.get(32)?,
        upstream_has_session,
        upstream_session_expires_at: r.get(34)?,
        upstream_group: r.get(35)?,
        price_multiplier: r.get::<_, Option<f64>>(36)?.unwrap_or(1.0),
        windows,
    })
}

fn row_to_plan_snapshot(
    r: &rusqlite::Row,
) -> rusqlite::Result<vibe_protocol::CredentialPlanSnapshot> {
    Ok(vibe_protocol::CredentialPlanSnapshot {
        id: r.get(0)?,
        credential_id: r.get(1)?,
        captured_at: r.get(2)?,
        codex_5h_used_percent: r.get(3)?,
        codex_7d_used_percent: r.get(4)?,
        codex_5h_reset_after_seconds: r.get(5)?,
        codex_7d_reset_after_seconds: r.get(6)?,
        codex_primary_used_percent: r.get(7)?,
        codex_secondary_used_percent: r.get(8)?,
        summary: r.get(9)?,
        source: r.get(10)?,
    })
}

fn row_to_attempt(r: &rusqlite::Row) -> rusqlite::Result<UpstreamAttemptLog> {
    let phase_s: String = r.get(11)?;
    let outcome_s: String = r.get(12)?;
    Ok(UpstreamAttemptLog {
        attempt_id: r.get(0)?,
        request_id: r.get(1)?,
        attempt_index: r.get(2)?,
        started_at: r.get(3)?,
        ended_at: r.get(4)?,
        provider_id: r.get(5)?,
        credential_id: r.get(6)?,
        wire: r.get(7)?,
        route_prefix: r.get(8)?,
        requested_model: r.get(9)?,
        upstream_model: r.get(10)?,
        phase: upstream_attempt_phase_from_str(&phase_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, e.into())
        })?,
        outcome: upstream_attempt_outcome_from_str(&outcome_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, e.into())
        })?,
        status_code: r.get(13)?,
        upstream_http_status: r.get(14)?,
        error_summary: r.get(15)?,
        latency_ms: r.get(16)?,
        first_token_ms: r.get(17)?,
        input_tokens: r.get(18)?,
        output_tokens: r.get(19)?,
        cache_read_tokens: r.get(20)?,
        cache_creation_tokens: r.get(21)?,
        upstream_first_byte_ms: r.get(22)?,
        client_first_write_ms: r.get(23)?,
        last_upstream_event_ms: r.get(24)?,
        last_client_write_ms: r.get(25)?,
        upstream_chunk_count: r.get(26)?,
        upstream_bytes: r.get(27)?,
        client_chunk_count: r.get(28)?,
        client_bytes: r.get(29)?,
        sse_event_count: r.get(30)?,
        sse_data_count: r.get(31)?,
        sse_comment_count: r.get(32)?,
        sse_keepalive_count: r.get(33)?,
        sse_done_count: r.get(34)?,
        parse_error_count: r.get(35)?,
        first_keepalive_ms: r.get(36)?,
        last_keepalive_ms: r.get(37)?,
        max_gap_between_upstream_events_ms: r.get(38)?,
        max_gap_between_data_events_ms: r.get(39)?,
        keepalive_after_last_data_count: r.get(40)?,
        last_data_event_ms: r.get(41)?,
        bridge_mode: r.get(42)?,
        status_injected: r.get::<_, i32>(43)? != 0,
        terminal_injected: r.get::<_, i32>(44)? != 0,
        upstream_terminal_type: r.get(45)?,
        active_upstream_decode_tps_peak: r.get(46)?,
        active_downstream_emit_tps_peak: r.get(47)?,
        request_headers: r.get(48)?,
        request_body: r.get(49)?,
        response_headers: r.get(50)?,
        response_body: r.get(51)?,
    })
}

fn row_to_log_list(r: &rusqlite::Row) -> rusqlite::Result<RequestLog> {
    Ok(RequestLog {
        id: r.get(0)?,
        started_at: r.get(1)?,
        app: r.get(2)?,
        provider_id: r.get(3)?,
        requested_model: r.get(4)?,
        upstream_model: r.get(5)?,
        status_code: r.get(6)?,
        error: r.get(7)?,
        latency_ms: r.get(8)?,
        first_token_ms: r.get(9)?,
        input_tokens: r.get(10)?,
        output_tokens: r.get(11)?,
        cache_read_tokens: r.get(12)?,
        cache_creation_tokens: r.get(13)?,
        estimated_cost_usd: r.get(14)?,
        wire: r.get(15)?,
        route_prefix: r.get(16)?,
        credential_id: r.get(17)?,
        cb_key: r.get(18)?,
        upstream_http_status: r.get(19)?,
        upstream_error_preview: r.get(20)?,
        dedupe_key: r.get(21)?,
        client_transport: r.get(22)?,
        request_headers: None,
        request_body: None,
        response_body: None,
        client_response_body: None,
        stream_kind: r.get(24)?,
        stream_terminal_seen: opt_bool(r.get::<_, Option<i32>>(25)?),
        stream_end_reason: r.get(26)?,
        stream_error_detail: r.get(27)?,
        upstream_first_byte_ms: r.get(28)?,
        client_first_write_ms: r.get(29)?,
        last_upstream_event_ms: r.get(30)?,
        last_client_write_ms: r.get(31)?,
        upstream_chunk_count: r.get(32)?,
        upstream_bytes: r.get(33)?,
        client_chunk_count: r.get(34)?,
        client_bytes: r.get(35)?,
        sse_event_count: r.get(36)?,
        sse_data_count: r.get(37)?,
        sse_comment_count: r.get(38)?,
        sse_keepalive_count: r.get(39)?,
        sse_done_count: r.get(40)?,
        parse_error_count: r.get(41)?,
        first_keepalive_ms: r.get(42)?,
        last_keepalive_ms: r.get(43)?,
        max_gap_between_upstream_events_ms: r.get(44)?,
        max_gap_between_data_events_ms: r.get(45)?,
        keepalive_after_last_data_count: r.get(46)?,
        last_data_event_ms: r.get(47)?,
        bridge_mode: r.get(48)?,
        status_injected: r.get::<_, i32>(49)? != 0,
        terminal_injected: r.get::<_, i32>(50)? != 0,
        upstream_terminal_type: r.get(51)?,
    })
}

fn row_to_log_detail(r: &rusqlite::Row) -> rusqlite::Result<RequestLog> {
    Ok(RequestLog {
        id: r.get(0)?,
        started_at: r.get(1)?,
        app: r.get(2)?,
        provider_id: r.get(3)?,
        requested_model: r.get(4)?,
        upstream_model: r.get(5)?,
        status_code: r.get(6)?,
        error: r.get(7)?,
        latency_ms: r.get(8)?,
        first_token_ms: r.get(9)?,
        input_tokens: r.get(10)?,
        output_tokens: r.get(11)?,
        cache_read_tokens: r.get(12)?,
        cache_creation_tokens: r.get(13)?,
        estimated_cost_usd: r.get(14)?,
        wire: r.get(15)?,
        route_prefix: r.get(16)?,
        credential_id: r.get(17)?,
        cb_key: r.get(18)?,
        upstream_http_status: r.get(19)?,
        upstream_error_preview: r.get(20)?,
        dedupe_key: r.get(21)?,
        client_transport: r.get(22)?,
        request_headers: r.get(23)?,
        request_body: r.get(24)?,
        response_body: r.get(25)?,
        client_response_body: r.get(26)?,
        stream_kind: r.get(27)?,
        stream_terminal_seen: opt_bool(r.get::<_, Option<i32>>(28)?),
        stream_end_reason: r.get(29)?,
        stream_error_detail: r.get(30)?,
        upstream_first_byte_ms: r.get(31)?,
        client_first_write_ms: r.get(32)?,
        last_upstream_event_ms: r.get(33)?,
        last_client_write_ms: r.get(34)?,
        upstream_chunk_count: r.get(35)?,
        upstream_bytes: r.get(36)?,
        client_chunk_count: r.get(37)?,
        client_bytes: r.get(38)?,
        sse_event_count: r.get(39)?,
        sse_data_count: r.get(40)?,
        sse_comment_count: r.get(41)?,
        sse_keepalive_count: r.get(42)?,
        sse_done_count: r.get(43)?,
        parse_error_count: r.get(44)?,
        first_keepalive_ms: r.get(45)?,
        last_keepalive_ms: r.get(46)?,
        max_gap_between_upstream_events_ms: r.get(47)?,
        max_gap_between_data_events_ms: r.get(48)?,
        keepalive_after_last_data_count: r.get(49)?,
        last_data_event_ms: r.get(50)?,
        bridge_mode: r.get(51)?,
        status_injected: r.get::<_, i32>(52)? != 0,
        terminal_injected: r.get::<_, i32>(53)? != 0,
        upstream_terminal_type: r.get(54)?,
    })
}

fn opt_bool(v: Option<i32>) -> Option<bool> {
    v.map(|x| x != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input() -> ProviderInput {
        ProviderInput {
            name: "anthropic-prod".into(),
            group_name: Some("prod".into()),
            kind: ProviderKind::Anthropic,
            avatar_url: None,
            base_url: "https://api.anthropic.com".into(),
            protocols: vec![],
            host: None,
            auth_ref: Some("keyring:anthropic-prod".into()),
            enabled: true,
            priority: 100,
            supports_websocket: None,
            passthrough_mode: true,
            model_aliases: vec![ModelAlias {
                alias: "high".into(),
                upstream_model: "claude-sonnet-4-6".into(),
            }],
        }
    }

    fn sample_route_input() -> RouteInput {
        RouteInput {
            name: "high route".into(),
            match_model: "high".into(),
            target_provider_id: None,
            target_model: Some("claude-sonnet-4-6".into()),
            tier: RouteTier::High,
            priority: 10,
        }
    }

    fn sample_credential_input(label: &str) -> CredentialInput {
        CredentialInput {
            label: label.into(),
            auth_ref: Some(format!("keyring:{label}")),
            plan_type: Some("pro".into()),
            notes: Some("primary credential".into()),
            enabled: true,
            priority: 10,
            upstream_vendor: Some(CredentialVendor::NewApi),
            upstream_username: Some(format!("{label}@example.com")),
            upstream_session: Some(format!("session-{label}")),
            upstream_session_expires_at: Some(12_345),
            upstream_group: Some("default".into()),
            price_multiplier: 1.25,
            ..CredentialInput::default()
        }
    }

    fn sample_log(
        id: &str,
        started_at: i64,
        provider_id: Option<&str>,
        status_code: Option<i32>,
    ) -> RequestLog {
        RequestLog {
            id: id.into(),
            started_at,
            app: Some("claude-code".into()),
            provider_id: provider_id.map(str::to_string),
            requested_model: Some("high".into()),
            upstream_model: Some("claude-sonnet-4-6".into()),
            status_code,
            error: if matches!(status_code, Some(400..=599) | None) {
                Some("upstream failed".into())
            } else {
                None
            },
            latency_ms: Some(123),
            first_token_ms: Some(50),
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 3,
            cache_creation_tokens: 4,
            estimated_cost_usd: "0.001".into(),
            wire: Some("anthropic".into()),
            route_prefix: Some("codex-v1".into()),
            credential_id: Some("cred-1".into()),
            cb_key: Some("cb-1".into()),
            upstream_http_status: status_code,
            upstream_error_preview: None,
            dedupe_key: Some(format!("dedupe-{id}")),
            client_transport: Some("http-sse".into()),
            request_headers: Some("x-test: 1".into()),
            request_body: Some(format!(r#"{{"id":"{id}"}}"#)),
            response_body: Some("upstream-body".into()),
            client_response_body: Some("client-body".into()),
            stream_kind: Some("sse".into()),
            stream_terminal_seen: Some(true),
            stream_end_reason: Some("stop".into()),
            stream_error_detail: None,
            upstream_first_byte_ms: Some(25),
            client_first_write_ms: Some(30),
            last_upstream_event_ms: Some(120),
            last_client_write_ms: Some(125),
            upstream_chunk_count: 5,
            upstream_bytes: 500,
            client_chunk_count: 6,
            client_bytes: 600,
            sse_event_count: 7,
            sse_data_count: 8,
            sse_comment_count: 1,
            sse_keepalive_count: 2,
            sse_done_count: 1,
            parse_error_count: 0,
            first_keepalive_ms: Some(60),
            last_keepalive_ms: Some(90),
            max_gap_between_upstream_events_ms: Some(40),
            max_gap_between_data_events_ms: Some(45),
            keepalive_after_last_data_count: 1,
            last_data_event_ms: Some(110),
            bridge_mode: Some("chat-to-responses".into()),
            status_injected: true,
            terminal_injected: true,
            upstream_terminal_type: Some("done".into()),
        }
    }

    #[test]
    fn provider_crud() {
        let db = Db::memory().unwrap();
        let p = db.provider_insert(sample_input()).unwrap();
        assert_eq!(p.name, "anthropic-prod");

        let all = db.provider_list().unwrap();
        assert_eq!(all.len(), 1);

        let mut input = sample_input();
        input.name = "anthropic-prod-2".into();
        let updated = db.provider_update(&p.id, input).unwrap();
        assert_eq!(updated.name, "anthropic-prod-2");

        db.provider_delete(&p.id).unwrap();
        assert_eq!(db.provider_list().unwrap().len(), 0);
    }

    #[test]
    fn provider_host_lookup_and_consolidate_moves_credentials() {
        let db = Db::memory().unwrap();
        let mut first = sample_input();
        first.name = "deepseek-hk".into();
        first.kind = ProviderKind::OpenaiChat;
        first.base_url = "https://API.DeepSeek.com/v1/".into();
        first.host = None;
        first.priority = 20;
        let keep = db.provider_insert(first).unwrap();
        assert_eq!(keep.host.as_deref(), Some("api.deepseek.com"));

        let mut dupe_input = sample_input();
        dupe_input.name = "deepseek-sg".into();
        dupe_input.kind = ProviderKind::OpenaiResponses;
        dupe_input.base_url = "https://proxy.local/responses".into();
        dupe_input.host = Some("www.API.DeepSeek.COM".into());
        dupe_input.priority = 10;
        let dupe = db.provider_insert(dupe_input).unwrap();

        let cred = db
            .credential_insert(
                &dupe.id,
                sample_credential_input("dupe-key"),
                Some("fp:dupe".into()),
            )
            .unwrap();
        assert_eq!(
            db.provider_find_by_host("https://www.api.deepseek.com/v1")
                .unwrap()
                .unwrap()
                .id,
            dupe.id,
            "host lookup follows provider ordering before consolidation"
        );
        assert_eq!(
            db.provider_find_all_by_host("api.deepseek.com")
                .unwrap()
                .len(),
            2
        );

        db.provider_consolidate_by_host(&keep.id, "https://api.deepseek.com/v1")
            .unwrap();

        assert!(db.provider_get(&dupe.id).unwrap().is_none());
        assert_eq!(
            db.provider_find_all_by_host("api.deepseek.com")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.credential_get(&cred.id).unwrap().unwrap().provider_id,
            keep.id
        );
    }

    #[test]
    fn route_crud_orders_by_priority_and_reports_missing_rows() {
        let db = Db::memory().unwrap();
        let provider = db.provider_insert(sample_input()).unwrap();

        let mut low = sample_route_input();
        low.name = "low route".into();
        low.match_model = "low".into();
        low.tier = RouteTier::Low;
        low.priority = 50;
        let low = db.route_insert(low).unwrap();

        let mut high = sample_route_input();
        high.target_provider_id = Some(provider.id.clone());
        high.priority = 5;
        let high = db.route_insert(high).unwrap();

        let listed = db.route_list().unwrap();
        assert_eq!(
            listed.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![high.id.as_str(), low.id.as_str()]
        );

        let mut update = sample_route_input();
        update.name = "updated high".into();
        update.match_model = "sonnet".into();
        update.target_provider_id = Some(provider.id);
        update.target_model = Some("claude-3-7-sonnet-latest".into());
        update.tier = RouteTier::Default;
        update.priority = 1;
        let updated = db.route_update(&high.id, update).unwrap();
        assert_eq!(updated.name, "updated high");
        assert_eq!(
            db.route_get(&high.id).unwrap().unwrap().match_model,
            "sonnet"
        );

        db.route_delete(&low.id).unwrap();
        assert!(db.route_get(&low.id).unwrap().is_none());
        assert!(db.route_update("missing", sample_route_input()).is_err());
        assert!(db.route_delete("missing").is_err());
    }

    #[test]
    fn log_insert_list_filter_and_get_preserve_summary_vs_body_fields() {
        let db = Db::memory().unwrap();
        let p1 = db.provider_insert(sample_input()).unwrap();
        let mut p2_input = sample_input();
        p2_input.name = "other".into();
        p2_input.base_url = "https://api.other.example/v1".into();
        let p2 = db.provider_insert(p2_input).unwrap();
        let base = now_secs() - 100;

        db.log_insert(&sample_log("ok-old", base, Some(&p1.id), Some(200)))
            .unwrap();
        db.log_insert(&sample_log("fail-new", base + 10, Some(&p1.id), Some(500)))
            .unwrap();
        db.log_insert(&sample_log("ok-other", base + 20, Some(&p2.id), Some(204)))
            .unwrap();

        let page = db.log_list(2, 0).unwrap();
        assert_eq!(
            page.items.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
            vec!["ok-other", "fail-new"]
        );
        assert!(page.has_more);
        assert_eq!(
            page.items[0].request_headers, None,
            "list endpoint omits bulky/sensitive request headers"
        );
        assert_eq!(page.items[0].request_body, None);

        let filtered_ok = db
            .log_list_filtered(10, 0, Some(base + 5), Some(&p1.id), Some(true))
            .unwrap();
        assert!(filtered_ok.items.is_empty());
        let filtered_fail = db
            .log_list_filtered(10, 0, Some(base + 5), Some(&p1.id), Some(false))
            .unwrap();
        assert_eq!(
            filtered_fail
                .items
                .iter()
                .map(|l| l.id.as_str())
                .collect::<Vec<_>>(),
            vec!["fail-new"]
        );

        let detail = db.log_get("fail-new").unwrap().unwrap();
        assert_eq!(detail.request_headers.as_deref(), Some("x-test: 1"));
        assert_eq!(detail.request_body.as_deref(), Some(r#"{"id":"fail-new"}"#));
        assert_eq!(detail.response_body.as_deref(), Some("upstream-body"));
        assert_eq!(detail.stream_terminal_seen, Some(true));
        assert!(detail.status_injected);
        assert_eq!(db.log_get("missing").unwrap().map(|l| l.id), None);
    }

    #[test]
    fn log_insert_and_summary() {
        let db = Db::memory().unwrap();
        db.log_insert(&sample_log("log-1", now_secs(), None, Some(200)))
            .unwrap();

        let page = db.log_list(50, 0).unwrap();
        assert_eq!(page.items.len(), 1);
        let summary = db.usage_summary_last_hours(1).unwrap();
        assert_eq!(summary.requests, 1);
        assert_eq!(summary.input_tokens, 10);
        assert_eq!(summary.output_tokens, 20);
        assert_eq!(summary.cache_read_tokens, 3);
        assert_eq!(summary.cache_creation_tokens, 4);
    }

    #[test]
    fn credential_crud_enable_rate_limit_financials_and_sensitive_reads() {
        let db = Db::memory().unwrap();
        let provider = db.provider_insert(sample_input()).unwrap();

        let cred = db
            .credential_insert(
                &provider.id,
                sample_credential_input("primary"),
                Some("fp:primary".into()),
            )
            .unwrap();
        assert_eq!(cred.provider_id, provider.id);
        assert_eq!(cred.label, "primary");
        assert_eq!(cred.auth_fingerprint.as_deref(), Some("fp:primary"));
        assert_eq!(cred.upstream_vendor, Some(CredentialVendor::NewApi));
        assert!(cred.upstream_has_session);
        assert_eq!(
            db.credential_get_session(&cred.id).unwrap().as_deref(),
            Some("session-primary")
        );
        assert_eq!(
            db.credential_list_for_provider(&provider.id).unwrap().len(),
            1
        );
        assert!(db
            .credential_has_fingerprint_for_provider(&provider.id, "fp:primary")
            .unwrap());

        let disabled = db.credential_set_enabled(&cred.id, false).unwrap();
        assert!(!disabled.enabled);

        db.credential_update_rate_limit(
            &cred.id,
            Some(100),
            Some(42),
            Some(1_000),
            Some(200),
            Some(99),
            Some(2_000),
        )
        .unwrap();
        let limited = db.credential_get(&cred.id).unwrap().unwrap();
        assert_eq!(limited.rl_requests_limit, Some(100));
        assert_eq!(limited.rl_requests_remaining, Some(42));
        assert_eq!(limited.rl_tokens_limit, Some(200));
        assert_eq!(limited.rl_tokens_remaining, Some(99));

        db.credential_record_failure(&cred.id, Some("rate limited"))
            .unwrap();
        let failed = db.credential_get(&cred.id).unwrap().unwrap();
        assert_eq!(failed.consecutive_failures, 1);
        assert_eq!(failed.last_error.as_deref(), Some("rate limited"));
        assert!(failed.last_used_at.is_some());
        db.credential_record_success(&cred.id).unwrap();
        let recovered = db.credential_get(&cred.id).unwrap().unwrap();
        assert_eq!(recovered.consecutive_failures, 0);
        assert_eq!(recovered.last_error, None);

        let balance = ProviderBalanceSnapshot {
            currency: "USD".into(),
            balance: Some("12.34".into()),
            remaining: Some("10.00".into()),
            used: Some("2.34".into()),
            total: None,
            period: Some("monthly".into()),
            note: Some("test balance".into()),
        };
        let usage = ProviderBalanceSnapshot {
            currency: "USD".into(),
            balance: None,
            remaining: None,
            used: Some("1.23".into()),
            total: Some("20.00".into()),
            period: Some("daily".into()),
            note: None,
        };
        let with_money = db
            .credential_update_financials(
                &cred.id,
                Some(balance.clone()),
                Some(usage.clone()),
                55_555,
            )
            .unwrap();
        assert_eq!(with_money.balance.unwrap().balance, balance.balance);
        assert_eq!(with_money.usage.unwrap().total, usage.total);
        assert_eq!(with_money.balance_fetched_at, Some(55_555));

        let mut update = sample_credential_input("updated");
        update.oauth_access_token = Some("access-2".into());
        update.oauth_refresh_token = Some("refresh-2".into());
        update.oauth_expires_at = Some(99_999);
        update.upstream_session = None;
        update.enabled = true;
        let updated = db
            .credential_update(&cred.id, update, Some("fp:updated".into()))
            .unwrap();
        assert_eq!(updated.label, "updated");
        assert!(updated.oauth_has_refresh);
        assert_eq!(
            db.credential_get_refresh_token(&cred.id)
                .unwrap()
                .as_deref(),
            Some("refresh-2")
        );
        assert_eq!(
            db.credential_count_same_fingerprint("fp:updated", None)
                .unwrap(),
            1
        );
        assert_eq!(
            db.credential_count_same_fingerprint("fp:updated", Some(&cred.id))
                .unwrap(),
            0
        );

        db.credential_delete(&cred.id).unwrap();
        assert!(db.credential_get(&cred.id).unwrap().is_none());
        assert!(db.credential_delete(&cred.id).is_err());
    }
}
