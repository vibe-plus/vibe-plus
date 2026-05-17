//! Default entry: start the gateway, take over known clients, and open the dashboard.

use anyhow::Result;

use super::{configured_port, gateway, ui};

pub async fn run() -> Result<()> {
    let port = configured_port();
    let base_url = gateway::ensure_running(port).await?;
    println!("vibe is ready at {base_url}");
    auto_takeover(&base_url);
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
