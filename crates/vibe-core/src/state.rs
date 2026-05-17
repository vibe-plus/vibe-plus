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
use vibe_protocol::AppLogEvent;

const MAX_IN_MEMORY_APP_LOGS: usize = 500;

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
            app_logs: Arc::new(InMemoryAppLogs::default()),
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
