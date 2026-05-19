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
        spawn_background(args.port)?;
        gateway::wait_until_ready(&base_url, Duration::from_secs(30)).await?;
        return Ok(());
    }

    run_server(args.port).await
}

/// Spawn `vibe up --foreground` detached from this terminal.
pub fn spawn_background(port: u16) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("up")
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

    println!("{}", text_env("cli-up-running-background"));
    println!("  endpoint: {}", gateway::local_base_url(port));
    Ok(())
}

pub async fn run_server(port: u16) -> Result<()> {
    if let Some(summary) = vibe_core::codex_history::try_auto_unify() {
        let changes = summary.sqlite_rows_changed + summary.rollout_fields_changed;
        if changes > 0 {
            tracing::info!(
                sqlite_rows = summary.sqlite_rows_changed,
                rollout_fields = summary.rollout_fields_changed,
                "codex history unified on gateway up"
            );
        }
    }

    // Config is in-memory only; the runtime defaults live in
    // `vibe_core::config::Config::default()`. The CLI `--port` flag below is
    // the one user-visible knob still in play.
    let db_path = paths::db_path()?;
    let observability_db_path = paths::observability_db_path()?;
    let body_dir = paths::bodies_dir()?;
    let mut cfg = Config::default();
    cfg.server.port = port;
    let observability = vibe_observability::ObservabilityStore::open(&observability_db_path)?;
    observability.migrate_from_legacy_path(&db_path)?;
    let db = Db::open(&db_path)?.with_body_store(body_dir);
    match db.migrate_inline_bodies_to_body_refs(10_000) {
        Ok(n) if n > 0 => tracing::info!(
            rows = n,
            "legacy inline log bodies moved to filesystem refs"
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!(?e, "legacy inline body migration failed on gateway up"),
    }
    if let Err(e) = db.prune_short_logs(&vibe_db::ShortLogRetentionPolicy::default()) {
        tracing::warn!(?e, "short log retention prune failed on gateway up");
    }
    let state = AppState::init_with_observability(db, cfg, port, observability)?;
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
