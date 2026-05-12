use anyhow::Result;
use vibe_core::paths;

pub async fn run() -> Result<()> {
    let pid_path = paths::pid_path()?;
    if !pid_path.exists() {
        println!("vibe is not running.");
        return Ok(());
    }
    let base_url = super::configured_base_url()?;
    let url = format!("{base_url}/status");
    match reqwest::get(&url).await {
        Ok(r) => {
            let body: serde_json::Value = r.json().await?;
            println!(
                "version:          {}",
                body["version"].as_str().unwrap_or("?")
            );
            println!(
                "uptime:           {}s",
                body["uptime_secs"].as_u64().unwrap_or(0)
            );
            println!("endpoint:         {base_url}");
            println!(
                "providers:        {} total / {} enabled",
                body["providers_total"].as_u64().unwrap_or(0),
                body["providers_enabled"].as_u64().unwrap_or(0)
            );
            println!(
                "requests/hour:    {}",
                body["requests_last_hour"].as_u64().unwrap_or(0)
            );
        }
        Err(_) => {
            println!("vibe pid file found but server is not responding at {base_url}.");
        }
    }
    Ok(())
}
