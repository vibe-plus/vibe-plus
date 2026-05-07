//! Process-wide state shared across axum handlers.

use crate::circuit_breaker::CircuitBreakers;
use crate::config::Config;
use crate::ws::WsHub;
use anyhow::Result;
use reqwest::Client;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;
use vibe_db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub http: Client,
    pub config: Arc<Config>,
    pub started_at: Instant,
    pub port: u16,
    pub ws: WsHub,
    pub cb: CircuitBreakers,
    /// Monotonically increasing per-request counter used for round-robin
    /// load balancing among same-priority providers.
    pub lb_counter: Arc<AtomicUsize>,
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
            config: Arc::new(config),
            started_at: Instant::now(),
            port,
            ws: WsHub::new(),
            cb,
            lb_counter: Arc::new(AtomicUsize::new(0)),
        })
    }
}
