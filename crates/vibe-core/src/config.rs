//! In-memory runtime defaults for vibe+.
//!
//! There is **no** user-editable config file anymore. Every field below is a
//! hard-coded default that the binary uses at startup. The `Config` struct is
//! still threaded through `AppState` because many call sites read fields like
//! `state.config.failover.inject_cache` — but the struct is constructed once
//! via `Config::default()` and never mutated or
//! persisted to disk.
//!
//! If you need to change one of these knobs, change the constant inline.
//! Reverting to a user-editable file means rebuilding the bug surface the
//! product set fire to in Phase A.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub failover: FailoverConfig,
    pub log: LogConfig,
    #[serde(default)]
    pub codex: CodexConfig,
    #[serde(default)]
    pub claude: ClaudeConfig,
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

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 15917,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// When true, sensitive inbound headers such as Authorization and x-api-key are redacted.
    pub redact_sensitive_headers: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            redact_sensitive_headers: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfig {
    #[serde(default)]
    pub summary: CodexSummaryConfig,
    /// When true, inject the TTFS + upstream routing line near the start of the assistant turn (begin slot).
    #[serde(default = "default_true")]
    pub route_status_enabled: bool,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            summary: CodexSummaryConfig::default(),
            route_status_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub native: ClaudeNativeConfig,
    pub summary: CodexSummaryConfig,
    pub status_line: ClaudeStatusLineConfig,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            native: ClaudeNativeConfig::default(),
            summary: default_claude_summary_config(),
            status_line: ClaudeStatusLineConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeNativeConfig {
    pub manage_settings_json: bool,
    pub proxy_env: bool,
    pub clear_model_overrides_on_takeover: bool,
    pub write_model_overrides_on_takeover: bool,
    pub default_model: Option<String>,
    pub small_fast_model: Option<String>,
    pub haiku_model: Option<String>,
    pub sonnet_model: Option<String>,
    pub opus_model: Option<String>,
    pub max_output_tokens: Option<u32>,
    pub disable_nonessential_traffic: bool,
    pub enable_tool_search: bool,
    pub experimental_agent_teams: bool,
    pub effort: ClaudeNativeEffort,
    pub disable_auto_updater: bool,
    pub hide_attribution: bool,
}

impl Default for ClaudeNativeConfig {
    fn default() -> Self {
        Self {
            manage_settings_json: true,
            proxy_env: true,
            clear_model_overrides_on_takeover: true,
            write_model_overrides_on_takeover: false,
            default_model: None,
            small_fast_model: None,
            haiku_model: None,
            sonnet_model: None,
            opus_model: None,
            max_output_tokens: None,
            disable_nonessential_traffic: false,
            enable_tool_search: false,
            experimental_agent_teams: false,
            effort: ClaudeNativeEffort::Default,
            disable_auto_updater: false,
            hide_attribution: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeNativeEffort {
    #[default]
    Default,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeStatusLineConfig {
    pub enabled: bool,
    pub style: ClaudeStatusLineStyle,
    pub show_provider: bool,
    pub show_model: bool,
    pub show_usage: bool,
}

impl Default for ClaudeStatusLineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            style: ClaudeStatusLineStyle::Compact,
            show_provider: true,
            show_model: true,
            show_usage: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeStatusLineStyle {
    #[default]
    Compact,
    Detailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSummaryConfig {
    pub enabled: bool,
    pub show_speed: bool,
    pub show_input: bool,
    pub show_output: bool,
    pub show_cache: bool,
    pub show_latency: bool,
    pub show_first_token: bool,
    pub show_cost: bool,
    pub show_thread_cost: bool,
    pub speed_decimal_places: u8,
    pub separator: String,
    pub label_overrides: CodexSummaryLabelOverrides,
    pub clients: CodexSummaryClientsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSummaryClientsConfig {
    pub app: CodexSummaryClientConfig,
    pub cli: CodexSummaryClientConfig,
    pub unknown: CodexSummaryClientConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSummaryClientConfig {
    pub enabled: bool,
    pub style: CodexSummaryStyle,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexSummaryStyle {
    FormulaCompact,
    PlainCompact,
    #[default]
    InlineChips,
    StatusBar,
    EnglishLight,
    ChineseLight,
    FormulaLabeled,
    AsciiPlain,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexSummaryLabelOverrides {
    pub speed: Option<String>,
    pub input: Option<String>,
    pub output: Option<String>,
    pub cache: Option<String>,
    pub latency: Option<String>,
    pub first_token: Option<String>,
    pub cost: Option<String>,
    pub thread_cost: Option<String>,
}

impl Default for CodexSummaryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_speed: true,
            show_input: true,
            show_output: true,
            show_cache: true,
            show_latency: false,
            show_first_token: false,
            show_cost: true,
            show_thread_cost: true,
            speed_decimal_places: 1,
            separator: " · ".to_string(),
            label_overrides: CodexSummaryLabelOverrides::default(),
            clients: CodexSummaryClientsConfig::default(),
        }
    }
}

impl Default for CodexSummaryClientsConfig {
    fn default() -> Self {
        Self {
            app: CodexSummaryClientConfig {
                enabled: true,
                style: CodexSummaryStyle::FormulaCompact,
                prefix: None,
                suffix: None,
            },
            cli: CodexSummaryClientConfig {
                enabled: true,
                style: CodexSummaryStyle::PlainCompact,
                prefix: None,
                suffix: None,
            },
            unknown: CodexSummaryClientConfig {
                enabled: true,
                style: CodexSummaryStyle::PlainCompact,
                prefix: None,
                suffix: None,
            },
        }
    }
}

impl Default for CodexSummaryClientConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            style: CodexSummaryStyle::InlineChips,
            prefix: None,
            suffix: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_claude_summary_config() -> CodexSummaryConfig {
    CodexSummaryConfig {
        enabled: true,
        show_speed: true,
        show_input: true,
        show_output: true,
        show_cache: true,
        show_latency: false,
        show_first_token: false,
        show_cost: false,
        show_thread_cost: true,
        speed_decimal_places: 1,
        separator: " · ".to_string(),
        label_overrides: CodexSummaryLabelOverrides::default(),
        clients: CodexSummaryClientsConfig {
            app: CodexSummaryClientConfig {
                enabled: false,
                style: CodexSummaryStyle::FormulaCompact,
                prefix: None,
                suffix: None,
            },
            cli: CodexSummaryClientConfig {
                enabled: true,
                style: CodexSummaryStyle::PlainCompact,
                prefix: None,
                suffix: None,
            },
            unknown: CodexSummaryClientConfig {
                enabled: false,
                style: CodexSummaryStyle::InlineChips,
                prefix: None,
                suffix: None,
            },
        },
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            failover: FailoverConfig::default(),
            log: LogConfig::default(),
            codex: CodexConfig::default(),
            claude: ClaudeConfig::default(),
        }
    }
}
