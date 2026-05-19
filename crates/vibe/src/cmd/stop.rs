use anyhow::Result;
use vibe_i18n::text_env;

use super::{configured_port, gateway};

pub async fn run() -> Result<()> {
    let port = configured_port();
    let base_url = gateway::local_base_url(port);

    let was_responsive = gateway::is_responsive(&base_url).await;
    gateway::stop_at_port(port).await?;

    if gateway::is_responsive(&base_url).await {
        println!("vibe may still be running at {base_url}");
        if std::env::consts::OS == "windows" {
            println!("  try: taskkill /PID <pid> /F  or  vibe stop  after updating the CLI");
        }
    } else if was_responsive {
        println!("{}", text_env("cli-stop-stopped"));
    } else {
        println!("{}", text_env("cli-stop-not-running"));
    }

    Ok(())
}
