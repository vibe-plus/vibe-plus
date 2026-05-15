//! User-editable config file at `~/.vibe/config.toml`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Raw request/response bodies are persisted for log inspection. Kept for config compatibility.
    /// Off by default for privacy.
    pub bodies: bool,
    /// When true, sensitive inbound headers such as Authorization and x-api-key are redacted.
    /// Local users can disable this to inspect their own traffic end to end.
    #[serde(default = "default_redact_sensitive_headers")]
    pub redact_sensitive_headers: bool,
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

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeConfig {
    pub native: ClaudeNativeConfig,
    pub summary: CodexSummaryConfig,
    pub routing: ClaudeRoutingConfig,
    pub fallback: ClaudeFallbackConfig,
    pub request: ClaudeRequestConfig,
    pub status_line: ClaudeStatusLineConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeNativeConfig {
    #[serde(default = "default_true")]
    pub manage_settings_json: bool,
    #[serde(default = "default_true")]
    pub proxy_env: bool,
    #[serde(default = "default_true")]
    pub clear_model_overrides_on_takeover: bool,
    #[serde(default)]
    pub write_model_overrides_on_takeover: bool,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub small_fast_model: Option<String>,
    #[serde(default)]
    pub haiku_model: Option<String>,
    #[serde(default)]
    pub sonnet_model: Option<String>,
    #[serde(default)]
    pub opus_model: Option<String>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub disable_nonessential_traffic: bool,
    #[serde(default)]
    pub enable_tool_search: bool,
    #[serde(default)]
    pub experimental_agent_teams: bool,
    #[serde(default)]
    pub effort: ClaudeNativeEffort,
    #[serde(default)]
    pub disable_auto_updater: bool,
    #[serde(default)]
    pub hide_attribution: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeNativeEffort {
    #[default]
    Default,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRoutingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub background_model: String,
    #[serde(default)]
    pub think_model: String,
    #[serde(default)]
    pub long_context_model: String,
    #[serde(default = "default_long_context_threshold_tokens")]
    pub long_context_threshold_tokens: u32,
    #[serde(default)]
    pub web_search_model: String,
    #[serde(default)]
    pub image_model: String,
    #[serde(default = "default_true")]
    pub route_haiku_to_background: bool,
    #[serde(default = "default_true")]
    pub enable_subagent_model_tag: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeFallbackConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub default: Vec<String>,
    #[serde(default)]
    pub background: Vec<String>,
    #[serde(default)]
    pub think: Vec<String>,
    #[serde(default)]
    pub long_context: Vec<String>,
    #[serde(default)]
    pub web_search: Vec<String>,
    #[serde(default)]
    pub image: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRequestConfig {
    #[serde(default = "default_claude_api_timeout_ms")]
    pub api_timeout_ms: u64,
    #[serde(default)]
    pub max_tokens_cap: Option<u32>,
    #[serde(default)]
    pub default_max_tokens: Option<u32>,
    #[serde(default)]
    pub disable_web_search: bool,
    #[serde(default)]
    pub thinking_policy: ClaudeThinkingPolicy,
    #[serde(default)]
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeThinkingPolicy {
    #[default]
    Preserve,
    Remove,
    ForceEnabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeStatusLineConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub style: ClaudeStatusLineStyle,
    #[serde(default = "default_true")]
    pub show_provider: bool,
    #[serde(default = "default_true")]
    pub show_model: bool,
    #[serde(default = "default_true")]
    pub show_usage: bool,
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
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub show_speed: bool,
    #[serde(default = "default_true")]
    pub show_input: bool,
    #[serde(default = "default_true")]
    pub show_output: bool,
    #[serde(default = "default_true")]
    pub show_cache: bool,
    #[serde(default)]
    pub show_latency: bool,
    #[serde(default)]
    pub show_first_token: bool,
    /// Show estimated USD cost per turn. Only accurate for models in the
    /// built-in pricing table; silently omitted for unknown models.
    #[serde(default)]
    pub show_cost: bool,
    /// Show cumulative USD cost for the current thread (all turns).
    #[serde(default = "default_true")]
    pub show_thread_cost: bool,
    #[serde(default = "default_speed_decimal_places")]
    pub speed_decimal_places: u8,
    #[serde(default = "default_summary_separator")]
    pub separator: String,
    #[serde(default)]
    pub label_overrides: CodexSummaryLabelOverrides,
    #[serde(default)]
    pub clients: CodexSummaryClientsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSummaryClientsConfig {
    #[serde(default = "default_app_summary_client")]
    pub app: CodexSummaryClientConfig,
    #[serde(default = "default_cli_summary_client")]
    pub cli: CodexSummaryClientConfig,
    #[serde(default = "default_unknown_summary_client")]
    pub unknown: CodexSummaryClientConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSummaryClientConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub style: CodexSummaryStyle,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub speed: Option<String>,
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub cache: Option<String>,
    #[serde(default)]
    pub latency: Option<String>,
    #[serde(default)]
    pub first_token: Option<String>,
    #[serde(default)]
    pub cost: Option<String>,
    #[serde(default)]
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
            speed_decimal_places: default_speed_decimal_places(),
            separator: default_summary_separator(),
            label_overrides: CodexSummaryLabelOverrides::default(),
            clients: CodexSummaryClientsConfig::default(),
        }
    }
}

impl Default for CodexSummaryClientsConfig {
    fn default() -> Self {
        Self {
            app: default_app_summary_client(),
            cli: default_cli_summary_client(),
            unknown: default_unknown_summary_client(),
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

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            native: ClaudeNativeConfig::default(),
            summary: default_claude_summary_config(),
            routing: ClaudeRoutingConfig::default(),
            fallback: ClaudeFallbackConfig::default(),
            request: ClaudeRequestConfig::default(),
            status_line: ClaudeStatusLineConfig::default(),
        }
    }
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

impl Default for ClaudeRoutingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_model: String::new(),
            background_model: String::new(),
            think_model: String::new(),
            long_context_model: String::new(),
            long_context_threshold_tokens: default_long_context_threshold_tokens(),
            web_search_model: String::new(),
            image_model: String::new(),
            route_haiku_to_background: true,
            enable_subagent_model_tag: true,
        }
    }
}

impl Default for ClaudeFallbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default: Vec::new(),
            background: Vec::new(),
            think: Vec::new(),
            long_context: Vec::new(),
            web_search: Vec::new(),
            image: Vec::new(),
        }
    }
}

impl Default for ClaudeRequestConfig {
    fn default() -> Self {
        Self {
            api_timeout_ms: default_claude_api_timeout_ms(),
            max_tokens_cap: None,
            default_max_tokens: None,
            disable_web_search: false,
            thinking_policy: ClaudeThinkingPolicy::Preserve,
            thinking_budget_tokens: None,
        }
    }
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

impl<'de> Deserialize<'de> for ClaudeConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Default, Deserialize)]
        struct RawClaudeConfig {
            native: Option<ClaudeNativeConfig>,
            summary: Option<RawSummaryConfig>,
            routing: Option<ClaudeRoutingConfig>,
            fallback: Option<ClaudeFallbackConfig>,
            request: Option<ClaudeRequestConfig>,
            status_line: Option<ClaudeStatusLineConfig>,
        }

        #[derive(Default, Deserialize)]
        struct RawSummaryConfig {
            enabled: Option<bool>,
            show_speed: Option<bool>,
            show_input: Option<bool>,
            show_output: Option<bool>,
            show_cache: Option<bool>,
            show_latency: Option<bool>,
            speed_decimal_places: Option<u8>,
            clients: Option<RawSummaryClientsConfig>,
        }

        #[derive(Default, Deserialize)]
        struct RawSummaryClientsConfig {
            app: Option<RawSummaryClientConfig>,
            cli: Option<RawSummaryClientConfig>,
            unknown: Option<RawSummaryClientConfig>,
        }

        #[derive(Default, Deserialize)]
        struct RawSummaryClientConfig {
            enabled: Option<bool>,
            style: Option<CodexSummaryStyle>,
        }

        fn merge_client(
            mut base: CodexSummaryClientConfig,
            raw: Option<RawSummaryClientConfig>,
        ) -> CodexSummaryClientConfig {
            if let Some(raw) = raw {
                if let Some(enabled) = raw.enabled {
                    base.enabled = enabled;
                }
                if let Some(style) = raw.style {
                    base.style = style;
                }
            }
            base
        }

        let raw = RawClaudeConfig::deserialize(deserializer)?;
        let mut summary = default_claude_summary_config();
        if let Some(raw_summary) = raw.summary {
            if let Some(enabled) = raw_summary.enabled {
                summary.enabled = enabled;
            }
            if let Some(show_speed) = raw_summary.show_speed {
                summary.show_speed = show_speed;
            }
            if let Some(show_input) = raw_summary.show_input {
                summary.show_input = show_input;
            }
            if let Some(show_output) = raw_summary.show_output {
                summary.show_output = show_output;
            }
            if let Some(show_cache) = raw_summary.show_cache {
                summary.show_cache = show_cache;
            }
            if let Some(show_latency) = raw_summary.show_latency {
                summary.show_latency = show_latency;
            }
            if let Some(speed_decimal_places) = raw_summary.speed_decimal_places {
                summary.speed_decimal_places = speed_decimal_places;
            }
            if let Some(clients) = raw_summary.clients {
                summary.clients.app = merge_client(summary.clients.app, clients.app);
                summary.clients.cli = merge_client(summary.clients.cli, clients.cli);
                summary.clients.unknown = merge_client(summary.clients.unknown, clients.unknown);
            }
        }
        Ok(Self {
            native: raw.native.unwrap_or_default(),
            summary,
            routing: raw.routing.unwrap_or_default(),
            fallback: raw.fallback.unwrap_or_default(),
            request: raw.request.unwrap_or_default(),
            status_line: raw.status_line.unwrap_or_default(),
        })
    }
}

fn default_redact_sensitive_headers() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_speed_decimal_places() -> u8 {
    1
}

fn default_summary_separator() -> String {
    " · ".to_string()
}

fn default_long_context_threshold_tokens() -> u32 {
    60_000
}

fn default_claude_api_timeout_ms() -> u64 {
    600_000
}

fn default_app_summary_client() -> CodexSummaryClientConfig {
    CodexSummaryClientConfig {
        enabled: true,
        style: CodexSummaryStyle::FormulaCompact,
        prefix: None,
        suffix: None,
    }
}

fn default_cli_summary_client() -> CodexSummaryClientConfig {
    CodexSummaryClientConfig {
        enabled: true,
        style: CodexSummaryStyle::PlainCompact,
        prefix: None,
        suffix: None,
    }
}

fn default_unknown_summary_client() -> CodexSummaryClientConfig {
    CodexSummaryClientConfig {
        enabled: false,
        style: CodexSummaryStyle::InlineChips,
        prefix: None,
        suffix: None,
    }
}

fn default_disabled_app_summary_client() -> CodexSummaryClientConfig {
    CodexSummaryClientConfig {
        enabled: false,
        style: CodexSummaryStyle::FormulaCompact,
        prefix: None,
        suffix: None,
    }
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
        speed_decimal_places: default_speed_decimal_places(),
        separator: default_summary_separator(),
        label_overrides: CodexSummaryLabelOverrides::default(),
        clients: CodexSummaryClientsConfig {
            app: default_disabled_app_summary_client(),
            cli: default_cli_summary_client(),
            unknown: default_unknown_summary_client(),
        },
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 15917,
            },
            failover: FailoverConfig::default(),
            log: LogConfig {
                bodies: true,
                redact_sensitive_headers: true,
            },
            codex: CodexConfig::default(),
            claude: ClaudeConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_without_codex_gets_default_summary() {
        let cfg: Config = toml::from_str(
            r#"
[server]
host = "127.0.0.1"
port = 15917

[failover]
failure_threshold = 3
success_threshold = 2
open_timeout_secs = 30
inject_cache = true

[log]
bodies = true
"#,
        )
        .unwrap();

        assert!(cfg.codex.route_status_enabled);
        assert!(cfg.codex.summary.enabled);
        assert_eq!(
            cfg.codex.summary.clients.app.style,
            CodexSummaryStyle::FormulaCompact
        );
        assert_eq!(
            cfg.codex.summary.clients.cli.style,
            CodexSummaryStyle::PlainCompact
        );
        assert!(!cfg.codex.summary.clients.unknown.enabled);
        assert!(cfg.claude.summary.enabled);
        assert!(!cfg.claude.summary.clients.app.enabled);
        assert!(cfg.claude.summary.clients.cli.enabled);
        assert_eq!(
            cfg.claude.summary.clients.cli.style,
            CodexSummaryStyle::PlainCompact
        );
    }

    #[test]
    fn tool_summary_round_trips_through_toml() {
        let cfg: Config = toml::from_str(
            r#"
[server]
host = "127.0.0.1"
port = 15917

[failover]
failure_threshold = 3
success_threshold = 2
open_timeout_secs = 30
inject_cache = true

[log]
bodies = true
redact_sensitive_headers = true

[codex]
route_status_enabled = false

[codex.summary]
enabled = true
show_speed = true
show_input = false
show_output = true
show_cache = true
show_latency = true
speed_decimal_places = 2

[codex.summary.clients.app]
enabled = true
style = "formula_labeled"

[codex.summary.clients.cli]
enabled = false
style = "ascii_plain"

[codex.summary.clients.unknown]
enabled = true
style = "inline_chips"

[claude.summary]
enabled = true
show_speed = false
show_input = true
show_output = true
show_cache = false
show_latency = true
speed_decimal_places = 0

[claude.summary.clients.app]
enabled = false
style = "formula_compact"

[claude.summary.clients.cli]
enabled = true
style = "english_light"

[claude.summary.clients.unknown]
enabled = false
style = "ascii_plain"
"#,
        )
        .unwrap();

        assert!(!cfg.codex.route_status_enabled);
        let out = toml::to_string_pretty(&cfg).unwrap();
        let reparsed: Config = toml::from_str(&out).unwrap();
        assert!(!reparsed.codex.route_status_enabled);
        assert!(!reparsed.codex.summary.show_input);
        assert!(reparsed.codex.summary.show_latency);
        assert_eq!(reparsed.codex.summary.speed_decimal_places, 2);
        assert_eq!(
            reparsed.codex.summary.clients.app.style,
            CodexSummaryStyle::FormulaLabeled
        );
        assert!(!reparsed.codex.summary.clients.cli.enabled);
        assert_eq!(
            reparsed.codex.summary.clients.cli.style,
            CodexSummaryStyle::AsciiPlain
        );
        assert!(reparsed.codex.summary.clients.unknown.enabled);
        assert!(!reparsed.claude.summary.show_speed);
        assert!(reparsed.claude.summary.show_latency);
        assert_eq!(reparsed.claude.summary.speed_decimal_places, 0);
        assert!(!reparsed.claude.summary.clients.app.enabled);
        assert_eq!(
            reparsed.claude.summary.clients.cli.style,
            CodexSummaryStyle::EnglishLight
        );
        assert_eq!(
            reparsed.claude.summary.clients.unknown.style,
            CodexSummaryStyle::AsciiPlain
        );
    }

    #[test]
    fn partial_claude_summary_keeps_claude_client_defaults() {
        let cfg: Config = toml::from_str(
            r#"
[server]
host = "127.0.0.1"
port = 15917

[failover]
failure_threshold = 3
success_threshold = 2
open_timeout_secs = 30
inject_cache = true

[log]
bodies = true

[claude.summary]
show_latency = true
"#,
        )
        .unwrap();

        assert!(cfg.claude.summary.show_latency);
        assert!(!cfg.claude.summary.clients.app.enabled);
        assert!(cfg.claude.summary.clients.cli.enabled);
        assert!(!cfg.claude.summary.clients.unknown.enabled);
    }

    #[test]
    fn claude_control_defaults_and_round_trip() {
        let cfg: Config = toml::from_str(
            r#"
[server]
host = "127.0.0.1"
port = 15917

[failover]
failure_threshold = 3
success_threshold = 2
open_timeout_secs = 30
inject_cache = true

[log]
bodies = true

[claude.native]
manage_settings_json = true
write_model_overrides_on_takeover = true
default_model = "claude-sonnet"
max_output_tokens = 8192
enable_tool_search = true
effort = "max"
hide_attribution = true

[claude.routing]
think_model = "Anthropic,claude-opus"
long_context_threshold_tokens = 90000

[claude.fallback]
think = ["Anthropic,claude-sonnet", "Backup,claude"]

[claude.request]
api_timeout_ms = 120000
max_tokens_cap = 8192
thinking_policy = "force_enabled"
thinking_budget_tokens = 4096

[claude.status_line]
enabled = true
style = "detailed"
"#,
        )
        .unwrap();

        assert!(cfg.claude.native.manage_settings_json);
        assert!(cfg.claude.native.write_model_overrides_on_takeover);
        assert_eq!(
            cfg.claude.native.default_model.as_deref(),
            Some("claude-sonnet")
        );
        assert_eq!(cfg.claude.native.max_output_tokens, Some(8192));
        assert!(cfg.claude.native.enable_tool_search);
        assert_eq!(cfg.claude.native.effort, ClaudeNativeEffort::Max);
        assert!(cfg.claude.native.hide_attribution);
        assert!(cfg.claude.routing.enabled);
        assert_eq!(cfg.claude.routing.think_model, "Anthropic,claude-opus");
        assert_eq!(cfg.claude.routing.long_context_threshold_tokens, 90_000);
        assert_eq!(cfg.claude.fallback.think.len(), 2);
        assert_eq!(cfg.claude.request.api_timeout_ms, 120_000);
        assert_eq!(cfg.claude.request.max_tokens_cap, Some(8192));
        assert_eq!(
            cfg.claude.request.thinking_policy,
            ClaudeThinkingPolicy::ForceEnabled
        );
        assert!(cfg.claude.status_line.enabled);
        assert_eq!(
            cfg.claude.status_line.style,
            ClaudeStatusLineStyle::Detailed
        );

        let out = toml::to_string_pretty(&cfg).unwrap();
        let reparsed: Config = toml::from_str(&out).unwrap();
        assert!(reparsed.claude.native.write_model_overrides_on_takeover);
        assert_eq!(reparsed.claude.native.max_output_tokens, Some(8192));
        assert_eq!(reparsed.claude.routing.think_model, "Anthropic,claude-opus");
        assert_eq!(reparsed.claude.fallback.think.len(), 2);
        assert_eq!(reparsed.claude.request.thinking_budget_tokens, Some(4096));
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
