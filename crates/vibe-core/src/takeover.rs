use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;

use crate::{
    config::{ClaudeNativeConfig, ClaudeNativeEffort, ClaudeStatusLineConfig},
    paths,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct TakeoverOutcome {
    pub client: String,
    pub config_path: String,
    pub backup_path: Option<String>,
}

pub fn takeover(client: &str, base_url: &str) -> Result<TakeoverOutcome> {
    let cfg_path = detect_config_path(client)?;
    let backup = if cfg_path.exists() {
        let backup = backup_path(client)?;
        std::fs::copy(&cfg_path, &backup)?;
        Some(backup)
    } else {
        None
    };

    patch_config(client, &cfg_path, base_url)?;

    Ok(TakeoverOutcome {
        client: client.into(),
        config_path: cfg_path.display().to_string(),
        backup_path: backup.map(|p| p.display().to_string()),
    })
}

pub fn restore(client: &str) -> Result<TakeoverOutcome> {
    let cfg_path = detect_config_path(client)?;
    let backup = latest_backup(client)?;
    if let Some(backup) = &backup {
        std::fs::copy(backup, &cfg_path).with_context(|| {
            format!("restoring {} from {}", cfg_path.display(), backup.display())
        })?;
    }
    disable_takeover(client, &cfg_path)?;

    Ok(TakeoverOutcome {
        client: client.into(),
        config_path: cfg_path.display().to_string(),
        backup_path: backup.map(|p| p.display().to_string()),
    })
}

fn backup_path(client: &str) -> Result<PathBuf> {
    let dir = paths::backups_dir()?.join("takeover");
    std::fs::create_dir_all(&dir)?;
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    Ok(dir.join(format!("{client}.{ts}.bak")))
}

fn latest_backup(client: &str) -> Result<Option<PathBuf>> {
    let base = paths::backups_dir()?;
    let dirs = [base.join("takeover"), base];
    let mut backups = Vec::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| backup_name_matches(client, name))
            {
                backups.push(path);
            }
        }
    }
    backups.sort();
    Ok(backups.pop())
}

fn backup_name_matches(client: &str, name: &str) -> bool {
    let new_prefix = format!("{client}.");
    if name.starts_with(&new_prefix) && name.ends_with(".bak") {
        return true;
    }

    let legacy_prefix = format!("{client}-");
    if !name.starts_with(&legacy_prefix) {
        return false;
    }
    match client {
        "codex" => name.ends_with(".bak.toml"),
        "claude" | "opencode" => name.ends_with(".bak.json"),
        _ => false,
    }
}

fn disable_takeover(client: &str, path: &PathBuf) -> Result<()> {
    match client {
        "claude" => disable_claude_takeover(path),
        "codex" => disable_codex_takeover(path),
        "opencode" => disable_opencode_takeover(path),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Config path detection
// ---------------------------------------------------------------------------

pub(crate) fn detect_config_path(client: &str) -> Result<PathBuf> {
    match client {
        "claude" => {
            // Claude Code reads $CLAUDE_CONFIG_DIR/settings.json (default ~/.claude/settings.json)
            // and injects its `env` block as environment variables before starting.
            paths::claude_settings_path()
        }
        "opencode" => {
            // OpenCode's canonical user-override file is
            // $XDG_CONFIG_HOME/opencode/opencode.json (default ~/.config/opencode/opencode.json).
            // config.json is OpenCode's own managed/generated file; we do not write there.
            paths::opencode_config_path()
        }
        "codex" => {
            // Codex CLI primary config is $CODEX_HOME/config.toml when CODEX_HOME is set.
            // Otherwise we prefer ~/.codex/config.toml and fall back to
            // $XDG_CONFIG_HOME/codex/config.toml.
            paths::codex_config_path()
        }
        other => anyhow::bail!("unknown client: {other}. Supported: claude, opencode, codex"),
    }
}

// ---------------------------------------------------------------------------
// Config patching
// ---------------------------------------------------------------------------

fn patch_config(client: &str, path: &PathBuf, base_url: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match client {
        "claude" => patch_claude_settings(path, base_url),
        "opencode" => patch_opencode_config(path, base_url),
        "codex" => patch_codex_config(path, base_url),
        _ => Ok(()),
    }
}

// Model-tier env var keys that third-party proxies (Mimo, etc.) inject and that
// we must clear when taking over, so Claude Code uses our proxy's model pool
// instead of hardcoded upstream-specific model names.
const CLAUDE_MODEL_OVERRIDES: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
];

/// Patch ~/.claude/settings.json:
/// - Set env.ANTHROPIC_BASE_URL to our proxy
/// - Set env.ANTHROPIC_AUTH_TOKEN to "PROXY_MANAGED" (proxy handles auth)
/// - Remove any hardcoded model overrides from prior proxies (Mimo, etc.)
///   so Claude Code uses its built-in defaults, which route through us.
fn patch_claude_settings(path: &PathBuf, base_url: &str) -> Result<()> {
    // takeover defaults are no longer user-configurable; they live in the
    // hard-coded `ClaudeNativeConfig::default()` block.
    let cfg = crate::config::Config::default();
    let native = &cfg.claude.native;
    if !native.manage_settings_json {
        return Ok(());
    }

    let mut v: Value = if path.exists() {
        let s = std::fs::read_to_string(path)?;
        if s.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display()))?
        }
    } else {
        Value::Object(Default::default())
    };

    let obj = v
        .as_object_mut()
        .context("settings.json root must be an object")?;
    let env = obj
        .entry("env")
        .or_insert_with(|| Value::Object(Default::default()));
    let env_obj = env
        .as_object_mut()
        .context("settings.json env must be an object")?;

    apply_claude_native_env(env_obj, native, base_url);
    apply_claude_status_line(obj, &cfg.claude.status_line);

    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

fn disable_claude_takeover(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(path)?;
    let mut v: Value = if raw.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
    };
    let Some(obj) = v.as_object_mut() else {
        return Ok(());
    };
    if let Some(env_obj) = obj.get_mut("env").and_then(|env| env.as_object_mut()) {
        if env_obj
            .get("ANTHROPIC_BASE_URL")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value.contains("/claude"))
        {
            env_obj.remove("ANTHROPIC_BASE_URL");
        }
        if env_obj
            .get("ANTHROPIC_AUTH_TOKEN")
            .and_then(|value| value.as_str())
            == Some("PROXY_MANAGED")
        {
            env_obj.remove("ANTHROPIC_AUTH_TOKEN");
        }
        if env_obj
            .get("ANTHROPIC_API_KEY")
            .and_then(|value| value.as_str())
            == Some("PROXY_MANAGED")
        {
            env_obj.remove("ANTHROPIC_API_KEY");
        }
    }
    if obj
        .get("statusLine")
        .and_then(|value| value.as_object())
        .and_then(|status| status.get("command"))
        .and_then(|value| value.as_str())
        == Some("vibe statusline")
    {
        obj.remove("statusLine");
    }
    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

fn apply_claude_native_env(
    env_obj: &mut serde_json::Map<String, Value>,
    native: &ClaudeNativeConfig,
    base_url: &str,
) {
    if native.proxy_env {
        // Use the /claude tool prefix: Claude SDK calls /claude/v1/messages and /claude/v1/models.
        // Vibe uses that prefix to select Anthropic providers and return Anthropic-shaped models.
        env_obj.insert(
            "ANTHROPIC_BASE_URL".into(),
            Value::String(format!("{base_url}/claude")),
        );
        // Replace the real token with a placeholder; the proxy handles authentication
        // via its own credential pool, so the incoming token is irrelevant.
        env_obj.insert(
            "ANTHROPIC_AUTH_TOKEN".into(),
            Value::String("PROXY_MANAGED".into()),
        );
        env_obj.remove("ANTHROPIC_API_KEY");
        env_obj.insert(
            "NO_PROXY".into(),
            Value::String("127.0.0.1,localhost".into()),
        );
    }

    if native.clear_model_overrides_on_takeover {
        for key in CLAUDE_MODEL_OVERRIDES {
            env_obj.remove(*key);
        }
    }
    if native.write_model_overrides_on_takeover {
        set_optional_env(env_obj, "ANTHROPIC_MODEL", native.default_model.as_deref());
        set_optional_env(
            env_obj,
            "ANTHROPIC_SMALL_FAST_MODEL",
            native.small_fast_model.as_deref(),
        );
        set_optional_env(
            env_obj,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            native.haiku_model.as_deref(),
        );
        set_optional_env(
            env_obj,
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            native.sonnet_model.as_deref(),
        );
        set_optional_env(
            env_obj,
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            native.opus_model.as_deref(),
        );
    }

    set_optional_env(
        env_obj,
        "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
        native.max_output_tokens.map(|n| n.to_string()).as_deref(),
    );
    set_bool_env(
        env_obj,
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        native.disable_nonessential_traffic,
        "1",
    );
    set_bool_env(
        env_obj,
        "ENABLE_TOOL_SEARCH",
        native.enable_tool_search,
        "true",
    );
    set_bool_env(
        env_obj,
        "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
        native.experimental_agent_teams,
        "1",
    );
    set_bool_env(
        env_obj,
        "DISABLE_AUTOUPDATER",
        native.disable_auto_updater,
        "1",
    );
    match native.effort {
        ClaudeNativeEffort::Default => {
            env_obj.remove("CLAUDE_CODE_EFFORT_LEVEL");
        }
        ClaudeNativeEffort::Max => {
            env_obj.insert(
                "CLAUDE_CODE_EFFORT_LEVEL".into(),
                Value::String("max".into()),
            );
        }
    }
}

fn apply_claude_status_line(
    obj: &mut serde_json::Map<String, Value>,
    status_line: &ClaudeStatusLineConfig,
) {
    if status_line.enabled {
        obj.insert(
            "statusLine".into(),
            serde_json::json!({
                "type": "command",
                "command": "vibe statusline",
                "padding": 0
            }),
        );
    } else {
        obj.remove("statusLine");
    }
}

fn set_optional_env(env_obj: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        Some(value) => {
            env_obj.insert(key.into(), Value::String(value.into()));
        }
        None => {
            env_obj.remove(key);
        }
    }
}

fn set_bool_env(
    env_obj: &mut serde_json::Map<String, Value>,
    key: &str,
    enabled: bool,
    enabled_value: &str,
) {
    if enabled {
        env_obj.insert(key.into(), Value::String(enabled_value.into()));
    } else {
        env_obj.remove(key);
    }
}

/// Patch ~/.config/opencode/opencode.json.
///
/// OpenCode's config schema (packages/opencode/src/config/provider.ts) requires custom
/// providers to be declared under the `provider` key as an AI SDK provider record:
///   { "provider": { "<id>": { "npm": "...", "options": { "baseURL": "...", "apiKey": "..." }, "models": {} } } }
///
/// A top-level `baseURL` or `baseUrl` key is NOT part of the schema and causes
/// "Unrecognized key" errors. We also clean up any legacy stale keys we may have
/// written in older vibe versions.
///
/// Additionally, we clean config.json of any stale `baseUrl`/`baseURL` keys that
/// prior takeover versions may have incorrectly written there.
fn patch_opencode_config(path: &PathBuf, base_url: &str) -> Result<()> {
    // Write to opencode.json (user override layer).
    let mut v: Value = if path.exists() {
        let s = std::fs::read_to_string(path)?;
        if s.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display()))?
        }
    } else {
        Value::Object(Default::default())
    };

    let obj = v
        .as_object_mut()
        .context("opencode.json root must be an object")?;

    // Remove any stale top-level keys from old vibe takeover versions.
    obj.remove("baseURL");
    obj.remove("baseUrl");

    // Ensure $schema is present (matches OpenCode's own write logic).
    obj.entry("$schema")
        .or_insert_with(|| Value::String("https://opencode.ai/config.json".into()));

    // Upsert provider.vibe entry.
    //
    // OpenCode does NOT auto-fetch /v1/models; it reads exclusively from
    // `provider.<id>.models`. An empty `models: {}` means no models exist
    // and the provider is silently skipped. We must list models explicitly.
    //
    // Empty model objects `{}` are fine: OpenCode fills in capability defaults.
    // We register the most common OpenAI-compat models so the user can pick
    // immediately without extra configuration.
    let provider_entry = serde_json::json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": "vibe+",
        "options": {
            "baseURL": format!("{base_url}/opencode/v1"),
            "apiKey": "PROXY_MANAGED"
        },
        "models": {
            "gpt-5.3-codex":       {},
            "gpt-5.4":             {},
            "gpt-5.1-codex-max":   {},
            "gpt-5.1-codex-mini":  {},
            "claude-sonnet-4-5": {},
            "claude-haiku-4-5":  {}
        }
    });

    let providers = obj
        .entry("provider")
        .or_insert_with(|| Value::Object(Default::default()));
    if let Some(p) = providers.as_object_mut() {
        p.insert("vibe".into(), provider_entry);
    }

    // Set the default model so OpenCode doesn't prompt the user to pick one.
    // Use `insert` (not `entry`) to always update, so re-running takeover
    // after the user changed the model still points back to vibe.
    obj.insert("model".into(), Value::String("vibe/gpt-5.3-codex".into()));

    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;

    // Also sanitize config.json: remove any stale keys we wrote there.
    if let Some(dir) = path.parent() {
        let cfg_json = dir.join("config.json");
        if cfg_json.exists() {
            if let Ok(s) = std::fs::read_to_string(&cfg_json) {
                if let Ok(mut cfg) = serde_json::from_str::<Value>(&s) {
                    if let Some(o) = cfg.as_object_mut() {
                        let had_stale =
                            o.remove("baseUrl").is_some() | o.remove("baseURL").is_some();
                        if had_stale {
                            if let Ok(out) = serde_json::to_string_pretty(&cfg) {
                                let _ = std::fs::write(&cfg_json, out);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn disable_opencode_takeover(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(path)?;
    let mut v: Value = if raw.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
    };
    if let Some(obj) = v.as_object_mut() {
        if obj.get("model").and_then(|value| value.as_str()) == Some("vibe/gpt-5.3-codex") {
            obj.remove("model");
        }
        if let Some(providers) = obj
            .get_mut("provider")
            .and_then(|value| value.as_object_mut())
        {
            providers.remove("vibe");
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// Patch ~/.codex/config.toml for Codex CLI takeover.
///
/// Use custom [model_providers.vibeplus] + model_provider = "vibeplus",
/// instead of openai_base_url.
///
/// Important: requires_openai_auth = false lets Codex skip login and OPENAI_API_KEY checks.
/// Upstream auth is handled by Vibe credentials, not Codex auth.json.
///
/// See codex-rs/model-provider-info/src/lib.rs:
///   requires_openai_auth = false: no login UI and no auth.json requirement
///   wire_api = "responses": use /v1/responses (Responses API)
///   supports_websockets = true: Codex can use Responses WebSocket transport
fn patch_codex_config(path: &PathBuf, base_url: &str) -> Result<()> {
    let existing = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());

    let root = doc.as_table_mut();

    // Point to the custom provider and bypass built-in openai auth.
    root.insert("model_provider", toml_edit::value("vibeplus"));

    // Remove stale openai_base_url written by older takeover versions.
    root.remove("openai_base_url");

    // Create or update the [model_providers] section.
    let mp_item = root
        .entry("model_providers")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let mp = mp_item
        .as_table_mut()
        .context("model_providers must be a TOML table")?;

    let existing_provider = mp.get("vibeplus").and_then(|item| item.as_table());
    let existing_supports_websockets = existing_provider
        .and_then(|provider| provider.get("supports_websockets"))
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    let existing_ws_timeout = existing_provider
        .and_then(|provider| provider.get("websocket_connect_timeout_ms"))
        .and_then(|item| item.as_integer())
        .filter(|value| *value > 0)
        .unwrap_or(15_000);

    // Upsert [model_providers.vibeplus]. Keep transport preferences when this
    // config was already managed through the web UI.
    let mut vibeplus = existing_provider
        .cloned()
        .unwrap_or_else(toml_edit::Table::new);
    vibeplus.insert("name", toml_edit::value("vibe+"));
    vibeplus.insert("base_url", toml_edit::value(format!("{base_url}/codex/v1")));
    vibeplus.insert("wire_api", toml_edit::value("responses"));
    vibeplus.insert("requires_openai_auth", toml_edit::value(false));
    vibeplus.insert(
        "supports_websockets",
        toml_edit::value(existing_supports_websockets),
    );
    vibeplus.insert(
        "websocket_connect_timeout_ms",
        toml_edit::value(existing_ws_timeout),
    );
    mp.insert("vibeplus", toml_edit::Item::Table(vibeplus));

    std::fs::write(path, doc.to_string())?;
    Ok(())
}

fn disable_codex_takeover(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let existing = std::fs::read_to_string(path)?;
    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .unwrap_or_else(|_| toml_edit::DocumentMut::new());
    let root = doc.as_table_mut();
    if root.get("model_provider").and_then(|item| item.as_str()) == Some("vibeplus") {
        root.remove("model_provider");
    }
    if let Some(model_providers) = root
        .get_mut("model_providers")
        .and_then(|item| item.as_table_mut())
    {
        model_providers.remove("vibeplus");
        if model_providers.is_empty() {
            root.remove("model_providers");
        }
    }
    std::fs::write(path, doc.to_string())?;
    Ok(())
}
