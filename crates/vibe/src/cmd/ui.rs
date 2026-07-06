use anyhow::Result;
use vibe_core::{UI_CDN_BASE_URL, UI_DASHBOARD_URL};

pub fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn()?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}

/// Open the hosted dashboard in the default browser.
pub async fn open_dashboard() -> Result<()> {
    println!("Opening {UI_DASHBOARD_URL}");
    open_url(UI_DASHBOARD_URL)
}

/// Open a hosted dashboard route in the default browser.
pub async fn open_dashboard_path(path: &str) -> Result<()> {
    let base = UI_CDN_BASE_URL.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let url = format!("{base}/{path}");
    println!("Opening {url}");
    open_url(&url)
}

pub async fn run() -> Result<()> {
    let port = super::configured_port();
    let base_url = super::gateway::ensure_running(port).await?;
    println!("vibe is ready at {base_url}");
    open_dashboard().await
}
