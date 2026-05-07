use anyhow::Result;
use clap::Args;
use vibe_core::{config::Config, paths};

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub cmd: ConfigCmd,
}

#[derive(clap::Subcommand)]
pub enum ConfigCmd {
    /// Print a config value by key (e.g. server.port).
    Get { key: String },
    /// Set a config value.
    Set { key: String, value: String },
    /// Print path to the config file.
    Path,
}

pub fn run(args: ConfigArgs) -> Result<()> {
    let path = paths::config_path()?;
    let cfg = Config::load_or_init(&path)?;
    match args.cmd {
        ConfigCmd::Path => println!("{}", path.display()),
        ConfigCmd::Get { key } => {
            let v = serde_json::to_value(&cfg)?;
            let parts: Vec<&str> = key.split('.').collect();
            let mut cur = &v;
            for part in &parts {
                cur = cur.get(part).ok_or_else(|| anyhow::anyhow!("key not found: {key}"))?;
            }
            println!("{cur}");
        }
        ConfigCmd::Set { key, value } => {
            let mut v = serde_json::to_value(&cfg)?;
            let parts: Vec<&str> = key.split('.').collect();
            let (last, head) = parts.split_last().ok_or_else(|| anyhow::anyhow!("empty key"))?;
            let mut cur = &mut v;
            for part in head {
                cur = cur.get_mut(part).ok_or_else(|| anyhow::anyhow!("key not found: {part}"))?;
            }
            let new_val: serde_json::Value = serde_json::from_str(&value)
                .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
            cur[last] = new_val;
            let updated: Config = serde_json::from_value(v)?;
            updated.save(&path)?;
            println!("Set {key} = {value}");
        }
    }
    Ok(())
}
