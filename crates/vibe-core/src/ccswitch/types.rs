//! Data shapes aligned with CC Switch `cc-switch.db` and `settings.json`.
//!
//! Copied from CC Switch desktop (`provider.rs`, `settings.rs`) for read-only extraction.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// CC Switch `providers.app_type` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CcSwitchAppType {
    Claude,
    Codex,
    Gemini,
    Opencode,
    Openclaw,
    Hermes,
}

impl CcSwitchAppType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Openclaw => "openclaw",
            Self::Hermes => "hermes",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "gemini" => Some(Self::Gemini),
            "opencode" => Some(Self::Opencode),
            "openclaw" => Some(Self::Openclaw),
            "hermes" => Some(Self::Hermes),
            _ => None,
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Claude,
        Self::Codex,
        Self::Gemini,
        Self::Opencode,
        Self::Openclaw,
        Self::Hermes,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchCustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

/// Provider row from SQLite + joined endpoints (CC Switch `Provider` + `app_type`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchProvider {
    pub app_type: String,
    pub id: String,
    pub name: String,
    pub settings_config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<CcSwitchProviderMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    #[serde(default)]
    pub in_failover_queue: bool,
    /// `providers.is_current` in DB.
    pub is_current_in_db: bool,
    pub custom_endpoints: Vec<CcSwitchCustomEndpoint>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchProviderMeta {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_endpoints: HashMap<String, CcSwitchCustomEndpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub common_config_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_auto_select: Option<bool>,
    #[serde(rename = "isPartner", skip_serializing_if = "Option::is_none")]
    pub is_partner: Option<bool>,
    #[serde(rename = "apiFormat", skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    #[serde(rename = "providerType", skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(rename = "codexFastMode", skip_serializing_if = "Option::is_none")]
    pub codex_fast_mode: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchVisibleApps {
    #[serde(default = "default_true")]
    pub claude: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
    #[serde(default = "default_true")]
    pub opencode: bool,
    #[serde(default = "default_true")]
    pub openclaw: bool,
    #[serde(default)]
    pub hermes: bool,
}

fn default_true() -> bool {
    true
}

/// Device-level `~/.cc-switch/settings.json` (not synced via WebDAV).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchAppSettings {
    #[serde(default)]
    pub show_in_tray: bool,
    #[serde(default)]
    pub enable_local_proxy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_apps: Option<CcSwitchVisibleApps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_claude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_codex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_gemini: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_opencode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_openclaw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_provider_hermes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
}

impl CcSwitchAppSettings {
    pub fn current_provider_id(&self, app: CcSwitchAppType) -> Option<&str> {
        match app {
            CcSwitchAppType::Claude => self.current_provider_claude.as_deref(),
            CcSwitchAppType::Codex => self.current_provider_codex.as_deref(),
            CcSwitchAppType::Gemini => self.current_provider_gemini.as_deref(),
            CcSwitchAppType::Opencode => self.current_provider_opencode.as_deref(),
            CcSwitchAppType::Openclaw => self.current_provider_openclaw.as_deref(),
            CcSwitchAppType::Hermes => self.current_provider_hermes.as_deref(),
        }
    }
}

/// Local proxy row (`proxy_config` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchProxyConfig {
    pub app_type: String,
    pub proxy_enabled: bool,
    pub enabled: bool,
    pub listen_address: String,
    pub listen_port: u16,
}

/// Full read-only snapshot from CC Switch on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CcSwitchSnapshot {
    pub root: PathBuf,
    pub db_path: PathBuf,
    pub settings_path: PathBuf,
    pub schema_version: i32,
    pub settings: Option<CcSwitchAppSettings>,
    pub providers: Vec<CcSwitchProvider>,
    /// `settings` table key → value (e.g. `common_config_codex`).
    pub db_settings: HashMap<String, String>,
    pub proxy_configs: Vec<CcSwitchProxyConfig>,
    /// Effective current provider per app: `settings.json` overrides DB `is_current`.
    pub effective_current: HashMap<String, String>,
}
