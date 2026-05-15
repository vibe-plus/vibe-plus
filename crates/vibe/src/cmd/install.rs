use anyhow::Result;
use super::install_codex_app;
use clap::{Parser, ValueEnum};

use crate::npm_registry::{self, PackageManager};

const PKG_CLAUDE_CODE: &str = "@anthropic-ai/claude-code@latest";
const PKG_CODEX_CLI: &str = "@openai/codex@latest";

#[derive(Parser)]
pub struct InstallArgs {
    /// 安装目标：`claude`（Claude Code）、`codex`（Codex CLI）、`app`（Codex Desktop）。
    /// 省略则安装或更新全部三项。
    #[arg(value_enum, num_args = 0..)]
    pub targets: Vec<InstallTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum InstallTarget {
    /// Anthropic Claude Code CLI（`claude`）
    #[value(name = "claude", alias = "claude-code")]
    Claude,
    /// OpenAI Codex CLI（`codex`）
    #[value(name = "codex", alias = "codex-cli")]
    Codex,
    /// OpenAI Codex Desktop（macOS / Windows）
    #[value(name = "app", alias = "codex-app")]
    App,
}

impl InstallTarget {
    fn all() -> &'static [InstallTarget] {
        &[
            InstallTarget::Claude,
            InstallTarget::Codex,
            InstallTarget::App,
        ]
    }
}

pub async fn run(args: InstallArgs) -> Result<()> {
    let targets: Vec<InstallTarget> = if args.targets.is_empty() {
        InstallTarget::all().to_vec()
    } else {
        args.targets
    };

    println!("=== vibe i — 安装 / 更新 AI 客户端 ===\n");

    let manager = npm_registry::package_manager();
    ensure_package_manager(manager)?;

    let mut had_error = false;

    for target in targets {
        let result = match target {
            InstallTarget::Claude => install_claude_code(manager),
            InstallTarget::Codex => install_codex_cli(manager),
            InstallTarget::App => install_codex_app::install_or_update().await,
        };
        match result {
            Ok(()) => println!(),
            Err(err) => {
                eprintln!("✗ {target}: {err:#}\n");
                had_error = true;
            }
        }
    }

    if had_error {
        anyhow::bail!("部分安装失败，请查看上方错误信息");
    }

    println!("全部完成。");
    Ok(())
}

fn ensure_package_manager(manager: PackageManager) -> Result<()> {
    let cmd = match manager {
        PackageManager::Npm => "npm",
        PackageManager::Bun => "bun",
    };
    if npm_registry::command_exists(cmd) {
        return Ok(());
    }
    anyhow::bail!(
        "未找到 `{cmd}`。请先安装 Node.js（推荐附带 npm），或安装 bun：https://bun.sh"
    );
}

fn install_claude_code(manager: PackageManager) -> Result<()> {
    println!("→ Claude Code（{PKG_CLAUDE_CODE}）");
    npm_registry::install_global(manager, PKG_CLAUDE_CODE)?;
    print_binary_version("claude");
    Ok(())
}

fn install_codex_cli(manager: PackageManager) -> Result<()> {
    println!("→ Codex CLI（{PKG_CODEX_CLI}）");
    npm_registry::install_global(manager, PKG_CODEX_CLI)?;
    print_binary_version("codex");
    Ok(())
}

fn print_binary_version(binary: &str) {
    if !npm_registry::command_exists(binary) {
        eprintln!(
            "  提示：`{binary}` 不在 PATH 中，请确认全局 bin 目录已加入 shell 配置。"
        );
        return;
    }
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let version = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !version.is_empty() {
                println!("  版本：{version}");
            }
        }
        _ => {}
    }
}

impl std::fmt::Display for InstallTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallTarget::Claude => write!(f, "claude"),
            InstallTarget::Codex => write!(f, "codex"),
            InstallTarget::App => write!(f, "app"),
        }
    }
}
