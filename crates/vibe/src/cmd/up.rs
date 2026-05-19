//! Default entry: bring up the gateway, take over known clients, and open the dashboard.

use anyhow::Result;

use super::{configured_port, daemon, gateway, ui};

#[derive(clap::Args)]
pub struct UpArgs {
    /// Port to listen on.
    #[arg(long, default_value_t = super::configured_port())]
    pub port: u16,

    /// Run the gateway in the foreground instead of daemonising.
    #[arg(long)]
    pub foreground: bool,
}

pub async fn run(args: UpArgs) -> Result<()> {
    if args.foreground || args.port != configured_port() {
        return daemon::run(daemon::UpArgs {
            port: args.port,
            foreground: args.foreground,
        })
        .await;
    }

    let port = configured_port();
    let base_url = gateway::ensure_running(port).await?;
    println!("vibe is ready at {base_url}");
    auto_takeover(&base_url);
    auto_unify_codex_history();
    ui::open_dashboard().await
}

/// Point every supported client at vibe+ (skips clients without a config file).
/// All errors are non-fatal — the dashboard opens regardless.
fn auto_takeover(base_url: &str) {
    for target in ["claude", "codex"] {
        match vibe_core::takeover::takeover(target, base_url) {
            Ok(_) => println!("  [takeover] {target} → vibe+"),
            Err(e) => eprintln!("  [warning]  {target}: {e}"),
        }
    }
}

/// Unify Codex local history metadata under `vibeplus`. Non-fatal.
fn auto_unify_codex_history() {
    let Some(summary) = vibe_core::codex_history::try_auto_unify() else {
        return;
    };
    let changes = summary.sqlite_rows_changed + summary.rollout_fields_changed;
    if changes == 0 {
        return;
    }
    println!(
        "  [codex-history] 已统一聊天记录（sqlite {} 行，rollout {} 处）",
        summary.sqlite_rows_changed, summary.rollout_fields_changed
    );
}

impl Default for UpArgs {
    fn default() -> Self {
        Self {
            port: configured_port(),
            foreground: false,
        }
    }
}
