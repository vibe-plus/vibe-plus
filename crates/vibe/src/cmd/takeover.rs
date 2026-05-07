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
    let cfg_path = detect_config_path(client)?;
    println!("Config file: {}", cfg_path.display());

    // Backup
    let backup = backup_path(client)?;
    if cfg_path.exists() {
        std::fs::copy(&cfg_path, &backup)?;
        println!("Backed up to: {}", backup.display());
    }

    // Patch
    let endpoint = format!("http://127.0.0.1:{}", super::DEFAULT_PORT);
    patch_config(client, &cfg_path, &endpoint)?;
    println!("Patched config for {client}.");

    // Verify
    let health_url = format!("{endpoint}/health");
    match reqwest::get(&health_url).await {
        Ok(r) if r.status().is_success() => {
            println!("[ok]  proxy reachable at {endpoint}");
        }
        _ => {
            println!("[!!]  warning: proxy not responding at {endpoint}");
            println!("      run `vibe start` first, then rerun takeover.");
        }
    }

    println!("\nDone! {client} will now route through vibe.");
    println!("To undo: vibe takeover {} --restore", client);
    Ok(())
}

fn restore(client: &str) -> Result<()> {
    let cfg_path = detect_config_path(client)?;
    let backup = latest_backup(client)?;
    std::fs::copy(&backup, &cfg_path)?;
    println!("Restored {client} config from {}.", backup.display());
    Ok(())
}

fn detect_config_path(client: &str) -> Result<PathBuf> {
    let dirs = directories::UserDirs::new().context("no home dir")?;
    let home = dirs.home_dir();
    let path = match client {
        "claude" => {
            // Claude Code stores its settings in ~/.claude.json or ~/Library/Application Support/claude-code
            let p1 = home.join(".claude.json");
            if p1.exists() { return Ok(p1); }
            home.join(".config").join("claude").join("settings.json")
        }
        "opencode" => home.join(".config").join("opencode").join("config.json"),
        "codex" => home.join(".config").join("codex").join("config.json"),
        other => anyhow::bail!("unknown client: {other}. Supported: claude, opencode, codex"),
    };
    Ok(path)
}

fn backup_path(client: &str) -> Result<PathBuf> {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let filename = format!("{client}-{ts}.bak.json");
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

fn patch_config(client: &str, path: &PathBuf, endpoint: &str) -> Result<()> {
    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut v: Value = if path.exists() {
        let s = std::fs::read_to_string(path)?;
        if s.trim().is_empty() { Value::Object(Default::default()) }
        else { serde_json::from_str(&s)? }
    } else {
        Value::Object(Default::default())
    };

    match client {
        "claude" => {
            // Claude Code uses `ANTHROPIC_BASE_URL` env or a config key.
            // The settings.json approach: set `apiBaseUrl`.
            if let Some(obj) = v.as_object_mut() {
                obj.insert("apiBaseUrl".into(), Value::String(endpoint.into()));
            }
        }
        "opencode" | "codex" => {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("baseUrl".into(), Value::String(format!("{endpoint}/v1")));
            }
        }
        _ => {}
    }

    std::fs::write(path, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}
