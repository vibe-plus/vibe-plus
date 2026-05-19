//! Assemble a [`CcSwitchSnapshot`] from CC Switch database + config on disk.

use super::db::CcSwitchDbReader;
use super::paths::{cc_switch_db_path, cc_switch_settings_path, resolve_cc_switch_dir};
use super::settings::load_app_settings;
use super::types::{CcSwitchAppType, CcSwitchSnapshot};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// Read CC Switch data from `root` (default `~/.cc-switch`).
pub fn extract_from_dir(root: PathBuf) -> Result<CcSwitchSnapshot> {
    let db_path = cc_switch_db_path(&root);
    if !db_path.is_file() {
        bail!(
            "cc-switch database not found at {} (is CC Switch installed?)",
            db_path.display()
        );
    }

    let settings_path = cc_switch_settings_path(&root);
    let settings = load_app_settings(&settings_path)?;

    let db = CcSwitchDbReader::open_read_only(&db_path)?;
    let schema_version = db.schema_version()?;
    let db_settings = db.load_db_settings()?;
    let proxy_configs = db.load_proxy_configs()?;
    let providers = db.load_all_providers()?;
    let effective_current = resolve_effective_current(&db, settings.as_ref())?;

    Ok(CcSwitchSnapshot {
        root,
        db_path,
        settings_path,
        schema_version,
        settings,
        providers,
        db_settings,
        proxy_configs,
        effective_current,
    })
}

/// Extract using `CC_SWITCH_HOME` or `~/.cc-switch`.
pub fn extract_default() -> Result<CcSwitchSnapshot> {
    let root = resolve_cc_switch_dir(None)?;
    extract_from_dir(root)
}

fn resolve_effective_current(
    db: &CcSwitchDbReader,
    settings: Option<&super::types::CcSwitchAppSettings>,
) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for app in CcSwitchAppType::ALL {
        let key = app.as_str().to_string();
        if let Some(id) = settings.and_then(|s| s.current_provider_id(app)) {
            if !id.is_empty() {
                out.insert(key, id.to_string());
                continue;
            }
        }
        if let Some(id) = db.current_provider_in_db(app.as_str())? {
            out.insert(key, id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccswitch::paths::default_cc_switch_dir;
    /// Runs when `~/.cc-switch/cc-switch.db` exists (developer machine).
    #[test]
    fn extract_real_installation_when_present() {
        let root = default_cc_switch_dir().expect("home");
        let db = root.join("cc-switch.db");
        if !db.is_file() {
            eprintln!(
                "skip extract_real_installation_when_present: no {}",
                db.display()
            );
            return;
        }

        let snap = extract_from_dir(root).expect("extract");
        assert!(snap.schema_version >= 1, "schema_version");
        assert!(!snap.providers.is_empty(), "providers");

        let codex_count = snap
            .providers
            .iter()
            .filter(|p| p.app_type == "codex")
            .count();
        assert!(codex_count > 0, "expected codex providers");

        assert!(
            snap.effective_current.contains_key("codex"),
            "effective_current codex"
        );

        eprintln!(
            "cc-switch extract ok: schema={} providers={} apps={:?} current_codex={:?}",
            snap.schema_version,
            snap.providers.len(),
            snap.providers_by_app(),
            snap.effective_current.get("codex")
        );
    }

    #[test]
    fn extract_missing_db_errors() {
        let err = extract_from_dir(PathBuf::from("/nonexistent-cc-switch-dir")).unwrap_err();
        assert!(err.to_string().contains("database not found"), "{err}");
    }
}

impl CcSwitchSnapshot {
    pub fn providers_by_app(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for p in &self.providers {
            *counts.entry(p.app_type.clone()).or_insert(0) += 1;
        }
        counts
    }

    pub fn provider_ids_for_app(&self, app_type: &str) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|p| p.app_type == app_type)
            .map(|p| p.id.as_str())
            .collect()
    }
}
