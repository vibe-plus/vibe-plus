use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct RunArgs {
    /// Command and arguments to run.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

pub fn run(args: RunArgs) -> Result<()> {
    if args.command.is_empty() {
        anyhow::bail!("usage: vibe run -- <command> [args…]");
    }
    let endpoint = super::configured_base_url()?;
    let status = std::process::Command::new(&args.command[0])
        .args(&args.command[1..])
        .env("ANTHROPIC_BASE_URL", &endpoint)
        .env("OPENAI_BASE_URL", format!("{endpoint}/v1"))
        .env("OPENAI_API_BASE", format!("{endpoint}/v1"))
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}
