use anyhow::Result;

use crate::cmd::auto_update;
use crate::npm_registry;
use vibe_i18n::{text_env, text_env_args};

const PACKAGE: &str = "@vibe-plus/cli@latest";

pub async fn run() -> Result<()> {
    println!("{}", text_env("cli-update-checking"));

    match auto_update::fetch_latest_version().await {
        Ok(Some(latest)) => {
            let current = vibe_core::VERSION;
            if !auto_update::is_newer(&latest, current) {
                println!(
                    "{}",
                    text_env_args(
                        "cli-update-already-latest",
                        &[("current", current), ("remote", latest.as_str())],
                    )
                );
                return Ok(());
            }
            println!(
                "{}",
                text_env_args(
                    "cli-update-upgrading",
                    &[("current", current), ("remote", latest.as_str())],
                )
            );
        }
        Ok(None) => {
            eprintln!("{}", text_env("cli-update-no-latest"));
        }
        Err(err) => {
            eprintln!(
                "{}",
                text_env_args("cli-update-check-failed", &[("error", &format!("{err:#}"))])
            );
        }
    }

    let manager = npm_registry::package_manager();
    npm_registry::install_global(manager, PACKAGE)?;
    println!("{}", text_env("cli-update-done"));
    Ok(())
}
