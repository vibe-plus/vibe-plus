use anyhow::Result;
use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;
use vibe_core::{config::Config, paths, state::AppState};
use vibe_db::Db;
use vibe_i18n::text_env;

use super::gateway;

fn default_port() -> u16 {
    super::configured_port()
}

#[derive(clap::Args)]
pub struct StartArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = default_port())]
    pub port: u16,

    /// Run in the foreground instead of daemonising.
    #[arg(long)]
    pub foreground: bool,
}

pub async fn run(args: StartArgs) -> Result<()> {
    let base_url = gateway::local_base_url(args.port);
    if gateway::is_responsive(&base_url).await {
        println!("{}", text_env("cli-start-already-running"));
        println!("  endpoint: {base_url}");
        return Ok(());
    }

    let pid_path = paths::pid_path()?;
    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }

    if !args.foreground {
        spawn_background(args.port)?;
        gateway::wait_until_ready(&base_url, Duration::from_secs(30)).await?;
        return Ok(());
    }

    start_server(args.port).await
}

/// Spawn `vibe start --foreground` detached from this terminal.
pub fn spawn_background(port: u16) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("start")
        .arg("--foreground")
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    cmd.spawn()?;

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()?;
    }

    println!("{}", text_env("cli-start-started-background"));
    println!("  endpoint: {}", gateway::local_base_url(port));
    Ok(())
}

pub async fn start_server(port: u16) -> Result<()> {
    // Config is in-memory only; the runtime defaults live in
    // `vibe_core::config::Config::default()`. The CLI `--port` flag below is
    // the one user-visible knob still in play.
    let db_path = paths::db_path()?;
    let mut cfg = Config::default();
    cfg.server.port = port;
    let db = Db::open(&db_path)?;
    let state = AppState::init(db, cfg, port)?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    write_pid()?;
    vibe_core::server::serve(addr, state).await?;
    Ok(())
}

fn write_pid() -> Result<()> {
    let pid = std::process::id();
    let path = paths::pid_path()?;
    std::fs::write(path, pid.to_string())?;
    Ok(())
}
