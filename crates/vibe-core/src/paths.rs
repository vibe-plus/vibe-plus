//! Filesystem layout under `~/.vibe/`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn vibe_home_override() -> Option<PathBuf> {
    env_path("VIBE_HOME")
}

pub fn real_home_dir() -> Result<PathBuf> {
    if let Some(home) = env_path("HOME") {
        return Ok(home);
    }
    if let Some(home) = env_path("USERPROFILE") {
        return Ok(home);
    }
    let dirs = directories::UserDirs::new().context("no home directory")?;
    Ok(dirs.home_dir().to_path_buf())
}

pub fn home_dir() -> Result<PathBuf> {
    if let Some(p) = vibe_home_override() {
        return Ok(p);
    }
    real_home_dir()
}

pub fn xdg_config_home() -> Result<PathBuf> {
    if let Some(path) = env_path("XDG_CONFIG_HOME") {
        return Ok(path);
    }
    Ok(real_home_dir()?.join(".config"))
}

pub fn vibe_dir() -> Result<PathBuf> {
    let p = home_dir()?.join(".vibe");
    std::fs::create_dir_all(&p).ok();
    Ok(p)
}

pub fn db_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("vibe.db"))
}

pub fn observability_db_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("observability.db"))
}

pub fn bodies_dir() -> Result<PathBuf> {
    let p = vibe_dir()?.join("bodies");
    std::fs::create_dir_all(&p).ok();
    Ok(p)
}

pub fn pid_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("vibe.pid"))
}

pub fn auto_update_lock_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("auto-update.lock"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("vibe.log"))
}

pub fn backups_dir() -> Result<PathBuf> {
    let p = vibe_dir()?.join("backups");
    std::fs::create_dir_all(&p).ok();
    Ok(p)
}

/// Stores auth.json files pasted by the user via the web UI.
/// Each file gets a UUID filename so multiple accounts can coexist.
pub fn codex_accounts_dir() -> Result<PathBuf> {
    let p = vibe_dir()?.join("codex-accounts");
    std::fs::create_dir_all(&p).ok();
    Ok(p)
}

pub fn claude_config_dir() -> Result<PathBuf> {
    Ok(env_path("CLAUDE_CONFIG_DIR").unwrap_or(real_home_dir()?.join(".claude")))
}

/// Claude Code user settings (`$CLAUDE_CONFIG_DIR/settings.json` or `~/.claude/settings.json`).
pub fn claude_settings_path() -> Result<PathBuf> {
    Ok(claude_config_dir()?.join("settings.json"))
}

pub fn codex_config_path() -> Result<PathBuf> {
    if let Some(home) = env_path("CODEX_HOME") {
        return Ok(home.join("config.toml"));
    }
    let home = real_home_dir()?;
    let primary = home.join(".codex").join("config.toml");
    if primary.exists() {
        return Ok(primary);
    }
    Ok(xdg_config_home()?.join("codex").join("config.toml"))
}

pub fn opencode_config_path() -> Result<PathBuf> {
    Ok(xdg_config_home()?.join("opencode").join("opencode.json"))
}

pub fn resolve_under(base: &Path, segments: &[&str]) -> PathBuf {
    segments
        .iter()
        .fold(base.to_path_buf(), |acc, segment| acc.join(segment))
}
