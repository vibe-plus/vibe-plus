//! vibe-core: local API gateway runtime.
//!
//! Owns the axum HTTP server, provider adapters, router, request forwarding,
//! and usage extraction.
//!
//! Public entry points:
//! - [`AppState::init`] — wire up DB + HTTP client + config
//! - [`server::serve`] — start the axum app on a TcpListener

pub mod auth_fingerprint;
pub mod cache;
pub mod circuit_breaker;
pub mod claude_control;
pub mod claude_summary;
pub mod codex_auth_json;
pub mod codex_config;
pub mod codex_history;
pub mod codex_plan_headers;
pub mod codex_summary;
pub mod codex_upstream_ws;
pub mod codex_visual;
pub mod codex_wham_usage;
pub mod config;
pub mod forward;
pub mod intake;
pub mod local_import;
pub mod model_defaults;
pub mod oauth_identity;
pub mod paths;
pub mod providers;
pub mod router;
pub mod secrets;
pub mod server;
pub mod state;
pub mod stream_trace;
pub mod takeover;
pub mod transforms;
pub mod usage;
pub mod ws;

pub use state::AppState;

/// Crate version, exposed in `/status` and `WsEvent::Hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
