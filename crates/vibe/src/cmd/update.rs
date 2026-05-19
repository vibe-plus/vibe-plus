use anyhow::Result;

use crate::npm_registry;

const PACKAGE: &str = "@vibe-plus/cli@latest";

pub fn run() -> Result<()> {
    let manager = npm_registry::package_manager();
    npm_registry::install_global(manager, PACKAGE)?;
    println!("Done. Run `vibe` (or `vibe up`) to launch the updated gateway.");
    println!("  If an older gateway is still listening, `vibe` will replace it automatically.");
    Ok(())
}
