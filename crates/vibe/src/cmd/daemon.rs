use anyhow::{Context, Result};
use std::io::Write;
use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;
use vibe_core::{config::Config, diagnostics, paths, state::AppState, VERSION};
use vibe_db::Db;
use vibe_i18n::text_env;

use super::{auto_update, gateway};

fn default_port() -> u16 {
    super::configured_port()
}

#[derive(clap::Args)]
pub struct UpArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = default_port())]
    pub port: u16,

    /// Run in the foreground instead of daemonising.
    #[arg(long)]
    pub foreground: bool,
}

pub async fn run(args: UpArgs) -> Result<()> {
    let base_url = gateway::local_base_url(args.port);
    if gateway::is_responsive(&base_url).await {
        if let Some(running) = gateway::fetch_running_version(&base_url).await {
            if running != gateway::current_version() {
                println!(
                    "{}",
                    gateway::upgrade_restart_message(&running, gateway::current_version())
                );
                gateway::stop_at_port(args.port).await?;
                gateway::wait_until_stopped(&base_url, Duration::from_secs(15)).await?;
            } else {
                println!("{}", text_env("cli-up-already-running"));
                println!("  endpoint: {base_url}");
                return Ok(());
            }
        } else {
            println!("{}", text_env("cli-up-already-running"));
            println!("  endpoint: {base_url}");
            return Ok(());
        }
    }

    let pid_path = paths::pid_path()?;
    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }

    if !args.foreground {
        diagnostics::preflight_gateway_start(VERSION)?;
        let spawn = spawn_background(args.port)?;
        gateway::wait_until_ready(
            &base_url,
            args.port,
            Duration::from_secs(30),
            Some(spawn),
        )
        .await?;
        return Ok(());
    }

    run_server(args.port).await
}

/// Spawn `vibe up --foreground` detached from this terminal.
pub fn spawn_background(port: u16) -> Result<gateway::BackgroundGatewaySpawn> {
    let log_offset = diagnostics::gateway_log_len().unwrap_or(0);
    let log_file = diagnostics::open_gateway_log_append()?;
    {
        let mut header = log_file
            .try_clone()
            .context("clone gateway log handle for startup banner")?;
        let _ = writeln!(header, "\n--- vibe up --foreground port={port} ---");
    }
    let log_stderr = log_file
        .try_clone()
        .context("clone gateway log handle for stderr")?;

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("up")
        .arg("--foreground")
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_stderr));

    #[cfg(unix)]
    let child_pid = {
        let child = cmd.spawn()?;
        child.id()
    };

    #[cfg(windows)]
    let child_pid = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let child = cmd
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn()?;
        child.id()
    };

    println!("{}", text_env("cli-up-running-background"));
    println!("  endpoint: {}", gateway::local_base_url(port));
    Ok(gateway::BackgroundGatewaySpawn {
        child_pid,
        log_offset,
    })
}

pub async fn run_server(port: u16) -> Result<()> {
    let db_path = paths::db_path()?;
    let observability_db_path = paths::observability_db_path()?;
    let body_dir = paths::bodies_dir()?;
    let mut cfg = Config::default();
    cfg.server.port = port;
    let observability = match vibe_observability::ObservabilityStore::open(&observability_db_path) {
        Ok(store) => Some(store),
        Err(e) => {
            tracing::warn!(?e, "observability plugin unavailable; gateway will run without persistent observability");
            None
        }
    };
    let db = Db::open(&db_path)?.with_body_store(body_dir);
    let state = AppState::init_with_optional_observability(db, cfg, port, observability)?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    write_pid()?;
    auto_update::spawn_background_checker(port);
    vibe_core::server::serve(addr, state).await?;
    Ok(())
}

fn write_pid() -> Result<()> {
    let pid = std::process::id();
    let path = paths::pid_path()?;
    std::fs::write(path, pid.to_string())?;
    Ok(())
}
