use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;
use vibe_core::codex_history::DEFAULT_PROVIDER_ID;
use vibe_protocol::CodexHistoryUnifyInput;

#[derive(Args)]
pub struct CodexHistoryArgs {
    #[command(subcommand)]
    pub command: CodexHistoryCmd,
}

#[derive(Subcommand)]
pub enum CodexHistoryCmd {
    /// Rewrite local Codex history metadata so all providers appear as vibeplus.
    Unify(UnifyArgs),
}

#[derive(Args, Debug, Clone)]
pub struct UnifyArgs {
    /// Provider id to write into Codex history metadata.
    #[arg(long, default_value = DEFAULT_PROVIDER_ID)]
    pub provider: String,
    /// Only rewrite these old provider ids. Repeat for multiple ids. Defaults to every non-target provider.
    #[arg(long = "from")]
    pub from_providers: Vec<String>,
    /// Actually write changes. Without this flag, only prints a dry-run summary.
    #[arg(long)]
    pub apply: bool,
    /// Do not create .bak files next to modified SQLite/JSONL files.
    #[arg(long)]
    pub no_backup: bool,
    /// Override the Codex home directory, defaults to ~/.codex.
    #[arg(long)]
    pub codex_home: Option<PathBuf>,
}

pub fn run(args: CodexHistoryArgs) -> Result<()> {
    match args.command {
        CodexHistoryCmd::Unify(args) => unify(args),
    }
}

fn unify(args: UnifyArgs) -> Result<()> {
    let input = CodexHistoryUnifyInput {
        provider: args.provider,
        from_providers: args.from_providers,
        apply: args.apply,
        no_backup: args.no_backup,
        codex_home: args
            .codex_home
            .map(|path| path.to_string_lossy().to_string()),
    };

    println!("=== vibe codex-history unify ===\n");
    let summary = vibe_core::codex_history::unify(input)?;
    println!("Codex home : {}", summary.codex_home);
    println!("Provider   : {}", summary.provider);
    if summary.from_providers.is_empty() {
        println!("From       : every non-{} provider", summary.provider);
    } else {
        println!("From       : {}", summary.from_providers.join(", "));
    }
    println!(
        "Mode       : {}",
        if summary.applied {
            "apply"
        } else {
            "dry-run (pass --apply to write)"
        }
    );
    println!();

    let verb = if summary.applied {
        "changed"
    } else {
        "would change"
    };
    println!(
        "SQLite     : {} file(s) scanned, {} row(s) {verb}",
        summary.sqlite_files_seen, summary.sqlite_rows_changed
    );
    println!(
        "Rollouts   : {} file(s) scanned, {} file(s) {verb}, {} field(s) {verb}",
        summary.rollout_files_seen, summary.rollout_files_changed, summary.rollout_fields_changed
    );
    if summary.applied {
        println!("Backups    : {} created", summary.backups_created);
        println!("\n[ok] Codex history provider metadata unified.");
    } else {
        println!("\nNo files changed. Re-run with `--apply` to write these updates.");
    }

    Ok(())
}
