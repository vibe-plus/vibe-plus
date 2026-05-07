use anyhow::Result;

pub fn run() -> Result<()> {
    println!("Updating vibe to latest version…");
    let status = std::process::Command::new("npm")
        .args(["install", "-g", "vibe-cli@latest"])
        .status()?;
    if !status.success() {
        anyhow::bail!("npm install failed");
    }
    println!("Done. Run `vibe --version` to confirm.");
    Ok(())
}
