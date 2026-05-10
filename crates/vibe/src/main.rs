mod cmd;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "vibe",
    version,
    about = "The Unified Toolchain for the Vibe — local AI API gateway"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the local proxy daemon.
    Start(cmd::start::StartArgs),
    /// Stop the running daemon.
    Stop,
    /// Show running status.
    Status,
    /// Diagnose config, port, and provider reachability.
    Doctor,
    /// Manage upstream providers.
    #[command(subcommand)]
    Provider(cmd::provider::ProviderCmd),
    /// Guide Claude Code / OpenCode / Codex to use the local proxy.
    Takeover(cmd::takeover::TakeoverArgs),
    /// Tail the request log.
    Logs(cmd::logs::LogsArgs),
    /// Run a subprocess with local proxy env vars injected.
    Run(cmd::run::RunArgs),
    /// Get or set config values.
    Config(cmd::config::ConfigArgs),
    /// Update to the latest version via npm.
    Update,
    /// Pair with the hosted control plane (Phase 2 placeholder).
    Pair(cmd::pair::PairArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();
    match cli.command {
        Command::Start(a) => cmd::start::run(a).await,
        Command::Stop => cmd::stop::run(),
        Command::Status => cmd::status::run().await,
        Command::Doctor => cmd::doctor::run().await,
        Command::Provider(c) => cmd::provider::run(c).await,
        Command::Takeover(a) => cmd::takeover::run(a).await,
        Command::Logs(a) => cmd::logs::run(a).await,
        Command::Run(a) => cmd::run::run(a),
        Command::Config(a) => cmd::config::run(a),
        Command::Update => cmd::update::run(),
        Command::Pair(a) => cmd::pair::run(a),
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vibe=info,vibe_core=info".into()),
        )
        .init();
}
