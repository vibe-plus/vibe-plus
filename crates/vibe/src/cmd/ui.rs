use anyhow::Result;

pub fn run() -> Result<()> {
    let url = format!("http://127.0.0.1:{}/_vp/ui/", super::DEFAULT_PORT);
    println!("Opening {url}");
    open_browser(&url)?;
    Ok(())
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd").args(["/c", "start", url]).spawn()?;
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}
