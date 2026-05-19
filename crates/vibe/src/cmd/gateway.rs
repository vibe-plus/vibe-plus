//! Shared helpers for talking to the local gateway process.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::{Duration, Instant};
use vibe_core::paths;
use vibe_i18n::{detect_locale_from_env, ZH_CN_LOCALE};

pub fn local_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn current_version() -> &'static str {
    vibe_core::VERSION
}

pub async fn is_responsive(base_url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct GatewayStatusVersion {
    version: String,
}

pub async fn fetch_running_version(base_url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let response = client.get(format!("{base_url}/status")).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response
        .json::<GatewayStatusVersion>()
        .await
        .ok()
        .map(|status| status.version)
}

pub async fn wait_until_ready(base_url: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if is_responsive(base_url).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    anyhow::bail!("gateway did not become ready at {base_url} within {timeout:?}");
}

pub async fn wait_until_stopped(base_url: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !is_responsive(base_url).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    anyhow::bail!("gateway did not stop at {base_url} within {timeout:?}");
}

/// Stop whatever is serving the gateway port (pid file and/or listener).
pub async fn stop_at_port(port: u16) -> Result<()> {
    let base_url = local_base_url(port);
    let pid_path = paths::pid_path().context("resolve pid path")?;

    let pid = pid_path
        .exists()
        .then(|| {
            std::fs::read_to_string(&pid_path)
                .ok()
                .and_then(|raw| raw.trim().parse::<u32>().ok())
        })
        .flatten();

    if pid.is_none() && !is_responsive(&base_url).await {
        return Ok(());
    }

    if let Some(pid) = pid {
        stop_pid(pid);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    if is_responsive(&base_url).await {
        stop_listener_on_port(port);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }

    Ok(())
}

pub async fn ensure_running(port: u16) -> Result<String> {
    let base_url = local_base_url(port);
    if is_responsive(&base_url).await {
        if let Some(running) = fetch_running_version(&base_url).await {
            if running == current_version() {
                return Ok(base_url);
            }
            println!("{}", upgrade_restart_message(&running, current_version()));
            stop_at_port(port).await?;
            wait_until_stopped(&base_url, Duration::from_secs(15)).await?;
        } else {
            return Ok(base_url);
        }
    }

    let pid_path = paths::pid_path().context("resolve pid path")?;
    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }

    super::daemon::spawn_background(port)?;
    wait_until_ready(&base_url, Duration::from_secs(30)).await?;
    Ok(base_url)
}

pub fn upgrade_restart_message(running: &str, expected: &str) -> String {
    if detect_locale_from_env().to_string() == ZH_CN_LOCALE {
        format!("检测到旧版网关 {running}，正在重启为 {expected}…")
    } else {
        format!("Replacing gateway v{running} with v{expected}…")
    }
}

#[cfg(unix)]
fn stop_pid(pid: u32) {
    unsafe {
        if libc::kill(pid as i32, libc::SIGTERM) != 0 {
            let _ = libc::kill(pid as i32, libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
fn stop_pid(pid: u32) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F", "/T"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(any(unix, windows)))]
fn stop_pid(pid: u32) {
    let _ = pid;
}

#[cfg(unix)]
fn stop_listener_on_port(port: u16) {
    use std::process::Command;
    let Ok(output) = Command::new("lsof")
        .args(["-ti", &format!("tcp:{port}")])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let pids = String::from_utf8_lossy(&output.stdout);
    for line in pids.lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            stop_pid(pid);
        }
    }
}

#[cfg(windows)]
fn stop_listener_on_port(port: u16) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = format!(
        "$c = Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue; \
         if ($c) {{ $c | ForEach-Object {{ Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }} }}"
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(any(unix, windows)))]
fn stop_listener_on_port(port: u16) {
    let _ = port;
}
