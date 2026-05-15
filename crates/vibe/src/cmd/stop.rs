use anyhow::Result;
use std::process::Command;
use std::time::Duration;
use vibe_core::paths;

use super::{configured_port, gateway};

pub async fn run() -> Result<()> {
    let port = configured_port();
    let base_url = gateway::local_base_url(port);
    let pid_path = paths::pid_path()?;

    let pid = pid_path
        .exists()
        .then(|| {
            std::fs::read_to_string(&pid_path)
                .ok()
                .and_then(|raw| raw.trim().parse::<u32>().ok())
        })
        .flatten();

    if pid.is_none() && !gateway::is_responsive(&base_url).await {
        println!("vibe is not running.");
        return Ok(());
    }

    if let Some(pid) = pid {
        stop_pid(pid);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    if gateway::is_responsive(&base_url).await {
        stop_listener_on_port(port);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }

    if gateway::is_responsive(&base_url).await {
        println!("vibe may still be running at {base_url}");
        if std::env::consts::OS == "windows" {
            println!("  try: taskkill /PID <pid> /F  or  vibe stop  after updating the CLI");
        }
    } else if let Some(pid) = pid {
        println!("vibe stopped (pid {pid}).");
    } else {
        println!("vibe stopped.");
    }

    Ok(())
}

fn stop_pid(pid: u32) {
    #[cfg(windows)]
    stop_pid_windows(pid);
    #[cfg(unix)]
    stop_pid_unix(pid);
    #[cfg(not(any(windows, unix)))]
    eprintln!("vibe stop: unsupported OS for pid {pid}");
}

#[cfg(unix)]
fn stop_pid_unix(pid: u32) {
    unsafe {
        if libc::kill(pid as i32, libc::SIGTERM) != 0 {
            let _ = libc::kill(pid as i32, libc::SIGKILL);
        }
    }
}

fn stop_pid_windows(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        eprintln!("vibe stop: taskkill is only available in Windows builds");
    }
}

fn stop_listener_on_port(port: u16) {
    #[cfg(windows)]
    stop_listener_on_port_windows(port);
    #[cfg(unix)]
    stop_listener_on_port_unix(port);
}

#[cfg(unix)]
fn stop_listener_on_port_unix(port: u16) {
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
            stop_pid_unix(pid);
        }
    }
}

fn stop_listener_on_port_windows(port: u16) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
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
    #[cfg(not(windows))]
    {
        let _ = port;
    }
}
