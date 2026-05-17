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
pub mod claude_summary;
pub mod codex_auth_json;
pub mod codex_config;
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

/// Dashboard/backend compatibility epoch. Increment when the local Web UI needs
/// gateway APIs that older CLI binaries do not expose.
pub const WEB_COMPAT_API: u32 = 1;

/// Oldest dashboard compatibility epoch this gateway promises to serve.
pub const MIN_WEB_COMPAT_API: u32 = 1;

/// GitHub Pages project-site root (trailing slash). `version.json` and built assets
/// are published here (`VITE_BASE_PATH=/vibe-plus/`).
pub const UI_CDN_BASE_URL: &str = "https://vibe-plus.github.io/vibe-plus/";

/// Alternate CDN mirror for the same project-site layout.
pub const UI_CDN_MIRROR_BASE_URL: &str = "https://vibe-plus.cheez.tech/vibe-plus/";

/// Dashboard SPA entry (deep link; GitHub Pages `404.html` restores the route in-browser).
pub const UI_DASHBOARD_URL: &str = "https://vibe-plus.github.io/vibe-plus/ui/overview";

/// Same dashboard entry on the mirror CDN.
pub const UI_DASHBOARD_MIRROR_URL: &str = "https://vibe-plus.cheez.tech/vibe-plus/ui/overview";

/// Alias kept for asset downloads (`version.json`, dist files).
pub const UI_BASE_URL: &str = UI_CDN_BASE_URL;
