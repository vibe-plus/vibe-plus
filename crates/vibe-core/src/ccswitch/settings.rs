//! Load `~/.cc-switch/settings.json`.

use super::types::CcSwitchAppSettings;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn load_app_settings(path: &Path) -> Result<Option<CcSwitchAppSettings>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let settings: CcSwitchAppSettings =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(settings))
}
