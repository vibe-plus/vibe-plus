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

#[allow(dead_code)]
pub fn proxy_addr(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub const DEFAULT_PORT: u16 = 15917;

pub fn configured_port() -> u16 {
    DEFAULT_PORT
}

pub fn configured_base_url() -> Result<String> {
    Ok(format!("http://127.0.0.1:{DEFAULT_PORT}"))
}
