//! Register Vibe Plus to start automatically on user login (macOS + Windows).
//!
//! On macOS this writes a per-user LaunchAgent with `RunAtLoad` and `KeepAlive`,
//! which doubles as a watchdog: if the user kills the gateway by hand, launchd
//! relaunches it. On Windows it writes the `HKCU\…\CurrentVersion\Run` registry
//! value, which only fires at login (no watchdog — Windows users get auto-start
//! but not auto-restart).
//!
//! Idempotency state lives at `~/.vibe/state/autostart.json`, so subsequent
//! `vibe` invocations skip silently once registration succeeded.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DISABLE_ENV: &str = "VIBE_DISABLE_AUTOSTART";
const LAUNCH_AGENT_LABEL: &str = "com.vibe-plus.gateway";
#[cfg(target_os = "windows")]
const WINDOWS_RUN_VALUE_NAME: &str = "VibePlus";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutostartState {
    pub enabled: bool,
    pub binary_path: PathBuf,
    pub registered_at: String,
    pub platform: String,
}

#[derive(Debug)]
#[allow(dead_code)] // `Skipped`'s reason string is for tracing/debug-print only.
pub enum EnsureOutcome {
    /// Just registered for the first time (or re-registered after a binary path change).
    Registered { binary_path: PathBuf },
    /// Previously registered and still pointing at this binary — nothing to do.
    AlreadyRegistered,
    /// User explicitly disabled it via `vibe autostart disable`; we won't re-enable silently.
    UserDisabled,
    /// Skipped because we're not in a supportable context (dev build, env override, unsupported OS).
    Skipped(&'static str),
}

/// Called from `vibe up` first-run path. Silent on success: prints one line on
/// state change, nothing if already registered.
pub fn ensure_enabled_silent() -> EnsureOutcome {
    match ensure_enabled() {
        Ok(outcome) => {
            match &outcome {
                EnsureOutcome::Registered { binary_path } => {
                    println!(
                        "  [autostart] 已注册开机自启（{}）。可用 `vibe autostart disable` 关闭。",
                        binary_path.display()
                    );
                }
                EnsureOutcome::AlreadyRegistered
                | EnsureOutcome::UserDisabled
                | EnsureOutcome::Skipped(_) => {}
            }
            outcome
        }
        Err(err) => {
            eprintln!("  [autostart] 注册失败（已忽略）：{err:#}");
            EnsureOutcome::Skipped("error")
        }
    }
}

pub fn ensure_enabled() -> Result<EnsureOutcome> {
    if std::env::var_os(DISABLE_ENV).is_some() {
        return Ok(EnsureOutcome::Skipped("VIBE_DISABLE_AUTOSTART set"));
    }
    if std::env::var_os("VIBE_HOME").is_some() {
        // Custom VIBE_HOME is typically a dev/test sandbox; don't pollute the
        // real login items.
        return Ok(EnsureOutcome::Skipped("VIBE_HOME override (dev/test)"));
    }
    if !platform_supported() {
        return Ok(EnsureOutcome::Skipped("platform not supported"));
    }

    let exe = std::env::current_exe().context("resolve current vibe executable")?;
    if looks_like_dev_build(&exe) {
        return Ok(EnsureOutcome::Skipped("dev build (target/ or tmp path)"));
    }

    if let Some(state) = read_state()? {
        if !state.enabled {
            return Ok(EnsureOutcome::UserDisabled);
        }
        if state.binary_path == exe && registration_is_live(&exe)? {
            return Ok(EnsureOutcome::AlreadyRegistered);
        }
        // Path changed (new install location) or registration was removed
        // externally — re-register.
    }

    register_platform(&exe)?;
    write_state(&AutostartState {
        enabled: true,
        binary_path: exe.clone(),
        registered_at: chrono::Utc::now().to_rfc3339(),
        platform: current_platform().to_string(),
    })?;
    Ok(EnsureOutcome::Registered { binary_path: exe })
}

pub fn enable() -> Result<()> {
    let exe = std::env::current_exe().context("resolve current vibe executable")?;
    register_platform(&exe)?;
    write_state(&AutostartState {
        enabled: true,
        binary_path: exe.clone(),
        registered_at: chrono::Utc::now().to_rfc3339(),
        platform: current_platform().to_string(),
    })?;
    println!("已注册开机自启：{}", exe.display());
    Ok(())
}

pub fn disable() -> Result<()> {
    unregister_platform()?;
    let state_path = vibe_core::paths::autostart_state_path()?;
    if let Ok(Some(mut state)) = read_state() {
        state.enabled = false;
        let json = serde_json::to_string_pretty(&state)?;
        std::fs::write(&state_path, json)
            .with_context(|| format!("write {}", state_path.display()))?;
    }
    println!("已关闭开机自启。可用 `vibe autostart enable` 恢复。");
    Ok(())
}

pub fn status() -> Result<()> {
    match read_state()? {
        Some(state) if state.enabled => {
            println!("autostart: enabled");
            println!("  binary:   {}", state.binary_path.display());
            println!("  platform: {}", state.platform);
            println!("  since:    {}", state.registered_at);
            println!("  live:     {}", registration_is_live(&state.binary_path)?);
        }
        Some(_) => println!("autostart: disabled (user opted out)"),
        None => println!("autostart: not configured"),
    }
    Ok(())
}

fn read_state() -> Result<Option<AutostartState>> {
    let path = vibe_core::paths::autostart_state_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let state: AutostartState = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(state))
}

fn write_state(state: &AutostartState) -> Result<()> {
    let path = vibe_core::paths::autostart_state_path()?;
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn looks_like_dev_build(exe: &Path) -> bool {
    let s = exe.to_string_lossy();
    s.contains("/target/") || s.contains("\\target\\") || s.starts_with("/tmp/")
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unsupported"
    }
}

fn platform_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

// ─── macOS ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<PathBuf> {
    let home = vibe_core::paths::real_home_dir()?;
    let dir = home.join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create {}", dir.display()))?;
    Ok(dir.join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn register_platform(exe: &Path) -> Result<()> {
    use std::io::Write;
    let plist_path = launch_agent_path()?;
    let log_dir = vibe_core::paths::vibe_dir()?;
    let stdout_log = log_dir.join("launchd.out.log");
    let stderr_log = log_dir.join("launchd.err.log");
    let path_env = std::env::var("PATH").unwrap_or_else(|_|
        "/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin".to_string());

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>up</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path_env}</string>
    </dict>
</dict>
</plist>
"#,
        label = LAUNCH_AGENT_LABEL,
        exe = xml_escape(&exe.to_string_lossy()),
        stdout = xml_escape(&stdout_log.to_string_lossy()),
        stderr = xml_escape(&stderr_log.to_string_lossy()),
        path_env = xml_escape(&path_env),
    );

    {
        let mut f = std::fs::File::create(&plist_path)
            .with_context(|| format!("create {}", plist_path.display()))?;
        f.write_all(plist.as_bytes())
            .with_context(|| format!("write {}", plist_path.display()))?;
    }

    // launchctl unload before load — idempotent for re-registration.
    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg(&plist_path)
        .status();
    let status = std::process::Command::new("launchctl")
        .arg("load")
        .arg("-w")
        .arg(&plist_path)
        .status()
        .context("invoke launchctl load")?;
    if !status.success() {
        anyhow::bail!("launchctl load -w {} failed", plist_path.display());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn unregister_platform() -> Result<()> {
    let plist_path = launch_agent_path()?;
    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .arg("unload")
            .arg(&plist_path)
            .status();
        std::fs::remove_file(&plist_path)
            .with_context(|| format!("remove {}", plist_path.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn registration_is_live(_exe: &Path) -> Result<bool> {
    Ok(launch_agent_path()?.exists())
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ─── Windows ────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn register_platform(exe: &Path) -> Result<()> {
    // Quoted exe path so spaces work. `vibe up` argument follows.
    let value = format!("\"{}\" up", exe.display());
    let status = std::process::Command::new("reg")
        .args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            WINDOWS_RUN_VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &value,
            "/f",
        ])
        .status()
        .context("invoke reg add")?;
    if !status.success() {
        anyhow::bail!("reg add failed");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn unregister_platform() -> Result<()> {
    let _ = std::process::Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            WINDOWS_RUN_VALUE_NAME,
            "/f",
        ])
        .status();
    Ok(())
}

#[cfg(target_os = "windows")]
fn registration_is_live(_exe: &Path) -> Result<bool> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            WINDOWS_RUN_VALUE_NAME,
        ])
        .output()
        .context("invoke reg query")?;
    Ok(output.status.success())
}

// ─── Unsupported platforms (Linux) ──────────────────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn register_platform(_exe: &Path) -> Result<()> {
    anyhow::bail!("autostart not supported on this platform")
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn unregister_platform() -> Result<()> {
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn registration_is_live(_exe: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn dev_build_paths_are_detected() {
        assert!(looks_like_dev_build(&PathBuf::from(
            "/Users/x/Documents/proj/target/debug/vibe"
        )));
        assert!(looks_like_dev_build(&PathBuf::from(
            "/tmp/vibe-auto-updater-1234"
        )));
        assert!(!looks_like_dev_build(&PathBuf::from(
            "/usr/local/bin/vibe"
        )));
        assert!(!looks_like_dev_build(&PathBuf::from(
            "/Users/x/.nvm/versions/node/v20/bin/vibe"
        )));
    }
}
