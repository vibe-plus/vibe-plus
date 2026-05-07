pub mod config;
pub mod doctor;
pub mod logs;
pub mod pair;
pub mod provider;
pub mod run;
pub mod start;
pub mod status;
pub mod stop;
pub mod takeover;
pub mod ui;
pub mod update;

#[allow(dead_code)]
pub fn proxy_addr(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub const DEFAULT_PORT: u16 = 15917;
