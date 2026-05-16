pub mod doctor;
pub mod gateway;
pub mod install;
mod install_codex_app;
pub mod logs;
pub mod start;
pub mod status;
pub mod statusline;
pub mod stop;
pub mod takeover;
pub mod ui;
pub mod up;
pub mod update;

use anyhow::Result;
use vibe_core::{config::Config, paths};

#[allow(dead_code)]
pub fn proxy_addr(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub const DEFAULT_PORT: u16 = 15917;

pub fn configured_port() -> u16 {
    paths::config_path()
        .ok()
        .and_then(|path| Config::load_or_init(&path).ok())
        .map(|cfg| cfg.server.port)
        .unwrap_or(DEFAULT_PORT)
}

pub fn configured_base_url() -> Result<String> {
    let cfg = Config::load_or_init(&paths::config_path()?)?;
    Ok(format!("http://{}:{}", cfg.server.host, cfg.server.port))
}
