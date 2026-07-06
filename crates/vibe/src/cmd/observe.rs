//! Open observability without running client takeover.

use anyhow::Result;

use super::{configured_port, gateway, ui};

pub async fn run() -> Result<()> {
    let port = configured_port();
    let base_url = gateway::ensure_running(port).await?;
    println!("vibe is ready at {base_url}");
    ui::open_dashboard_path("ui/observability").await
}
