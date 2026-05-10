use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;
use std::path::PathBuf;
use vibe_core::paths;

#[derive(Args)]
pub struct TakeoverArgs {
    /// Target client: claude, opencode, codex.
    pub client: String,
    /// Undo the takeover and restore from backup.
    #[arg(long)]
    pub restore: bool,
}

pub async fn run(args: TakeoverArgs) -> Result<()> {
    if args.restore {
        return restore(&args.client);
    }
    takeover(&args.client).await
}

async fn takeover(client: &str) -> Result<()> {
    let base_url = format!("http://127.0.0.1:{}", super::DEFAULT_PORT);

    println!("=== vibe takeover: {client} ===\n");

    // Verify proxy is running first.
    match reqwest::get(format!("{base_url}/health")).await {
        Ok(r) if r.status().is_success() => {
            println!("[ok]  proxy is running at {base_url}");
        }
        _ => {
            println!("[!!]  warning: proxy not reachable at {base_url}");
            println!("      run `vibe start` first for the config to take effect.\n");
        }
    }

    let cfg_path = detect_config_path(client)?;
    println!("Config : {}", cfg_path.display());

    if cfg_path.exists() {
        let backup = backup_path(client)?;
        std::fs::copy(&cfg_path, &backup)?;
        println!("Backup : {}", backup.display());
    }

    patch_config(client, &cfg_path, &base_url)?;
    println!("Status : patched\n");

    print_next_steps(client, &base_url);
    Ok(())
}

fn restore(client: &str) -> Result<()> {
    let cfg_path = detect_config_path(client)?;
    let backup = latest_backup(client)?;
    std::fs::copy(&backup, &cfg_path).with_context(|| {
        format!("restoring {} from {}", cfg_path.display(), backup.display())
    })?;
    println!("[ok]  restored {client} config from {}", backup.display());
    // auth.json 从未被修改，无需恢复
    Ok(())
}

// ---------------------------------------------------------------------------
// Config path detection
// ---------------------------------------------------------------------------

fn detect_config_path(client: &str) -> Result<PathBuf> {
    let dirs = directories::UserDirs::new().context("cannot find home directory")?;
    let home = dirs.home_dir();

    let path = match client {
        "claude" => {
            // Claude Code reads ~/.claude/settings.json and injects its `env` block
            // as environment variables before starting. This is how all proxy
            // implementations (cc-switch, cligate, OmniProxy) redirect the base URL.
            home.join(".claude").join("settings.json")
        }
        "opencode" => {
            // CC Switch confirms: OpenCode's canonical user-override file is
            // ~/.config/opencode/opencode.json (opencode_config.rs get_opencode_config_path).
            // config.json is OpenCode's own managed/generated file — we don't write there.
            home.join(".config").join("opencode").join("opencode.json")
        }
        "codex" => {
            // Codex CLI: ~/.codex/config.toml is the primary config; auth.json holds keys.
            // We write to config.toml for the base_url field.
            let p1 = home.join(".codex").join("config.toml");
            if p1.exists() {
                return Ok(p1);
            }
            // Older/alternate path
            let p2 = home.join(".config").join("codex").join("config.toml");
            if p2.exists() {
                return Ok(p2);
            }
            p1
        }
        other => anyhow::bail!("unknown client: {other}. Supported: claude, opencode, codex"),
    };
    Ok(path)
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
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
];

/// Patch ~/.claude/settings.json:
/// - Set env.ANTHROPIC_BASE_URL → our proxy
/// - Set env.ANTHROPIC_AUTH_TOKEN → "PROXY_MANAGED" (proxy handles auth)
/// - Remove any hardcoded model overrides from prior proxies (Mimo, etc.)
///   so Claude Code uses its built-in defaults, which route through us.
fn patch_claude_settings(path: &PathBuf, base_url: &str) -> Result<()> {
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

    let obj = v.as_object_mut().context("settings.json root must be an object")?;
    let env = obj
        .entry("env")
        .or_insert_with(|| Value::Object(Default::default()));
    let env_obj = env.as_object_mut().context("settings.json env must be an object")?;

    // 用 /claude 工具前缀：Claude SDK 会调用 /claude/v1/messages、/claude/v1/models
    // vibe 可据此过滤出 Anthropic 供应商，并在 /claude/v1/models 返回 Anthropic 格式列表
    let claude_base = format!("{base_url}/claude");
    env_obj.insert("ANTHROPIC_BASE_URL".into(), Value::String(claude_base));

    // Replace the real token with a placeholder — the proxy handles authentication
    // via its own credential pool, so the incoming token is irrelevant.
    env_obj.insert("ANTHROPIC_AUTH_TOKEN".into(), Value::String("PROXY_MANAGED".into()));

    // Strip upstream-specific model pins so Claude Code falls back to its own
    // built-in defaults (e.g. claude-sonnet-4-5), which our proxy can route.
    for key in CLAUDE_MODEL_OVERRIDES {
        env_obj.remove(*key);
    }

    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
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
    // ── Write to opencode.json (user override layer) ─────────────────────────
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

    let obj = v.as_object_mut().context("opencode.json root must be an object")?;

    // Remove any stale top-level keys from old vibe takeover versions.
    obj.remove("baseURL");
    obj.remove("baseUrl");

    // Ensure $schema is present (matches OpenCode's own write logic).
    obj.entry("$schema")
        .or_insert_with(|| Value::String("https://opencode.ai/config.json".into()));

    // Upsert provider.vibe entry.
    //
    // OpenCode does NOT auto-fetch /v1/models — it reads exclusively from
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

    // ── Also sanitize config.json: remove any stale keys we wrote there ──────
    if let Some(dir) = path.parent() {
        let cfg_json = dir.join("config.json");
        if cfg_json.exists() {
            if let Ok(s) = std::fs::read_to_string(&cfg_json) {
                if let Ok(mut cfg) = serde_json::from_str::<Value>(&s) {
                    if let Some(o) = cfg.as_object_mut() {
                        let had_stale = o.remove("baseUrl").is_some() | o.remove("baseURL").is_some();
                        if had_stale {
                            if let Ok(out) = serde_json::to_string_pretty(&cfg) {
                                let _ = std::fs::write(&cfg_json, out);
                                println!("Cleaned : removed stale key from {}", cfg_json.display());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Patch ~/.codex/config.toml for Codex CLI takeover.
///
/// 使用自定义 [model_providers.vibeplus] + model_provider = "vibeplus"，
/// 而不是 openai_base_url。
///
/// 关键：requires_openai_auth = false 让 Codex 完全跳过登录界面和 OPENAI_API_KEY 校验。
/// 上游认证由 vibe 自己的 credential pool 负责，与 Codex 的 auth.json 无关。
///
/// 见 codex-rs/model-provider-info/src/lib.rs：
///   requires_openai_auth = false → 不展示登录界面，不读取 auth.json
///   wire_api = "responses"       → 使用 /v1/responses（Responses API）
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

    // 指向自定义 provider，跳过 built-in openai（requires_openai_auth=true）
    root.insert("model_provider", toml_edit::value("vibeplus"));

    // 清除旧版 takeover 写入的 openai_base_url（已无意义）
    root.remove("openai_base_url");

    // 创建或覆盖 [model_providers] 节
    let mp_item = root
        .entry("model_providers")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let mp = mp_item
        .as_table_mut()
        .context("model_providers must be a TOML table")?;

    // Upsert [model_providers.vibeplus]
    let mut vibeplus = toml_edit::Table::new();
    vibeplus.insert("name", toml_edit::value("vibe+"));
    vibeplus.insert("base_url", toml_edit::value(format!("{base_url}/codex/v1")));
    vibeplus.insert("wire_api", toml_edit::value("responses"));
    vibeplus.insert("requires_openai_auth", toml_edit::value(false));
    mp.insert("vibeplus", toml_edit::Item::Table(vibeplus));

    std::fs::write(path, doc.to_string())?;
    println!("Auth    : auth.json untouched — requires_openai_auth=false, no login needed");

    Ok(())
}

fn print_next_steps(client: &str, base_url: &str) {
    match client {
        "claude" => {
            println!("Next steps:");
            println!("  Restart Claude Code — it will pick up ANTHROPIC_BASE_URL from settings.json.");
            println!("  All requests will route through vibe+ at {base_url}.");
            println!();
            println!("  Verify: claude -p \"say hi\"");
            println!("          then check: curl -s {base_url}/_vp/logs?limit=1 | jq");
        }
        "opencode" => {
            println!("Next steps:");
            println!("  Restart OpenCode — 请求将经 vibe+ 路由，路径 {base_url}/opencode/v1");
        }
        "codex" => {
            println!("Next steps:");
            println!("  Restart codex — 无需登录，请求将经 vibe+ 路由至 {base_url}/codex/v1");
            println!();
            println!("  Verify: codex \"say hi\"");
            println!("          then check: curl -s {base_url}/_vp/logs?limit=1 | jq");
        }
        _ => {}
    }
    println!();
    println!("To undo: vibe takeover {client} --restore");
}

// ---------------------------------------------------------------------------
// Backup helpers
// ---------------------------------------------------------------------------

fn backup_path(client: &str) -> Result<PathBuf> {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let ext = if client == "codex" { "bak.toml" } else { "bak.json" };
    let filename = format!("{client}-{ts}.{ext}");
    Ok(paths::backups_dir()?.join(filename))
}

fn latest_backup(client: &str) -> Result<PathBuf> {
    let dir = paths::backups_dir()?;
    let prefix = format!("{client}-");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix))
                .unwrap_or(false)
        })
        .collect();
    entries.sort();
    entries.pop().context(format!("no backup found for {client}"))
}
