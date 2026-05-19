//! CC Switch on-disk layout (`~/.cc-switch`).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Default CC Switch config root: `$HOME/.cc-switch`.
pub fn default_cc_switch_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CC_SWITCH_HOME") {
        return Ok(PathBuf::from(dir));
    }
    Ok(crate::paths::real_home_dir()?.join(".cc-switch"))
}

pub fn cc_switch_db_path(root: &Path) -> PathBuf {
    root.join("cc-switch.db")
}

pub fn cc_switch_settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub fn resolve_cc_switch_dir(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    let dir = override_dir.unwrap_or_else(|| default_cc_switch_dir().expect("home dir"));
    if dir.exists() {
        dir.canonicalize()
            .context("canonicalize cc-switch directory")
    } else {
        Ok(dir)
    }
}
