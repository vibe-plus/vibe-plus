//! vibe-core: local API gateway runtime.
//!
//! Owns the axum HTTP server, provider adapters, router, request forwarding,
//! usage extraction, and the embedded Vue dashboard.
//!
//! Public entry points:
//! - [`AppState::init`] — wire up DB + HTTP client + config
//! - [`server::serve`] — start the axum app on a TcpListener

pub mod cache;
pub mod circuit_breaker;
pub mod config;
pub mod embedded;
pub mod forward;
pub mod paths;
pub mod providers;
pub mod router;
pub mod secrets;
pub mod server;
pub mod state;
pub mod transforms;
pub mod usage;
pub mod ws;

pub use state::AppState;

/// Crate version, exposed in `/status` and `WsEvent::Hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
