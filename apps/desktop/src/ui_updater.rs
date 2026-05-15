//! Background UI update checker.
//!
//! At startup the desktop shell spawns this as a detached async task.  It
//! fetches `{UI_BASE_URL}version.json` from GitHub Pages, compares the remote
//! version against the version embedded in the binary, and — if a compatible
//! newer build is available — downloads every file in the manifest to
//! `~/.vibe/ui-cache/dist/`.
//!
//! `version.json` is written *last*, so it doubles as an atomic "download
//! complete" marker.  `ui_assets::handle` picks up the new files on the very
//! next request; no restart is required.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

use crate::ui_assets::{ui_cache_dist_dir, ui_cache_version_file};

const UI_BASE_URL: &str = vibe_core::UI_BASE_URL;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
struct UiManifest {
    version: String,
    min_cli_protocol: u32,
    files: Vec<String>,
}

/// Fire-and-forget: check for a newer compatible UI build and download it.
/// All errors are logged at debug level so they never surface to the user.
pub async fn check_and_update(embedded_version: String) {
    match try_update(&embedded_version).await {
        Ok(true) => tracing::info!(
            embedded = %embedded_version,
            "UI cache updated; new version will be served on next navigation"
        ),
        Ok(false) => tracing::debug!(embedded = %embedded_version, "UI cache is up to date"),
        Err(e) => tracing::debug!("UI update check skipped: {e:#}"),
    }
}

async fn try_update(embedded_version: &str) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build reqwest client")?;

    // 1. Fetch remote manifest.
    let manifest_url = format!("{UI_BASE_URL}version.json");
    let manifest: UiManifest = client
        .get(&manifest_url)
        .send()
        .await
        .context("fetch remote version.json")?
        .error_for_status()
        .context("remote version.json HTTP error")?
        .json()
        .await
        .context("parse remote version.json")?;

    // 2. Skip if the remote UI requires a newer CLI protocol than we implement.
    if manifest.min_cli_protocol > vibe_core::WEB_COMPAT_API {
        tracing::debug!(
            remote_min = manifest.min_cli_protocol,
            cli_protocol = vibe_core::WEB_COMPAT_API,
            "remote UI requires newer CLI; skipping update"
        );
        return Ok(false);
    }

    // 3. Skip if not newer than what's embedded.
    if !is_newer(&manifest.version, embedded_version) {
        return Ok(false);
    }

    // 4. Skip if we already cached this exact version.
    if cached_version_matches(&manifest.version).await {
        return Ok(false);
    }

    tracing::info!(
        embedded = %embedded_version,
        remote = %manifest.version,
        files = manifest.files.len(),
        "downloading UI update"
    );

    // 5. Download every file in the manifest into the cache dir.
    let cache_dir = ui_cache_dist_dir().context("cannot determine UI cache dir")?;
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .context("create ui-cache/dist")?;

    for file in &manifest.files {
        if file == "version.json" {
            continue; // written last as the atomic completion marker
        }
        let url = format!("{UI_BASE_URL}{file}");
        let data = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("fetch {file}"))?
            .error_for_status()
            .with_context(|| format!("HTTP error for {file}"))?
            .bytes()
            .await
            .with_context(|| format!("read body of {file}"))?;

        let dest = cache_dir.join(file);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create parent dir for {file}"))?;
        }
        tokio::fs::write(&dest, &data)
            .await
            .with_context(|| format!("write {file} to cache"))?;
    }

    // 6. Write version.json last — signals that the cache is complete.
    let version_json = serde_json::to_vec(&serde_json::json!({
        "version": manifest.version,
        "min_cli_protocol": manifest.min_cli_protocol,
        "files": manifest.files,
    }))?;
    let version_file = ui_cache_version_file().context("cannot determine version file path")?;
    tokio::fs::write(&version_file, version_json)
        .await
        .context("write ui-cache/version.json")?;

    Ok(true)
}

/// Returns `true` when `~/.vibe/ui-cache/version.json` records the same
/// version as `remote_version`, meaning we've already downloaded this build.
async fn cached_version_matches(remote_version: &str) -> bool {
    let Some(path) = ui_cache_version_file() else {
        return false;
    };
    tokio::fs::read_to_string(&path)
        .await
        .ok()
        .and_then(|s| serde_json::from_str::<UiManifest>(&s).ok())
        .is_some_and(|m| m.version == remote_version)
}

/// Returns `true` when `remote` is strictly newer than `local` by semver.
fn is_newer(remote: &str, local: &str) -> bool {
    semver_cmp(remote, local) > 0
}

fn semver_cmp(a: &str, b: &str) -> i32 {
    let parse = |v: &str| {
        v.trim_start_matches('v')
            .splitn(4, '.')
            .take(3)
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let aa = parse(a);
    let bb = parse(b);
    for i in 0..3 {
        let av = aa.get(i).copied().unwrap_or(0);
        let bv = bb.get(i).copied().unwrap_or(0);
        if av != bv {
            return if av > bv { 1 } else { -1 };
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch() {
        assert!(is_newer("0.1.1", "0.1.0"));
    }

    #[test]
    fn same_version_not_newer() {
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn older_not_newer() {
        assert!(!is_newer("0.0.9", "1.0.0"));
    }

    #[test]
    fn v_prefix_handled() {
        assert!(is_newer("v1.2.0", "1.1.9"));
    }
}
