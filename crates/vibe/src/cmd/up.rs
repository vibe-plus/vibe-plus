//! Default entry: start the gateway (if needed) and open the dashboard.

use anyhow::Result;

use super::{configured_port, gateway, ui};

pub async fn run() -> Result<()> {
    let port = configured_port();
    let base_url = gateway::ensure_running(port).await?;
    println!("vibe is ready at {base_url}");
    ui::open_dashboard().await
}
