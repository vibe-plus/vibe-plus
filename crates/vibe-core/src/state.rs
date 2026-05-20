//! Process-wide state shared across axum handlers.

use crate::circuit_breaker::CircuitBreakers;
use crate::config::{ClaudeConfig, CodexSummaryConfig, Config};
use anyhow::Result;
use reqwest::Client;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vibe_db::Db;
use vibe_observability::ObservabilityStore;
use vibe_plugin_api::PluginRegistry;
use vibe_protocol::{
    AppLogEvent, RealtimeAttempt, RealtimeProvider, RealtimeRequest, RealtimeSnapshot,
    UpstreamAttemptLog, UpstreamAttemptPhase,
};

const MAX_IN_MEMORY_APP_LOGS: usize = 500;

const MAX_RECENT_REALTIME_REQUESTS: usize = 20;
const ACTIVE_REQUEST_STALE_SECS: i64 = 120;

#[derive(Default)]
pub struct RealtimeRequests {
    active: Mutex<HashMap<String, RealtimeRequest>>,
    recent: Mutex<VecDeque<RealtimeRequest>>,
    attempts: Mutex<HashMap<String, RealtimeAttempt>>,
}

#[allow(clippy::too_many_arguments)]
impl RealtimeRequests {
    pub fn started(
        &self,
        id: &str,
        started_at: i64,
        app: &Option<String>,
        provider_id: Option<&str>,
        credential_id: Option<&str>,
        requested_model: &str,
        upstream_model: Option<&str>,
        wire: Option<&str>,
        route_prefix: Option<&str>,
        client_transport: Option<&str>,
    ) {
        let now = chrono::Utc::now().timestamp();
        let item = RealtimeRequest {
            id: id.to_owned(),
            started_at,
            updated_at: now,
            app: app.clone(),
            provider_id: provider_id.map(str::to_owned),
            credential_id: credential_id.map(str::to_owned),
            requested_model: (!requested_model.is_empty()).then(|| requested_model.to_owned()),
            upstream_model: upstream_model.and_then(|m| (!m.is_empty()).then(|| m.to_owned())),
            wire: wire.map(str::to_owned),
            route_prefix: route_prefix.map(str::to_owned),
            client_transport: client_transport.map(str::to_owned),
            phase: "routing".to_owned(),
            status_code: None,
            error: None,
            active_output_tokens_per_sec: None,
            active_cost_usd_per_hour: None,
            active_upstream_bytes_per_sec: 0.0,
            active_downstream_bytes_per_sec: 0.0,
            output_tokens_so_far: 0,
            upstream_bytes_so_far: 0,
            client_bytes_so_far: 0,
            upstream_first_byte_ms: None,
            client_first_write_ms: None,
            attempts: Vec::new(),
        };
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.to_owned(), item);
    }

    pub fn attempt_started(
        &self,
        attempt_id: &str,
        request_id: &str,
        attempt_index: i32,
        wave_index: i32,
        wave_size: i32,
        upstream_id: Option<&str>,
        provider_id: Option<&str>,
        credential_id: Option<&str>,
        requested_model: Option<&str>,
        upstream_model: Option<&str>,
        wire: Option<&str>,
        route_prefix: Option<&str>,
        phase: &str,
    ) {
        let now = chrono::Utc::now().timestamp();
        let attempt = RealtimeAttempt {
            attempt_id: attempt_id.to_owned(),
            request_id: request_id.to_owned(),
            attempt_index,
            wave_index,
            wave_size,
            upstream_id: upstream_id.map(str::to_owned),
            started_at: now,
            updated_at: now,
            provider_id: provider_id.map(str::to_owned),
            credential_id: credential_id.map(str::to_owned),
            wire: wire.map(str::to_owned),
            route_prefix: route_prefix.map(str::to_owned),
            requested_model: requested_model.and_then(|m| (!m.is_empty()).then(|| m.to_owned())),
            upstream_model: upstream_model.and_then(|m| (!m.is_empty()).then(|| m.to_owned())),
            phase: phase.to_owned(),
            status_code: None,
            upstream_http_status: None,
            error: None,
            active_output_tokens_per_sec: None,
            active_cost_usd_per_hour: None,
            active_upstream_bytes_per_sec: 0.0,
            active_downstream_bytes_per_sec: 0.0,
            output_tokens_so_far: 0,
            upstream_bytes_so_far: 0,
            client_bytes_so_far: 0,
            upstream_first_byte_ms: None,
            client_first_write_ms: None,
            last_upstream_event_ms: None,
            last_client_write_ms: None,
        };
        self.attempts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(attempt_id.to_owned(), attempt);

        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(item) = active.get_mut(request_id) {
            item.updated_at = chrono::Utc::now().timestamp();
            if provider_id.is_some() {
                item.provider_id = provider_id.map(str::to_owned);
            }
            if credential_id.is_some() {
                item.credential_id = credential_id.map(str::to_owned);
            }
            if let Some(model) = upstream_model.filter(|m| !m.is_empty()) {
                item.upstream_model = Some(model.to_owned());
            }
            item.phase = phase.to_owned();
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn runtime(
        &self,
        attempt_id: Option<&str>,
        request_id: &str,
        provider_id: Option<&str>,
        active_output_tokens_per_sec: Option<f64>,
        active_upstream_bytes_per_sec: f64,
        active_downstream_bytes_per_sec: f64,
        output_tokens_so_far: i64,
        upstream_bytes_so_far: i64,
        client_bytes_so_far: i64,
        upstream_first_byte_ms: Option<i64>,
        client_first_write_ms: Option<i64>,
    ) {
        if let Some(attempt_id) = attempt_id {
            let mut attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(item) = attempts.get_mut(attempt_id) {
                item.updated_at = chrono::Utc::now().timestamp();
                if provider_id.is_some() {
                    item.provider_id = provider_id.map(str::to_owned);
                }
                item.phase = "streaming".to_owned();
                item.active_output_tokens_per_sec = active_output_tokens_per_sec;
                let model = item
                    .upstream_model
                    .as_deref()
                    .or(item.requested_model.as_deref());
                item.active_cost_usd_per_hour = model
                    .and_then(crate::usage::output_cost_usd_per_token)
                    .and_then(|per_token| {
                        active_output_tokens_per_sec.map(|tps| per_token * tps * 3600.0)
                    });
                item.active_upstream_bytes_per_sec = active_upstream_bytes_per_sec;
                item.active_downstream_bytes_per_sec = active_downstream_bytes_per_sec;
                item.output_tokens_so_far = output_tokens_so_far;
                item.upstream_bytes_so_far = upstream_bytes_so_far;
                item.client_bytes_so_far = client_bytes_so_far;
                item.upstream_first_byte_ms = upstream_first_byte_ms;
                item.client_first_write_ms = client_first_write_ms;
            }
        }

        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(item) = active.get_mut(request_id) {
            item.updated_at = chrono::Utc::now().timestamp();
            if provider_id.is_some() {
                item.provider_id = provider_id.map(str::to_owned);
            }
            item.phase = "streaming".to_owned();
            item.active_output_tokens_per_sec = active_output_tokens_per_sec;
            let model = item
                .upstream_model
                .as_deref()
                .or(item.requested_model.as_deref());
            item.active_cost_usd_per_hour = model
                .and_then(crate::usage::output_cost_usd_per_token)
                .and_then(|per_token| {
                    active_output_tokens_per_sec.map(|tps| per_token * tps * 3600.0)
                });
            item.active_upstream_bytes_per_sec = active_upstream_bytes_per_sec;
            item.active_downstream_bytes_per_sec = active_downstream_bytes_per_sec;
            item.output_tokens_so_far = output_tokens_so_far;
            item.upstream_bytes_so_far = upstream_bytes_so_far;
            item.client_bytes_so_far = client_bytes_so_far;
            item.upstream_first_byte_ms = upstream_first_byte_ms;
            item.client_first_write_ms = client_first_write_ms;
        }
    }

    pub fn finished(&self, id: &str, status_code: Option<i32>, error: Option<String>) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(mut item) = active.remove(id) {
            item.updated_at = chrono::Utc::now().timestamp();
            item.phase = if error.is_some() {
                "failed"
            } else {
                "completed"
            }
            .to_owned();
            item.status_code = status_code;
            item.error = error;
            drop(active);
            let mut recent = self
                .recent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            recent.push_front(item);
            while recent.len() > MAX_RECENT_REALTIME_REQUESTS {
                recent.pop_back();
            }
        }
    }

    pub fn attempt_finished(&self, attempt: &UpstreamAttemptLog) {
        let mut attempts = self
            .attempts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(item) = attempts.get_mut(&attempt.attempt_id) else {
            return;
        };
        item.updated_at = chrono::Utc::now().timestamp();
        item.phase = match attempt.phase {
            UpstreamAttemptPhase::Connecting => "connecting",
            UpstreamAttemptPhase::Streaming => "streaming",
            UpstreamAttemptPhase::Completed => "completed",
            UpstreamAttemptPhase::Failed => "failed",
            UpstreamAttemptPhase::Abandoned => "abandoned",
        }
        .to_owned();
        item.status_code = attempt.status_code;
        item.upstream_http_status = attempt.upstream_http_status;
        item.error = attempt.error_summary.clone();
        item.active_output_tokens_per_sec = None;
        item.active_cost_usd_per_hour = None;
        item.active_upstream_bytes_per_sec = 0.0;
        item.active_downstream_bytes_per_sec = 0.0;
        item.output_tokens_so_far = attempt.output_tokens;
        item.upstream_bytes_so_far = attempt.upstream_bytes;
        item.client_bytes_so_far = attempt.client_bytes;
        item.upstream_first_byte_ms = attempt.upstream_first_byte_ms;
        item.client_first_write_ms = attempt.client_first_write_ms;
        item.last_upstream_event_ms = attempt.last_upstream_event_ms;
        item.last_client_write_ms = attempt.last_client_write_ms;
    }

    pub fn snapshot(&self, codex_transport: CodexTransportStats) -> RealtimeSnapshot {
        let now = chrono::Utc::now().timestamp();
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.retain(|_, item| now.saturating_sub(item.updated_at) <= ACTIVE_REQUEST_STALE_SECS);
        let active_requests: Vec<RealtimeRequest> = active.values().cloned().collect();
        drop(active);

        let recent_requests: Vec<RealtimeRequest> = self
            .recent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect();

        let attempts_by_request: HashMap<String, Vec<RealtimeAttempt>> = {
            let attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut grouped: HashMap<String, Vec<RealtimeAttempt>> = HashMap::new();
            for attempt in attempts.values() {
                grouped
                    .entry(attempt.request_id.clone())
                    .or_default()
                    .push(attempt.clone());
            }
            for attempts in grouped.values_mut() {
                attempts.sort_by(|a, b| a.attempt_index.cmp(&b.attempt_index));
            }
            grouped
        };

        let active_requests: Vec<RealtimeRequest> = active_requests
            .into_iter()
            .map(|mut req| {
                if let Some(attempts) = attempts_by_request.get(&req.id) {
                    req.attempts = attempts.clone();
                }
                req
            })
            .collect();

        let recent_requests: Vec<RealtimeRequest> = recent_requests
            .into_iter()
            .map(|mut req| {
                if let Some(attempts) = attempts_by_request.get(&req.id) {
                    req.attempts = attempts.clone();
                }
                req
            })
            .collect();

        let mut by_provider: HashMap<String, RealtimeProvider> = HashMap::new();
        let mut active_output_tokens_per_sec = 0.0;
        let mut active_cost_usd_per_hour: Option<f64> = None;
        let mut active_upstream_bytes_per_sec = 0.0;
        let mut active_downstream_bytes_per_sec = 0.0;
        for item in &active_requests {
            active_output_tokens_per_sec += item.active_output_tokens_per_sec.unwrap_or(0.0);
            if let Some(cost) = item.active_cost_usd_per_hour {
                active_cost_usd_per_hour = Some(active_cost_usd_per_hour.unwrap_or(0.0) + cost);
            }
            active_upstream_bytes_per_sec += item.active_upstream_bytes_per_sec;
            active_downstream_bytes_per_sec += item.active_downstream_bytes_per_sec;
            if let Some(provider_id) = &item.provider_id {
                let entry =
                    by_provider
                        .entry(provider_id.clone())
                        .or_insert_with(|| RealtimeProvider {
                            provider_id: provider_id.clone(),
                            provider_name: provider_id.clone(),
                            active_requests: 0,
                            active_output_tokens_per_sec: 0.0,
                            active_cost_usd_per_hour: None,
                            active_upstream_bytes_per_sec: 0.0,
                            active_downstream_bytes_per_sec: 0.0,
                            output_tokens_so_far: 0,
                            upstream_bytes_so_far: 0,
                            client_bytes_so_far: 0,
                        });
                entry.active_requests += 1;
                entry.active_output_tokens_per_sec +=
                    item.active_output_tokens_per_sec.unwrap_or(0.0);
                if let Some(cost) = item.active_cost_usd_per_hour {
                    entry.active_cost_usd_per_hour =
                        Some(entry.active_cost_usd_per_hour.unwrap_or(0.0) + cost);
                }
                entry.active_upstream_bytes_per_sec += item.active_upstream_bytes_per_sec;
                entry.active_downstream_bytes_per_sec += item.active_downstream_bytes_per_sec;
                entry.output_tokens_so_far += item.output_tokens_so_far;
                entry.upstream_bytes_so_far += item.upstream_bytes_so_far;
                entry.client_bytes_so_far += item.client_bytes_so_far;
            }
        }
        let mut providers: Vec<RealtimeProvider> = by_provider.into_values().collect();
        providers.sort_by(|a, b| b.active_requests.cmp(&a.active_requests));

        RealtimeSnapshot {
            now,
            active_count: active_requests.len(),
            active_requests,
            recent_requests,
            providers,
            active_output_tokens_per_sec,
            active_cost_usd_per_hour,
            active_upstream_bytes_per_sec,
            active_downstream_bytes_per_sec,
            codex_ws_active: codex_transport.ws_active,
            codex_last_transport: codex_transport.last_transport,
        }
    }
}

#[derive(Default)]
pub struct InMemoryAppLogs {
    inner: Mutex<VecDeque<AppLogEvent>>,
}

impl InMemoryAppLogs {
    pub fn push(&self, event: AppLogEvent) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.push_front(event);
        while inner.len() > MAX_IN_MEMORY_APP_LOGS {
            inner.pop_back();
        }
    }

    pub fn list(&self, limit: i64, since: Option<i64>) -> Vec<AppLogEvent> {
        let limit = limit.clamp(1, 500) as usize;
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|log| since.is_none_or(|ts| log.ts >= ts))
            .take(limit)
            .cloned()
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct CodexStickyRoute {
    pub provider_id: String,
    pub credential_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CodexTransportStats {
    pub ws_active: usize,
    pub ws_total: usize,
    pub ws_requests_total: usize,
    pub http_responses_total: usize,
    pub last_transport: Option<String>,
}

#[derive(Default)]
pub struct CodexTransportCounters {
    ws_active: AtomicUsize,
    ws_total: AtomicUsize,
    ws_requests_total: AtomicUsize,
    http_responses_total: AtomicUsize,
    last_transport: Mutex<Option<String>>,
}

impl CodexTransportCounters {
    pub fn ws_opened(&self) {
        self.ws_active.fetch_add(1, Ordering::Relaxed);
        self.ws_total.fetch_add(1, Ordering::Relaxed);
        self.set_last_transport("ws");
    }

    pub fn ws_closed(&self) {
        self.ws_active.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn ws_request(&self) {
        self.ws_requests_total.fetch_add(1, Ordering::Relaxed);
        self.set_last_transport("ws");
    }

    pub fn http_response_request(&self, streaming: bool) {
        self.http_responses_total.fetch_add(1, Ordering::Relaxed);
        self.set_last_transport(if streaming { "http-sse" } else { "http" });
    }

    pub fn snapshot(&self) -> CodexTransportStats {
        CodexTransportStats {
            ws_active: self.ws_active.load(Ordering::Relaxed),
            ws_total: self.ws_total.load(Ordering::Relaxed),
            ws_requests_total: self.ws_requests_total.load(Ordering::Relaxed),
            http_responses_total: self.http_responses_total.load(Ordering::Relaxed),
            last_transport: self
                .last_transport
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }

    fn set_last_transport(&self, transport: &str) {
        *self
            .last_transport
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(transport.to_owned());
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub http: Client,
    pub config: Arc<Config>,
    pub started_at: Instant,
    pub port: u16,
    pub cb: CircuitBreakers,
    /// Monotonically increasing per-request counter used for round-robin
    /// load balancing among same-priority providers.
    pub lb_counter: Arc<AtomicUsize>,
    pub codex_status_dedupe: Arc<Mutex<HashMap<String, Instant>>>,
    pub codex_summary_dedupe: Arc<Mutex<HashMap<String, Instant>>>,
    pub claude_summary_dedupe: Arc<Mutex<HashMap<String, Instant>>>,
    pub codex_transport: Arc<CodexTransportCounters>,
    pub codex_sticky_routes: Arc<Mutex<HashMap<String, (CodexStickyRoute, Instant)>>>,
    /// Accumulated USD cost per thread_id (keyed by thread_id, TTL 30 min).
    pub codex_thread_costs: Arc<Mutex<HashMap<String, (f64, Instant)>>>,
    /// Accumulated (input_sum, output_sum) per turn_id across ALL requests in a turn.
    /// Lets the final-request summary see tokens from every tool-call iteration.
    pub codex_turn_io: Arc<Mutex<HashMap<String, (i64, i64, Instant)>>>,
    pub app_logs: Arc<InMemoryAppLogs>,
    pub realtime: Arc<RealtimeRequests>,
    pub plugins: PluginRegistry<AppState>,
    pub observability: Option<ObservabilityStore>,
    codex_summary_config: Arc<Mutex<CodexSummaryConfig>>,
    /// Mirrors `codex.route_status_enabled` from disk; updated on config GET/PUT.
    codex_route_status_on: Arc<AtomicBool>,
    claude_summary_config: Arc<Mutex<CodexSummaryConfig>>,
    claude_config: Arc<Mutex<ClaudeConfig>>,
}

impl AppState {
    pub fn init(db: Db, config: Config, port: u16) -> Result<Self> {
        #[cfg(test)]
        let observability = ObservabilityStore::memory().ok();
        #[cfg(not(test))]
        let observability = Some(ObservabilityStore::open(
            crate::paths::observability_db_path()?,
        )?);
        Self::init_with_optional_observability(db, config, port, observability)
    }

    pub fn init_with_observability(
        db: Db,
        config: Config,
        port: u16,
        observability: ObservabilityStore,
    ) -> Result<Self> {
        Self::init_with_optional_observability(db, config, port, Some(observability))
    }

    pub fn init_without_observability(db: Db, config: Config, port: u16) -> Result<Self> {
        Self::init_with_optional_observability(db, config, port, None)
    }

    pub fn init_with_optional_observability(
        db: Db,
        config: Config,
        port: u16,
        observability: Option<ObservabilityStore>,
    ) -> Result<Self> {
        if let Some(observability) = observability.as_ref() {
            observability.migrate_from_legacy(&db)?;
        }
        let plugins = if let Some(observability) = observability.as_ref() {
            PluginRegistry::new().with_plugin(vibe_observability::ObservabilityPlugin::from_store(
                observability.clone(),
            ))
        } else {
            PluginRegistry::new()
        };
        let http = Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        let cb = CircuitBreakers::new(config.failover.clone());
        Ok(Self {
            db,
            http,
            codex_summary_config: Arc::new(Mutex::new(config.codex.summary.clone())),
            codex_route_status_on: Arc::new(AtomicBool::new(config.codex.route_status_enabled)),
            claude_summary_config: Arc::new(Mutex::new(config.claude.summary.clone())),
            claude_config: Arc::new(Mutex::new(config.claude.clone())),
            config: Arc::new(config),
            started_at: Instant::now(),
            port,
            cb,
            lb_counter: Arc::new(AtomicUsize::new(0)),
            codex_status_dedupe: Arc::new(Mutex::new(HashMap::new())),
            codex_summary_dedupe: Arc::new(Mutex::new(HashMap::new())),
            claude_summary_dedupe: Arc::new(Mutex::new(HashMap::new())),
            codex_transport: Arc::new(CodexTransportCounters::default()),
            codex_sticky_routes: Arc::new(Mutex::new(HashMap::new())),
            codex_thread_costs: Arc::new(Mutex::new(HashMap::new())),
            codex_turn_io: Arc::new(Mutex::new(HashMap::new())),
            app_logs: Arc::new(InMemoryAppLogs::default()),
            realtime: Arc::new(RealtimeRequests::default()),
            plugins,
            observability,
        })
    }

    pub fn codex_summary_config(&self) -> CodexSummaryConfig {
        self.codex_summary_config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn set_codex_summary_config(&self, cfg: CodexSummaryConfig) {
        *self
            .codex_summary_config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = cfg;
    }

    pub fn codex_route_status_enabled(&self) -> bool {
        self.codex_route_status_on.load(Ordering::Relaxed)
    }

    pub fn set_codex_route_status_enabled(&self, enabled: bool) {
        self.codex_route_status_on.store(enabled, Ordering::Relaxed);
    }

    pub fn claude_summary_config(&self) -> CodexSummaryConfig {
        self.claude_summary_config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn set_claude_summary_config(&self, cfg: CodexSummaryConfig) {
        *self
            .claude_summary_config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = cfg;
    }

    pub fn claude_config(&self) -> ClaudeConfig {
        self.claude_config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn set_claude_config(&self, cfg: ClaudeConfig) {
        self.set_claude_summary_config(cfg.summary.clone());
        *self
            .claude_config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = cfg;
    }

    pub fn remember_codex_status_key(&self, key: String, ttl: Duration) -> bool {
        let now = Instant::now();
        let mut seen = self
            .codex_status_dedupe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        seen.retain(|_, last_seen| now.duration_since(*last_seen) <= ttl);
        if seen.contains_key(&key) {
            return false;
        }
        seen.insert(key, now);
        true
    }

    pub fn remember_codex_summary_key(&self, key: String, ttl: Duration) -> bool {
        let now = Instant::now();
        let mut seen = self
            .codex_summary_dedupe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        seen.retain(|_, last_seen| now.duration_since(*last_seen) <= ttl);
        if seen.contains_key(&key) {
            return false;
        }
        seen.insert(key, now);
        true
    }

    pub fn remember_claude_summary_key(&self, key: String, ttl: Duration) -> bool {
        let now = Instant::now();
        let mut seen = self
            .claude_summary_dedupe
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        seen.retain(|_, last_seen| now.duration_since(*last_seen) <= ttl);
        if seen.contains_key(&key) {
            return false;
        }
        seen.insert(key, now);
        true
    }

    /// Read the current sticky route without updating `last_seen` (non-destructive peek).
    pub fn peek_codex_sticky_route(&self, key: &str, ttl: Duration) -> Option<CodexStickyRoute> {
        let now = Instant::now();
        let routes = self
            .codex_sticky_routes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (route, last_seen) = routes.get(key)?;
        if now.duration_since(*last_seen) > ttl {
            return None;
        }
        Some(route.clone())
    }

    pub fn get_codex_sticky_route(&self, key: &str, ttl: Duration) -> Option<CodexStickyRoute> {
        let now = Instant::now();
        let mut routes = self
            .codex_sticky_routes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        routes.retain(|_, (_, last_seen)| now.duration_since(*last_seen) <= ttl);
        let (route, last_seen) = routes.get_mut(key)?;
        *last_seen = now;
        Some(route.clone())
    }

    pub fn remember_codex_sticky_route(&self, key: String, route: CodexStickyRoute, ttl: Duration) {
        let now = Instant::now();
        let mut routes = self
            .codex_sticky_routes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        routes.retain(|_, (_, last_seen)| now.duration_since(*last_seen) <= ttl);
        routes.insert(key, (route, now));
    }

    pub fn forget_codex_sticky_route(&self, key: &str) {
        let mut routes = self
            .codex_sticky_routes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        routes.remove(key);
    }

    /// Add `delta` USD to the running thread cost, evicting expired entries.
    /// Returns the new cumulative cost for this thread_id.
    pub fn add_codex_thread_cost(&self, thread_id: &str, delta: f64) -> f64 {
        let ttl = Duration::from_secs(30 * 60);
        let now = Instant::now();
        let mut map = self
            .codex_thread_costs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.retain(|_, (_, ts)| now.duration_since(*ts) <= ttl);
        let entry = map.entry(thread_id.to_string()).or_insert((0.0, now));
        entry.0 += delta;
        entry.1 = now;
        entry.0
    }

    /// Read cumulative cost for this thread_id without updating the timestamp.
    pub fn get_codex_thread_cost(&self, thread_id: &str) -> f64 {
        let ttl = Duration::from_secs(30 * 60);
        let now = Instant::now();
        let map = self
            .codex_thread_costs
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.get(thread_id)
            .filter(|(_, ts)| now.duration_since(*ts) <= ttl)
            .map(|(cost, _)| *cost)
            .unwrap_or(0.0)
    }

    /// Accumulate `input` and `output` tokens for a turn across all its tool-call requests.
    /// Returns the new (input_sum, output_sum) for this turn.
    pub fn accumulate_codex_turn_io(&self, turn_id: &str, input: i64, output: i64) -> (i64, i64) {
        let ttl = Duration::from_secs(30 * 60);
        let now = Instant::now();
        let mut map = self.codex_turn_io.lock().unwrap_or_else(|p| p.into_inner());
        map.retain(|_, (_, _, ts)| now.duration_since(*ts) <= ttl);
        let entry = map.entry(turn_id.to_string()).or_insert((0, 0, now));
        entry.0 += input;
        entry.1 += output;
        entry.2 = now;
        (entry.0, entry.1)
    }

    /// Read accumulated (input_sum, output_sum) for a turn without modifying it.
    pub fn get_codex_turn_io(&self, turn_id: &str) -> (i64, i64) {
        let ttl = Duration::from_secs(30 * 60);
        let now = Instant::now();
        let map = self.codex_turn_io.lock().unwrap_or_else(|p| p.into_inner());
        map.get(turn_id)
            .filter(|(_, _, ts)| now.duration_since(*ts) <= ttl)
            .map(|(i, o, _)| (*i, *o))
            .unwrap_or((0, 0))
    }
}
