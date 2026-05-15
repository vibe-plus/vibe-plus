use anyhow::Result;
use std::time::Instant;

const CDN_CANDIDATES: &[(&str, &str)] = &[
    ("github", "https://vibe-plus.github.io/ui"),
    ("cheez.tech", "https://vibe-plus.cheez.tech/ui"),
];

/// Probe each CDN with a HEAD request and return the URL of the fastest one.
async fn pick_fastest_cdn() -> &'static str {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap_or_default();

    let mut handles = Vec::new();
    for (label, url) in CDN_CANDIDATES {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            let t = Instant::now();
            let ok = c.head(*url).send().await.is_ok();
            (label, url, t.elapsed().as_millis(), ok)
        }));
    }

    let mut best_url = CDN_CANDIDATES[0].1;
    let mut best_ms = u128::MAX;
    for h in handles {
        if let Ok((label, url, ms, true)) = h.await {
            if ms < best_ms {
                best_ms = ms;
                best_url = url;
            }
            tracing::debug!("CDN probe {label}: {ms}ms");
        }
    }
    best_url
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
    let url = pick_fastest_cdn().await;
    println!("Opening {url}");
    open_url(url)?;
    Ok(())
}
