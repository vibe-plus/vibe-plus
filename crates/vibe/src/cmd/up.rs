//! Default entry: start the gateway, auto-import local providers, and open the dashboard.

use anyhow::Result;
use std::time::Duration;

use super::{configured_port, gateway, ui};

pub async fn run() -> Result<()> {
    let port = configured_port();
    let base_url = gateway::ensure_running(port).await?;
    println!("vibe is ready at {base_url}");
    auto_setup(&base_url).await;
    ui::open_dashboard().await
}

/// Scan local AI clients, import their credentials, and point them at vibe+.
/// All errors are non-fatal — the dashboard opens regardless.
async fn auto_setup(base_url: &str) {
    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    // Discover locally installed providers (Claude, Codex, CC Switch profiles).
    let candidates: Vec<serde_json::Value> = match http
        .get(format!("{base_url}/_vp/providers/import-local"))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => return,
    };

    if candidates.is_empty() {
        return;
    }

    // Import (or refresh) all detected providers into the gateway DB.
    let client_names: Vec<String> = candidates
        .iter()
        .filter_map(|c| c["client"].as_str().map(str::to_string))
        .collect();

    if let Ok(resp) = http
        .post(format!("{base_url}/_vp/providers/import-local"))
        .json(&client_names)
        .send()
        .await
    {
        if resp.status().is_success() {
            let imported: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
            for p in &imported {
                println!("  [import]   {}", p["name"].as_str().unwrap_or("?"));
            }
        }
    }

    // Point each detected client at vibe+ (skip if already taken over).
    for c in &candidates {
        let client_name = c["client"].as_str().unwrap_or("");
        let target = match client_name {
            "claude" | "codex" => client_name,
            _ => continue,
        };

        let already_taken = match http
            .get(format!("{base_url}/_vp/clients/{target}/status"))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v["taken_over"].as_bool())
                .unwrap_or(false),
            _ => false,
        };

        if already_taken {
            continue;
        }

        match vibe_core::takeover::takeover(target, base_url) {
            Ok(_) => println!("  [takeover] {target} → vibe+"),
            Err(e) => eprintln!("  [warning]  {target}: {e}"),
        }
    }
}
