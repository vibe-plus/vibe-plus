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
pub mod codex_auth_json;
pub mod codex_plan_headers;
pub mod codex_wham_usage;
pub mod config;
pub mod forward;
pub mod paths;
pub mod providers;
pub mod router;
pub mod local_import;
pub mod model_defaults;
pub mod oauth_identity;
pub mod secrets;
pub mod server;
pub mod state;
pub mod transforms;
pub mod usage;
pub mod ws;

pub use state::AppState;

/// Crate version, exposed in `/status` and `WsEvent::Hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
