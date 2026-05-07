//! User-editable config file at `~/.vibe/config.toml`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub failover: FailoverConfig,
    pub log: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    /// Number of consecutive failures before opening a circuit.
    pub failure_threshold: u32,
    /// Successes in half-open state needed to close the circuit.
    pub success_threshold: u32,
    /// Seconds to wait in open state before probing again.
    pub open_timeout_secs: u64,
    /// Whether to automatically inject Anthropic cache_control on requests.
    pub inject_cache: bool,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            success_threshold: 2,
            open_timeout_secs: 30,
            inject_cache: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// When true, request/response bodies are stored in memory for `/_vp/logs/:id/body`.
    /// Off by default for privacy.
    pub bodies: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 15917,
            },
            failover: FailoverConfig::default(),
            log: LogConfig { bodies: false },
        }
    }
}

impl Config {
    pub fn load_or_init(path: &Path) -> Result<Self> {
        if path.exists() {
            let s = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&s)?)
        } else {
            let cfg = Self::default();
            cfg.save(path)?;
            Ok(cfg)
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).ok();
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}
