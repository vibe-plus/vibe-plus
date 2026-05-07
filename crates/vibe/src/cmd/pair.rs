use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PairArgs {
    /// Pairing token from the hosted control plane.
    pub token: Option<String>,
}

pub fn run(_args: PairArgs) -> Result<()> {
    println!("Pairing with the hosted control plane is coming in Phase 2.");
    println!("For now, use the local dashboard at:");
    println!("  http://127.0.0.1:{}/_vp/ui/", super::DEFAULT_PORT);
    Ok(())
}
