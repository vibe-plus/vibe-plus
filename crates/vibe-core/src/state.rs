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
use vibe_protocol::{AppLogEvent, LogPage, RequestLog, UpstreamAttemptLog};


const MAX_IN_MEMORY_REQUEST_LOGS: usize = 2_000;
const MAX_IN_MEMORY_APP_LOGS: usize = 500;
const MAX_IN_MEMORY_ATTEMPT_LOGS: usize = 4_000;

#[derive(Default)]
pub struct InMemoryRequestLogs {
    inner: Mutex<VecDeque<RequestLog>>,
}

impl InMemoryRequestLogs {
    pub fn push(&self, mut log: RequestLog) {
        // Never retain raw network payloads in process memory. The overview only needs summary fields.
        log.request_body = None;
        log.response_body = None;
        log.client_response_body = None;
        log.request_headers = None;
        log.response_headers = None;
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pos) = inner.iter().position(|row| row.id == log.id) {
            inner.remove(pos);
        }
        inner.push_front(log);
        while inner.len() > MAX_IN_MEMORY_REQUEST_LOGS {
            inner.pop_back();
        }
    }

    pub fn list(
        &self,
        limit: i64,
        offset: i64,
        since: Option<i64>,
        provider_id: Option<&str>,
        status_ok: Option<bool>,
    ) -> LogPage {
        let limit = limit.clamp(1, 500);
        let offset = offset.max(0) as usize;
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut filtered = inner
            .iter()
            .filter(|log| since.is_none_or(|ts| log.started_at >= ts))
            .filter(|log| provider_id.is_none_or(|pid| log.provider_id.as_deref() == Some(pid)))
            .filter(|log| match status_ok {
                Some(true) => log.status_code.is_some_and(|code| (200..300).contains(&code)),
                Some(false) => log.status_code.is_none_or(|code| code >= 400),
                None => true,
            })
            .skip(offset)
            .take(limit as usize + 1)
            .cloned()
            .collect::<Vec<_>>();
        let has_more = filtered.len() > limit as usize;
        if has_more {
            filtered.pop();
        }
        LogPage {
            total: offset as i64 + filtered.len() as i64,
            items: filtered,
            limit,
            offset: offset as i64,
            has_more,
        }
    }

    pub fn get(&self, id: &str) -> Option<RequestLog> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|log| log.id == id)
            .cloned()
    }

    pub fn count_since(&self, since: i64) -> i64 {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|log| log.started_at >= since)
            .count() as i64
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

#[derive(Default)]
pub struct InMemoryAppLogs {
    inner: Mutex<VecDeque<AppLogEvent>>,
}

impl InMemoryAppLogs {
    pub fn push(&self, event: AppLogEvent) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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

#[derive(Default)]
pub struct InMemoryAttemptLogs {
    inner: Mutex<VecDeque<UpstreamAttemptLog>>,
}

impl InMemoryAttemptLogs {
    pub fn push(&self, mut attempt: UpstreamAttemptLog) {
        attempt.request_body = None;
        attempt.response_body = None;
        attempt.request_headers = None;
        attempt.response_headers = None;
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pos) = inner.iter().position(|row| row.attempt_id == attempt.attempt_id) {
            inner.remove(pos);
        }
        inner.push_front(attempt);
        while inner.len() > MAX_IN_MEMORY_ATTEMPT_LOGS {
            inner.pop_back();
        }
    }

    pub fn get(&self, attempt_id: &str) -> Option<UpstreamAttemptLog> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|attempt| attempt.attempt_id == attempt_id)
            .cloned()
    }

    pub fn for_request(&self, request_id: &str) -> Vec<UpstreamAttemptLog> {
        let mut rows = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|attempt| attempt.request_id == request_id)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|attempt| attempt.attempt_index);
        rows
    }

    pub fn list(&self, limit: i64, offset: i64) -> Vec<UpstreamAttemptLog> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .skip(offset.max(0) as usize)
            .take(limit.clamp(1, 500) as usize)
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
    pub request_logs: Arc<InMemoryRequestLogs>,
    pub app_logs: Arc<InMemoryAppLogs>,
    pub upstream_attempt_logs: Arc<InMemoryAttemptLogs>,
    codex_summary_config: Arc<Mutex<CodexSummaryConfig>>,
    /// Mirrors `codex.route_status_enabled` from disk; updated on config GET/PUT.
    codex_route_status_on: Arc<AtomicBool>,
    claude_summary_config: Arc<Mutex<CodexSummaryConfig>>,
    claude_config: Arc<Mutex<ClaudeConfig>>,
}

impl AppState {
    pub fn init(db: Db, config: Config, port: u16) -> Result<Self> {
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
            request_logs: Arc::new(InMemoryRequestLogs::default()),
            app_logs: Arc::new(InMemoryAppLogs::default()),
            upstream_attempt_logs: Arc::new(InMemoryAttemptLogs::default()),
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
        let mut map = self
            .codex_turn_io
            .lock()
            .unwrap_or_else(|p| p.into_inner());
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
        let map = self
            .codex_turn_io
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        map.get(turn_id)
            .filter(|(_, _, ts)| now.duration_since(*ts) <= ttl)
            .map(|(i, o, _)| (*i, *o))
            .unwrap_or((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_log(id: usize) -> RequestLog {
        let mut log = RequestLog {
            id: format!("log-{id}"),
            started_at: id as i64,
            app: None,
            provider_id: Some("provider-a".into()),
            requested_model: Some("gpt-test".into()),
            upstream_model: None,
            status_code: Some(200),
            error: None,
            latency_ms: Some(42),
            first_token_ms: Some(10),
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            estimated_cost_usd: "0".into(),
            wire: None,
            route_prefix: None,
            credential_id: None,
            cb_key: None,
            upstream_http_status: None,
            upstream_error_preview: None,
            dedupe_key: None,
            client_transport: None,
            request_headers: Some("secret headers".into()),
            request_body: Some("secret request".into()),
            response_headers: Some("secret response headers".into()),
            response_body: Some("secret response".into()),
            client_response_body: Some("secret client response".into()),
            stream_kind: None,
            stream_terminal_seen: None,
            stream_end_reason: None,
            stream_error_detail: None,
            upstream_first_byte_ms: None,
            client_first_write_ms: None,
            last_upstream_event_ms: None,
            last_client_write_ms: None,
            upstream_chunk_count: 0,
            upstream_bytes: 0,
            client_chunk_count: 0,
            client_bytes: 0,
            sse_event_count: 0,
            sse_data_count: 0,
            sse_comment_count: 0,
            sse_keepalive_count: 0,
            sse_done_count: 0,
            parse_error_count: 0,
            first_keepalive_ms: None,
            last_keepalive_ms: None,
            max_gap_between_upstream_events_ms: None,
            max_gap_between_data_events_ms: None,
            keepalive_after_last_data_count: 0,
            last_data_event_ms: None,
            bridge_mode: None,
            status_injected: false,
            terminal_injected: false,
            upstream_terminal_type: None,
        };
        // Keep this assignment so the test fails if sanitization is removed.
        log.request_body = Some("must-not-survive".into());
        log
    }

    #[test]
    fn in_memory_logs_are_bounded_and_strip_network_payloads_quickly() {
        let logs = InMemoryRequestLogs::default();
        let started = Instant::now();
        for i in 0..2_500 {
            logs.push(test_log(i));
        }
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(logs.len(), MAX_IN_MEMORY_REQUEST_LOGS);
        let latest = logs.get("log-2499").expect("latest log");
        assert!(latest.request_headers.is_none());
        assert!(latest.request_body.is_none());
        assert!(latest.response_headers.is_none());
        assert!(latest.response_body.is_none());
        assert!(latest.client_response_body.is_none());
    }
}
