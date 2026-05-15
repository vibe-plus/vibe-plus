//! Shared helpers for talking to the local gateway process.

use anyhow::{Context, Result};
use std::time::{Duration, Instant};

pub fn local_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub async fn is_responsive(base_url: &str) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

pub async fn wait_until_ready(base_url: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if is_responsive(base_url).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    anyhow::bail!("gateway did not become ready at {base_url} within {timeout:?}");
}

pub async fn ensure_running(port: u16) -> Result<String> {
    let base_url = local_base_url(port);
    if is_responsive(&base_url).await {
        return Ok(base_url);
    }

    let pid_path = vibe_core::paths::pid_path().context("resolve pid path")?;
    if pid_path.exists() {
        let _ = std::fs::remove_file(&pid_path);
    }

    super::start::spawn_background(port)?;
    wait_until_ready(&base_url, Duration::from_secs(30)).await?;
    Ok(base_url)
}
