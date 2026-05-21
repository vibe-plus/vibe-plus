//! `vibe db …` maintenance subcommands.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use vibe_core::paths;
use vibe_db::Db;

#[derive(Subcommand)]
pub enum DbCommand {
    /// Shrink the local DB: externalise inline bodies into the gzipped body
    /// store, then VACUUM. Safe to run while the gateway is stopped.
    Slim(SlimArgs),
}

#[derive(Args)]
pub struct SlimArgs {
    /// Report what would be done, but don't change anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip the body backfill — only run VACUUM.
    #[arg(long)]
    pub vacuum_only: bool,
}

pub async fn run(cmd: DbCommand) -> Result<()> {
    match cmd {
        DbCommand::Slim(args) => slim_run(args).await,
    }
}

async fn slim_run(args: SlimArgs) -> Result<()> {
    let pid_path = paths::pid_path()?;
    if pid_path.exists() {
        eprintln!(
            "[warn] {} exists — gateway may be running. Stop it with `vibe stop` first to avoid lock contention.",
            pid_path.display()
        );
    }

    let main_db_path = paths::db_path()?;
    let obs_db_path = paths::observability_db_path()?;

    println!("== Pre-slim sizes ==");
    let main_before = total_db_bytes(&main_db_path)?;
    let obs_before = single_file_bytes(&obs_db_path)?;
    println!(
        "  main DB (+short-logs):  {:>10}  ({})",
        human_bytes(main_before),
        main_db_path.display()
    );
    println!(
        "  observability DB:        {:>10}  ({})",
        human_bytes(obs_before),
        obs_db_path.display()
    );

    // We slim both DBs. Each holds its own short-conn body columns; they
    // share the filesystem body store at ~/.vibe/bodies.
    let mut total = SlimSummary::default();

    if main_db_path.exists() {
        println!("\n-- main DB --");
        slim_one(&main_db_path, false, &args, &mut total)?;
    } else {
        println!("\n-- main DB: not present, skipping --");
    }

    if obs_db_path.exists() {
        println!("\n-- observability DB --");
        slim_one(&obs_db_path, true, &args, &mut total)?;
    } else {
        println!("\n-- observability DB: not present, skipping --");
    }

    println!("\n== Summary ==");
    println!("  rows migrated:       {}", total.rows_migrated);
    println!(
        "  bytes externalised:  {} (compressed on disk by gzip)",
        human_bytes(total.bytes_externalised)
    );

    if !args.dry_run {
        println!("\n== Post-slim sizes ==");
        let main_after = total_db_bytes(&main_db_path)?;
        let obs_after = single_file_bytes(&obs_db_path)?;
        println!(
            "  main DB (+short-logs):  {:>10}  (was {})  Δ {}",
            human_bytes(main_after),
            human_bytes(main_before),
            signed_delta(main_after, main_before),
        );
        println!(
            "  observability DB:        {:>10}  (was {})  Δ {}",
            human_bytes(obs_after),
            human_bytes(obs_before),
            signed_delta(obs_after, obs_before),
        );
    } else {
        println!("\n(dry-run) no changes written.");
    }

    Ok(())
}

#[derive(Default)]
struct SlimSummary {
    rows_migrated: i64,
    bytes_externalised: i64,
}

fn slim_one(
    path: &Path,
    is_observability: bool,
    args: &SlimArgs,
    total: &mut SlimSummary,
) -> Result<()> {
    let db = if is_observability {
        let store = vibe_observability::ObservabilityStore::open(path)?;
        return store.with_legacy_db(|db| slim_open_db(db, path, args, total));
    } else {
        Db::open(path)?
    };
    slim_open_db(&db, path, args, total)
}

fn slim_open_db(db: &Db, path: &Path, args: &SlimArgs, total: &mut SlimSummary) -> Result<()> {
    let stats = db.slim_stats()?;
    println!(
        "  inline candidates: {} rows / {} (request_logs: {} rows, attempts: {} rows)",
        stats.inline_rows(),
        human_bytes(stats.inline_bytes()),
        stats.request_log_inline_rows,
        stats.upstream_attempt_inline_rows,
    );

    if args.vacuum_only {
        println!("  --vacuum-only: skipping backfill.");
    } else if args.dry_run {
        println!(
            "  --dry-run: would externalise {} rows ({}).",
            stats.inline_rows(),
            human_bytes(stats.inline_bytes())
        );
    } else if stats.inline_rows() == 0 {
        println!("  nothing to backfill.");
    } else {
        let report = db.slim_backfill()?;
        println!(
            "  externalised {} request_log rows + {} attempt rows ({} of inline data)",
            report.request_log_rows_migrated,
            report.upstream_attempt_rows_migrated,
            human_bytes(report.bytes_externalised),
        );
        total.rows_migrated +=
            report.request_log_rows_migrated + report.upstream_attempt_rows_migrated;
        total.bytes_externalised += report.bytes_externalised;
    }

    if args.dry_run {
        println!("  --dry-run: skipping VACUUM.");
    } else {
        println!("  running VACUUM…");
        db.vacuum()
            .with_context(|| format!("VACUUM on {}", path.display()))?;
        println!("  VACUUM done.");
    }
    Ok(())
}

fn total_db_bytes(main_path: &Path) -> Result<u64> {
    let mut total = 0u64;
    total += single_file_bytes(main_path)?;
    let short_path = short_logs_path_for(main_path);
    total += single_file_bytes(&short_path)?;
    // WAL / SHM if present
    for suffix in ["-wal", "-shm"] {
        let p = with_suffix(main_path, suffix);
        total += single_file_bytes(&p)?;
        let p2 = with_suffix(&short_path, suffix);
        total += single_file_bytes(&p2)?;
    }
    Ok(total)
}

fn single_file_bytes(path: &Path) -> Result<u64> {
    match fs::metadata(path) {
        Ok(m) => Ok(m.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e).with_context(|| format!("stat {}", path.display())),
    }
}

fn short_logs_path_for(main_path: &Path) -> PathBuf {
    let file_name = main_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("vibe.db");
    let short_name = if let Some(stem) = file_name.strip_suffix(".db") {
        format!("{stem}-short-logs.db")
    } else {
        format!("{file_name}-short-logs.db")
    };
    main_path
        .parent()
        .map(|p| p.join(&short_name))
        .unwrap_or_else(|| PathBuf::from(short_name))
}

fn with_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn human_bytes(n: impl Into<i128>) -> String {
    let n: i128 = n.into();
    let abs = n.unsigned_abs() as f64;
    let sign = if n < 0 { "-" } else { "" };
    if abs >= 1024.0 * 1024.0 * 1024.0 {
        format!("{sign}{:.2} GB", abs / (1024.0 * 1024.0 * 1024.0))
    } else if abs >= 1024.0 * 1024.0 {
        format!("{sign}{:.1} MB", abs / (1024.0 * 1024.0))
    } else if abs >= 1024.0 {
        format!("{sign}{:.1} KB", abs / 1024.0)
    } else {
        format!("{sign}{} B", abs as i64)
    }
}

fn signed_delta(after: u64, before: u64) -> String {
    let d = after as i128 - before as i128;
    let sign = if d > 0 { "+" } else { "" };
    format!("{sign}{}", human_bytes(d))
}
