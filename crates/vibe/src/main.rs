mod cmd;
mod npm_registry;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use vibe_i18n::text_env;

#[derive(Parser)]
#[command(
    name = "vibe",
    version,
    about = "Vibe Plus — local AI API gateway (control via the dashboard)",
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Parser)]
struct AutoUpdateChildArgs {
    #[arg(long, default_value_t = cmd::DEFAULT_PORT)]
    port: u16,

    #[arg(long)]
    expected_version: Option<String>,
}

#[derive(Subcommand)]
enum CcSwitchCommand {
    /// Dump CC Switch extract summary (secrets redacted).
    Extract,
}

#[derive(Subcommand)]
enum AutostartCommand {
    /// Register Vibe Plus to start at login (idempotent).
    Enable,
    /// Remove the login-item registration; future `vibe` runs won't re-add it.
    Disable,
    /// Show whether autostart is registered and live.
    Status,
}

#[derive(Subcommand)]
enum SetupCommand {
    /// List all setup steps and their state.
    Status,
    /// Run every pending setup step (interactive prompts kept to a minimum).
    All,
    /// Run a single setup step by id (see `vibe setup status`).
    Run {
        /// The step id, e.g. `cc-switch-import` or `autostart`.
        id: String,
    },
}

#[derive(Subcommand)]
enum Command {
    /// Internal helper used by the detached auto-updater.
    #[command(name = "auto-update-child", hide = true)]
    AutoUpdateChild(AutoUpdateChildArgs),
    /// Bring up Vibe Plus: gateway, client takeover, and dashboard.
    Up(cmd::up::UpArgs),
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
    /// Manage Vibe Plus login-item / autostart registration.
    #[command(subcommand)]
    Autostart(AutostartCommand),
    /// Versioned post-install / post-upgrade setup steps (CC Switch import, etc).
    #[command(subcommand)]
    Setup(SetupCommand),
    /// Local DB maintenance (slim / vacuum).
    #[command(subcommand)]
    Db(cmd::db::DbCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cmd = Cli::command();
    cmd = cmd.about(cli_about());
    let cli =
        Cli::from_arg_matches(&cmd.get_matches()).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    init_tracing();
    match cli.command {
        None => cmd::up::run(cmd::up::UpArgs::default()).await,
        Some(Command::AutoUpdateChild(a)) => {
            cmd::auto_update::run_updater_child(a.port, a.expected_version)
        }
        Some(Command::Up(a)) => cmd::up::run(a).await,
        Some(Command::Stop) => cmd::stop::run().await,
        Some(Command::Status) => cmd::status::run().await,
        Some(Command::Statusline) => cmd::statusline::run(),
        Some(Command::Doctor) => cmd::doctor::run().await,
        Some(Command::CcSwitch(CcSwitchCommand::Extract)) => cmd::ccswitch_extract::run(),
        Some(Command::Takeover(a)) => cmd::takeover::run(a).await,
        Some(Command::Logs(a)) => cmd::logs::run(a).await,
        Some(Command::Install(a)) => cmd::install::run(a).await,
        Some(Command::Update) => cmd::update::run().await,
        Some(Command::Ui) => cmd::ui::run().await,
        Some(Command::Autostart(c)) => match c {
            AutostartCommand::Enable => cmd::autostart::enable(),
            AutostartCommand::Disable => cmd::autostart::disable(),
            AutostartCommand::Status => cmd::autostart::status(),
        },
        Some(Command::Setup(c)) => match c {
            SetupCommand::Status => cmd::setup::print_status(),
            SetupCommand::All => cmd::setup::run_all_pending(),
            SetupCommand::Run { id } => cmd::setup::run_step(&id),
        },
        Some(Command::Db(c)) => cmd::db::run(c).await,
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
