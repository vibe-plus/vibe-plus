use anyhow::Result;
use clap::Subcommand;

#[cfg(target_os = "macos")]
use anyhow::Context;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const LABEL: &str = "com.vibe-plus.gateway";

#[derive(Subcommand)]
pub enum AutostartCmd {
    /// Install a user-level startup entry for `vibe start --foreground`.
    Enable,
    /// Remove the startup entry.
    Disable,
    /// Show startup entry status.
    Status,
}

pub fn run(cmd: AutostartCmd) -> Result<()> {
    match cmd {
        AutostartCmd::Enable => enable(),
        AutostartCmd::Disable => disable(),
        AutostartCmd::Status => status(),
    }
}

#[cfg(target_os = "macos")]
fn enable() -> Result<()> {
    let path = launch_agent_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let log = vibe_core::paths::log_path()?.to_string_lossy().into_owned();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>start</string>
    <string>--foreground</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <false/>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    );
    std::fs::write(&path, plist)?;
    let _ = std::process::Command::new("launchctl")
        .arg("bootout")
        .arg("gui")
        .arg(format!("{}/{}", unsafe { libc::getuid() }, LABEL))
        .output();
    let output = std::process::Command::new("launchctl")
        .arg("bootstrap")
        .arg(format!("gui/{}", unsafe { libc::getuid() }))
        .arg(&path)
        .output()
        .context("running launchctl bootstrap")?;
    if !output.status.success() {
        anyhow::bail!(
            "launchctl bootstrap failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    println!("Autostart enabled.");
    println!("  {}", path.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn disable() -> Result<()> {
    let path = launch_agent_path()?;
    let _ = std::process::Command::new("launchctl")
        .arg("bootout")
        .arg("gui")
        .arg(format!("{}/{}", unsafe { libc::getuid() }, LABEL))
        .output();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    println!("Autostart disabled.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn status() -> Result<()> {
    let path = launch_agent_path()?;
    let installed = path.exists();
    let output = std::process::Command::new("launchctl")
        .arg("print")
        .arg(format!("gui/{}/{}", unsafe { libc::getuid() }, LABEL))
        .output();
    let loaded = output.as_ref().is_ok_and(|o| o.status.success());
    println!("installed: {}", if installed { "yes" } else { "no" });
    println!("loaded:    {}", if loaded { "yes" } else { "no" });
    if installed {
        println!("path:      {}", path.display());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<PathBuf> {
    let home = directories::UserDirs::new()
        .context("cannot find home directory")?
        .home_dir()
        .to_path_buf();
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

#[cfg(not(target_os = "macos"))]
fn enable() -> Result<()> {
    anyhow::bail!("autostart is currently implemented for macOS LaunchAgent only")
}

#[cfg(not(target_os = "macos"))]
fn disable() -> Result<()> {
    anyhow::bail!("autostart is currently implemented for macOS LaunchAgent only")
}

#[cfg(not(target_os = "macos"))]
fn status() -> Result<()> {
    println!("installed: unsupported");
    println!("loaded:    unsupported");
    Ok(())
}
