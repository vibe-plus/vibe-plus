//! Filesystem layout under `~/.vibe/`.

use anyhow::{Context, Result};
use std::path::PathBuf;

fn vibe_home_override() -> Option<PathBuf> {
    std::env::var_os("VIBE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

pub fn home_dir() -> Result<PathBuf> {
    if let Some(p) = vibe_home_override() {
        return Ok(p);
    }
    let dirs = directories::UserDirs::new().context("no home directory")?;
    Ok(dirs.home_dir().to_path_buf())
}

pub fn vibe_dir() -> Result<PathBuf> {
    let p = home_dir()?.join(".vibe");
    std::fs::create_dir_all(&p).ok();
    Ok(p)
}

pub fn db_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("vibe.db"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("config.toml"))
}

pub fn pid_path() -> Result<PathBuf> {
    Ok(vibe_dir()?.join("vibe.pid"))
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
