use anyhow::{Context, Result};
use chrono::Utc;
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use crate::npm_registry;

const PACKAGE_NAME: &str = "@vibe-plus/cli";
const PACKAGE_LATEST: &str = "@vibe-plus/cli@latest";
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const REGISTRY_TIMEOUT: Duration = Duration::from_secs(10);
const UPDATER_ENV: &str = "VIBE_AUTO_UPDATER_CHILD";
const DISABLE_ENV: &str = "VIBE_DISABLE_AUTO_UPDATE";
const REGISTRY_URL_ENV: &str = "VIBE_AUTO_UPDATE_REGISTRY_URL";

#[derive(Debug, Deserialize)]
struct NpmPackageInfo {
    version: String,
}

pub fn disabled() -> bool {
    std::env::var_os(DISABLE_ENV).is_some()
}

pub fn spawn_background_checker(port: u16) {
    if disabled() || std::env::var_os(UPDATER_ENV).is_some() {
        return;
    }

    tokio::spawn(async move {
        log_update_step("background auto-update checker scheduled");
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            log_update_step("background auto-update check tick");
            if let Err(e) = check_once_and_spawn_updater(port).await {
                log_update_step(&format!("auto-update check skipped: {e:#}"));
                tracing::debug!(?e, "auto-update check skipped");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

pub async fn check_once_and_spawn_updater(port: u16) -> Result<bool> {
    if !try_acquire_update_lock() {
        log_update_step("auto-update lock already held; skipping");
        tracing::debug!("auto-update lock already held; skipping");
        return Ok(false);
    }

    log_update_step("checking npm registry for latest CLI version");
    let Some(latest) = fetch_latest_version().await? else {
        log_update_step("npm registry did not return a latest version; skipping");
        release_update_lock();
        return Ok(false);
    };
    if !is_newer(&latest, vibe_core::VERSION) {
        log_update_step(&format!(
            "no update needed: current {} is not older than remote {latest}",
            vibe_core::VERSION
        ));
        tracing::debug!(
            current = vibe_core::VERSION,
            latest,
            "vibe CLI is up to date"
        );
        release_update_lock();
        return Ok(false);
    }

    tracing::info!(
        current = vibe_core::VERSION,
        latest,
        "vibe CLI update available; spawning updater"
    );
    log_update_step(&format!(
        "update available: current {} -> remote {latest}",
        vibe_core::VERSION
    ));
    if let Err(err) = spawn_detached_updater(port, &latest) {
        log_update_step(&format!("failed to spawn updater helper: {err:#}"));
        release_update_lock();
        return Err(err);
    }
    log_update_step("updater helper spawned; exiting current gateway");
    Ok(true)
}

async fn fetch_latest_version() -> Result<Option<String>> {
    let registry = std::env::var_os(REGISTRY_URL_ENV)
        .map(std::path::PathBuf::from)
        .and_then(|p| p.into_os_string().into_string().ok())
        .unwrap_or_else(|| {
            let manager = npm_registry::package_manager();
            npm_registry::pick_registry(manager)
                .unwrap_or_else(|| npm_registry::DEFAULT_NPM_REGISTRY.to_owned())
        });
    let url = format!(
        "{}{PACKAGE_NAME}/latest",
        registry.trim_end_matches('/').to_owned() + "/"
    );
    let client = reqwest::Client::builder()
        .timeout(REGISTRY_TIMEOUT)
        .build()
        .context("build npm registry client")?;
    let response = client
        .get(url)
        .send()
        .await
        .context("fetch latest package metadata")?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    let info = response
        .error_for_status()
        .context("npm registry returned error")?
        .json::<NpmPackageInfo>()
        .await
        .context("parse npm package metadata")?;
    Ok(Some(info.version))
}

pub fn run_updater_child(port: u16, expected_version: Option<String>) -> Result<()> {
    release_update_lock();
    log_update_step("auto-updater child started");
    if let Some(expected) = expected_version.as_deref() {
        log_update_step(&format!("expected target version: {expected}"));
    }
    log_update_step("preflight: rechecking remote latest before touching the running gateway");
    let preflight: Result<Option<String>> = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(fetch_latest_version())
    });
    match preflight {
        Ok(Some(latest)) if is_newer(&latest, vibe_core::VERSION) => {
            log_update_step(&format!("preflight confirmed remote latest {latest}"));
        }
        Ok(Some(latest)) => {
            log_update_step(&format!(
                "preflight says no update needed anymore (remote {latest}, current {})",
                vibe_core::VERSION
            ));
            relaunch_previous_gateway(port)?;
            return Ok(());
        }
        Ok(None) => {
            log_update_step("preflight could not read a latest version; restoring old gateway");
            relaunch_previous_gateway(port)?;
            return Ok(());
        }
        Err(err) => {
            log_update_step(&format!(
                "preflight network/query error: {err:#}; restoring old gateway"
            ));
            relaunch_previous_gateway(port)?;
            return Ok(());
        }
    }
    log_update_step("waiting for gateway port to close");
    wait_for_port_closed(port, Duration::from_secs(15));
    log_update_step("stopping the running gateway before install");
    shutdown_running_gateway(port);
    log_update_step(&format!("installing {PACKAGE_LATEST}"));
    let install_result = (|| -> Result<()> {
        let manager = npm_registry::package_manager();
        npm_registry::install_global(manager, PACKAGE_LATEST)
    })();

    match install_result {
        Ok(()) => {
            if let Some(expected) = expected_version.as_deref() {
                log_update_step(&format!("install succeeded for target {expected}"));
            } else {
                log_update_step("install succeeded");
            }
            log_update_step("relaunching updated gateway");
            spawn_updated_gateway(port)?;
        }
        Err(err) => {
            log_update_step(&format!(
                "install failed: {err:#}; relaunching previous gateway"
            ));
            eprintln!("vibe auto-updater: update failed: {err:#}");
            relaunch_previous_gateway(port)?;
        }
    }
    Ok(())
}

fn spawn_detached_updater(port: u16, latest: &str) -> Result<()> {
    let relaunch_exe = std::env::current_exe().context("resolve current vibe executable")?;
    let updater_exe = copy_current_exe_for_update()?;
    let mut cmd = std::process::Command::new(updater_exe);
    cmd.arg("auto-update-child")
        .arg("--port")
        .arg(port.to_string())
        .arg("--expected-version")
        .arg(latest)
        .env("VIBE_AUTO_UPDATE_RELAUNCH_EXE", relaunch_exe)
        .env(UPDATER_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    spawn_detached(&mut cmd)?;
    std::process::exit(0);
}

fn copy_current_exe_for_update() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("resolve current vibe executable")?;
    let mut dest = std::env::temp_dir();
    let ext = exe.extension().and_then(|s| s.to_str()).unwrap_or("");
    let file = if ext.is_empty() {
        format!("vibe-auto-updater-{}", std::process::id())
    } else {
        format!("vibe-auto-updater-{}.{}", std::process::id(), ext)
    };
    dest.push(file);
    std::fs::copy(&exe, &dest)
        .with_context(|| format!("copy updater helper to {}", dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dest)
            .with_context(|| format!("read helper metadata {}", dest.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms)
            .with_context(|| format!("mark helper executable {}", dest.display()))?;
    }
    Ok(dest)
}

fn spawn_updated_gateway(port: u16) -> Result<()> {
    let mut cmd = std::process::Command::new(resolve_vibe_command());
    cmd.arg("up")
        .arg("--foreground")
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    spawn_detached(&mut cmd)
}

fn relaunch_previous_gateway(port: u16) -> Result<()> {
    let relaunch = resolve_relaunch_exe();
    let mut cmd = std::process::Command::new(relaunch);
    cmd.arg("up")
        .arg("--foreground")
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    spawn_detached(&mut cmd)
}

fn resolve_vibe_command() -> std::path::PathBuf {
    std::env::var_os("VIBE_AUTO_UPDATE_RELAUNCH_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("vibe"))
}

fn resolve_relaunch_exe() -> std::path::PathBuf {
    std::env::var_os("VIBE_AUTO_UPDATE_RELAUNCH_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("vibe"))
}

fn spawn_detached(cmd: &mut std::process::Command) -> Result<()> {
    #[cfg(unix)]
    {
        cmd.spawn().context("spawn detached process")?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()
            .context("spawn detached process")?;
    }

    Ok(())
}

fn wait_for_port_closed(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !port_is_listening(port) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn port_is_listening(port: u16) -> bool {
    let Ok(addr) = format!("127.0.0.1:{port}").parse() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

fn shutdown_running_gateway(port: u16) {
    let _ = std::process::Command::new(resolve_vibe_command())
        .arg("stop")
        .arg("--port")
        .arg(port.to_string())
        .status();
}

fn try_acquire_update_lock() -> bool {
    let Ok(path) = vibe_core::paths::auto_update_lock_path() else {
        return false;
    };
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .is_ok()
}

fn release_update_lock() {
    if let Ok(path) = vibe_core::paths::auto_update_lock_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn log_update_step(message: &str) {
    tracing::info!(target: "vibe::auto_update", "{message}");
    let Ok(path) = vibe_core::paths::log_path() else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} [auto-update] {}",
            Utc::now().to_rfc3339(),
            message
        );
    }
}

fn is_newer(remote: &str, local: &str) -> bool {
    semver_cmp(remote, local) > 0
}

fn semver_cmp(a: &str, b: &str) -> i32 {
    let parse = |v: &str| {
        v.trim_start_matches('v')
            .splitn(4, '.')
            .take(3)
            .map(|p| {
                p.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .unwrap_or(0)
            })
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
    fn detects_newer_versions() {
        assert!(is_newer("0.0.7", "0.0.6"));
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(!is_newer("0.0.6", "0.0.6"));
        assert!(!is_newer("0.0.5", "0.0.6"));
    }
}
