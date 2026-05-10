use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PairArgs {
    /// Pairing token from the hosted control plane.
    pub token: Option<String>,
}

pub fn run(_args: PairArgs) -> Result<()> {
    println!("Pairing with the hosted control plane is coming in Phase 2.");
    println!("This CLI build does not include a built-in UI.");
    println!("Use your external UI project against the local gateway endpoint:");
    println!("  http://127.0.0.1:{}", super::DEFAULT_PORT);
    Ok(())
}
