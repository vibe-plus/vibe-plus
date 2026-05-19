use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct TakeoverArgs {
    /// Target client: claude, opencode, codex.
    pub client: String,
    /// Undo the takeover and restore from backup.
    #[arg(long)]
    pub restore: bool,
}

pub async fn run(args: TakeoverArgs) -> Result<()> {
    if args.restore {
        let outcome = vibe_core::takeover::restore(&args.client)?;
        println!(
            "[ok]  restored {} config {}",
            outcome.client,
            outcome
                .backup_path
                .as_deref()
                .map(|path| format!("from {path}"))
                .unwrap_or_else(|| "by removing vibe takeover entries".into())
        );
        return Ok(());
    }

    let base_url = super::configured_base_url()?;

    println!("=== vibe takeover: {} ===\n", args.client);

    match reqwest::get(format!("{base_url}/health")).await {
        Ok(r) if r.status().is_success() => {
            println!("[ok]  proxy is running at {base_url}");
        }
        _ => {
            println!("[!!]  warning: proxy not reachable at {base_url}");
            println!("      run `vibe up` first for the config to take effect.\n");
        }
    }

    let outcome = vibe_core::takeover::takeover(&args.client, &base_url)?;
    println!("Config : {}", outcome.config_path);
    if let Some(backup) = outcome.backup_path {
        println!("Backup : {backup}");
    }
    println!("Status : patched\n");

    print_next_steps(&args.client, &base_url);
    Ok(())
}

fn print_next_steps(client: &str, base_url: &str) {
    match client {
        "claude" => {
            println!("Next steps:");
            println!(
                "  Restart Claude Code — it will pick up ANTHROPIC_BASE_URL from settings.json."
            );
            println!("  All requests will route through vibe+ at {base_url}.");
            println!();
            println!("  Verify: claude -p \"say hi\"");
            println!("          then check: curl -s {base_url}/_vp/logs?limit=1 | jq");
        }
        "opencode" => {
            println!("Next steps:");
            println!("  Restart OpenCode — requests will be routed through vibe+, path {base_url}/opencode/v1");
        }
        "codex" => {
            println!("Next steps:");
            println!("  Restart codex — no login required; requests will be routed through vibe+ to {base_url}/codex/v1");
            println!();
            println!("  Verify: codex \"say hi\"");
            println!("          then check: curl -s {base_url}/_vp/logs?limit=1 | jq");
        }
        _ => {}
    }
    println!();
    println!("To undo: vibe takeover {client} --restore");
}
