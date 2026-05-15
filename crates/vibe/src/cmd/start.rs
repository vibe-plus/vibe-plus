use anyhow::Result;
use clap::Args;
use std::net::SocketAddr;
use vibe_core::{config::Config, paths, state::AppState};
use vibe_db::Db;

fn default_port() -> u16 {
    super::configured_port()
}

#[derive(Args)]
pub struct StartArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = default_port())]
    pub port: u16,

    /// Run in the foreground instead of daemonising.
    #[arg(long)]
    pub foreground: bool,
}

pub async fn run(args: StartArgs) -> Result<()> {
    let pid_path = paths::pid_path()?;
    if pid_path.exists() {
        let pid_s = std::fs::read_to_string(&pid_path).unwrap_or_default();
        let pid: u32 = pid_s.trim().parse().unwrap_or(0);
        if pid > 0 && is_alive(pid) {
            println!("vibe is already running (pid {pid}).");
            println!("  endpoint: http://127.0.0.1:{}", args.port);
            return Ok(());
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    if !args.foreground {
        return start_background_or_foreground(args.port).await;
    }

    start_server(args.port).await
}

pub async fn start_server(port: u16) -> Result<()> {
    let db_path = paths::db_path()?;
    let cfg_path = paths::config_path()?;
    let mut cfg = Config::load_or_init(&cfg_path)?;
    cfg.server.port = port;
    cfg.save(&cfg_path)?;
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

fn is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(unix)]
async fn start_background_or_foreground(port: u16) -> Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("start")
        .arg("--foreground")
        .arg("--port")
        .arg(port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    println!("vibe started in background.");
    println!("  endpoint: http://127.0.0.1:{port}");
    Ok(())
}

#[cfg(not(unix))]
async fn start_background_or_foreground(port: u16) -> Result<()> {
    // On Windows, run foreground for now; proper service wrapper is Phase 2.
    println!("Background mode not yet supported on Windows — running in foreground.");
    start_server(port).await
}
