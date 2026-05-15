mod cmd;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "vibe",
    version,
    about = "The Unified Toolchain for the Vibe — local AI API gateway",
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the local proxy daemon.
    Start(cmd::start::StartArgs),
    /// Stop the running daemon.
    Stop,
    /// Show running status.
    Status,
    /// Render Claude Code statusLine text from stdin JSON.
    Statusline,
    /// Diagnose config, port, and provider reachability.
    Doctor,
    /// Manage upstream providers.
    #[command(subcommand)]
    Provider(cmd::provider::ProviderCmd),
    /// Manage explicit model routing rules.
    #[command(subcommand)]
    Route(cmd::route::RouteCmd),
    /// Manage OS startup for the local proxy daemon.
    #[command(subcommand)]
    Autostart(cmd::autostart::AutostartCmd),
    /// Guide Claude Code / OpenCode / Codex to use the local proxy.
    Takeover(cmd::takeover::TakeoverArgs),
    /// Inspect or repair local Codex App history metadata.
    #[command(name = "codex-history")]
    CodexHistory(cmd::codex_history::CodexHistoryArgs),
    /// Inspect local client takeover state.
    #[command(subcommand)]
    Client(cmd::client::ClientCmd),
    /// Tail the request log.
    Logs(cmd::logs::LogsArgs),
    /// Get or set config values.
    Config(cmd::config::ConfigArgs),
    /// Update to the latest version via npm.
    Update,
    /// Open the vibe+ dashboard in the browser (picks fastest CDN).
    Ui,
    /// Pair with the hosted control plane (Phase 2 placeholder).
    Pair(cmd::pair::PairArgs),
    /// Run or install the Vibe Plus MCP server for Codex.
    Mcp(cmd::mcp::McpArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();
    match cli.command {
        None => cmd::up::run().await,
        Some(Command::Start(a)) => cmd::start::run(a).await,
        Some(Command::Stop) => cmd::stop::run().await,
        Some(Command::Status) => cmd::status::run().await,
        Some(Command::Statusline) => cmd::statusline::run(),
        Some(Command::Doctor) => cmd::doctor::run().await,
        Some(Command::Provider(c)) => cmd::provider::run(c).await,
        Some(Command::Route(c)) => cmd::route::run(c).await,
        Some(Command::Autostart(c)) => cmd::autostart::run(c),
        Some(Command::Takeover(a)) => cmd::takeover::run(a).await,
        Some(Command::CodexHistory(a)) => cmd::codex_history::run(a),
        Some(Command::Client(c)) => cmd::client::run(c).await,
        Some(Command::Logs(a)) => cmd::logs::run(a).await,
        Some(Command::Config(a)) => cmd::config::run(a),
        Some(Command::Update) => cmd::update::run(),
        Some(Command::Ui) => cmd::ui::run().await,
        Some(Command::Pair(a)) => cmd::pair::run(a),
        Some(Command::Mcp(a)) => cmd::mcp::run(a).await,
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
