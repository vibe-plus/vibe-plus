mod cmd;
mod npm_registry;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use vibe_i18n::text_env;

#[derive(Parser)]
#[command(
    name = "vibe",
    version,
    about = "vibe+ — local AI API gateway (control via the dashboard)",
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum CcSwitchCommand {
    /// Dump CC Switch extract summary (secrets redacted).
    Extract,
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
    /// Read CC Switch ~/.cc-switch database + settings (extraction smoke test).
    #[command(subcommand, name = "ccswitch")]
    CcSwitch(CcSwitchCommand),
    /// Guide Claude Code / OpenCode / Codex to use the local proxy.
    Takeover(cmd::takeover::TakeoverArgs),
    /// Tail the request log.
    Logs(cmd::logs::LogsArgs),
    /// Install or update Claude Code, Codex CLI, and/or Codex Desktop.
    #[command(name = "i", visible_alias = "install")]
    Install(cmd::install::InstallArgs),
    /// Update to the latest version via npm.
    Update,
    /// Open the vibe+ dashboard in the browser (picks fastest CDN).
    Ui,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cmd = Cli::command();
    cmd = cmd.about(cli_about());
    let cli =
        Cli::from_arg_matches(&cmd.get_matches()).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    init_tracing();
    match cli.command {
        None => cmd::up::run().await,
        Some(Command::Start(a)) => cmd::start::run(a).await,
        Some(Command::Stop) => cmd::stop::run().await,
        Some(Command::Status) => cmd::status::run().await,
        Some(Command::Statusline) => cmd::statusline::run(),
        Some(Command::Doctor) => cmd::doctor::run().await,
        Some(Command::CcSwitch(CcSwitchCommand::Extract)) => cmd::ccswitch_extract::run(),
        Some(Command::Takeover(a)) => cmd::takeover::run(a).await,
        Some(Command::Logs(a)) => cmd::logs::run(a).await,
        Some(Command::Install(a)) => cmd::install::run(a).await,
        Some(Command::Update) => cmd::update::run(),
        Some(Command::Ui) => cmd::ui::run().await,
    }
}

fn cli_about() -> String {
    text_env("cli-about")
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vibe=info,vibe_core=info".into()),
        )
        .init();
}
