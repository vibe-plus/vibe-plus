use anyhow::Result;

use crate::npm_registry;

const PACKAGE: &str = "@vibe-plus/cli@latest";

pub fn run() -> Result<()> {
    let manager = npm_registry::package_manager();
    npm_registry::install_global(manager, PACKAGE)?;
    println!("完成。运行 `vibe --version` 确认版本。");
    Ok(())
}
