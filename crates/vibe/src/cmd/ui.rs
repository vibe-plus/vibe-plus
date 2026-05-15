use anyhow::Result;
use std::time::Instant;
use vibe_core::{UI_CDN_BASE_URL, UI_CDN_MIRROR_BASE_URL, UI_DASHBOARD_MIRROR_URL, UI_DASHBOARD_URL};

const CDN_BASES: &[(&str, &str, &str)] = &[
    ("github", UI_CDN_BASE_URL, UI_DASHBOARD_URL),
    (
        "cheez.tech",
        UI_CDN_MIRROR_BASE_URL,
        UI_DASHBOARD_MIRROR_URL,
    ),
];

/// Probe each CDN `version.json` and return the dashboard URL for the fastest healthy origin.
async fn pick_dashboard_url() -> &'static str {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap_or_default();

    let mut handles = Vec::new();
    for (label, base, dashboard) in CDN_BASES {
        let c = client.clone();
        let probe_url = format!("{base}version.json");
        handles.push(tokio::spawn(async move {
            let t = Instant::now();
            let ok = c
                .head(&probe_url)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false);
            (label, dashboard, t.elapsed().as_millis(), ok)
        }));
    }

    let mut best = CDN_BASES[0].2;
    let mut best_ms = u128::MAX;
    for h in handles {
        if let Ok((label, dashboard, ms, true)) = h.await {
            if ms < best_ms {
                best_ms = ms;
                best = dashboard;
            }
            tracing::debug!("CDN probe {label}: {ms}ms");
        }
    }
    best
}

fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd").args(["/c", "start", url]).spawn()?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}

pub async fn run() -> Result<()> {
    println!("Probing CDN speed…");
    let url = pick_dashboard_url().await;
    println!("Opening {url}");
    open_url(url)?;
    Ok(())
}
