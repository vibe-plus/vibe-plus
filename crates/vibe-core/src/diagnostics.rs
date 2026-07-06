//! CLI / doctor diagnostics: DB schema compatibility, local proxy bypass, gateway logs.

use anyhow::{Context, Result};
use std::path::Path;
use vibe_db::{Db, DbSchemaInspect};
use vibe_i18n::{detect_locale_from_env, ZH_CN_LOCALE};

use crate::paths;

#[derive(Debug, Clone)]
pub struct DiagnosticCheck {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// Fail fast before spawning a background gateway that will exit immediately.
pub fn preflight_gateway_start(cli_version: &str) -> Result<()> {
    let db_path = paths::db_path()?;
    let schemas = Db::inspect_related_schemas(&db_path)?;
    for (path, inspect) in schemas {
        if inspect.is_too_far_ahead() {
            anyhow::bail!("{}", db_too_far_ahead_message(cli_version, &path, inspect));
        }
    }
    Ok(())
}

/// Checks run by `vibe doctor` and appended when background startup times out.
pub fn collect_startup_checks(port: u16, cli_version: &str) -> Vec<DiagnosticCheck> {
    let mut checks = Vec::new();
    checks.extend(db_schema_checks(cli_version));
    checks.push(local_proxy_bypass_check(port));
    checks
}

pub fn startup_failure_message(port: u16, cli_version: &str) -> String {
    let zh = detect_locale_from_env().to_string() == ZH_CN_LOCALE;
    let log_path = paths::gateway_log_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "~/.vibe/logs/gateway.log".into());

    let mut lines = Vec::new();
    if zh {
        lines.push("网关未在预期时间内就绪。可能原因：".into());
    } else {
        lines.push("Gateway did not become ready in time. Likely causes:".into());
    }

    for check in collect_startup_checks(port, cli_version) {
        if check.ok {
            continue;
        }
        lines.push(format!("  • [{}] {}", check.name, check.detail));
    }

    if let Some(tail) = read_gateway_log_tail(40) {
        lines.push(format!("  • [gateway_log]\n{tail}"));
    }

    if zh {
        lines.push(format!(
            "完整日志：{log_path}（可用 `tail -f {log_path}` 查看；前台调试：`vibe up --foreground`）"
        ));
    } else {
        lines.push(format!(
            "Full log: {log_path} (`tail -f {log_path}`; debug in foreground: `vibe up --foreground`)"
        ));
    }
    lines.join("\n")
}

fn db_schema_checks(cli_version: &str) -> Vec<DiagnosticCheck> {
    let Ok(db_path) = paths::db_path() else {
        return vec![DiagnosticCheck {
            name: "db_schema",
            ok: false,
            detail: "could not resolve ~/.vibe path".into(),
        }];
    };
    let Ok(schemas) = Db::inspect_related_schemas(&db_path) else {
        return vec![DiagnosticCheck {
            name: "db_schema",
            ok: false,
            detail: "could not read database schema versions".into(),
        }];
    };
    if schemas.is_empty() {
        return vec![DiagnosticCheck {
            name: "db_schema",
            ok: true,
            detail: "no database yet (will be created on first start)".into(),
        }];
    }
    schemas
        .into_iter()
        .map(|(path, inspect)| {
            let file = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("vibe.db");
            if inspect.is_too_far_ahead() {
                DiagnosticCheck {
                    name: "db_schema",
                    ok: false,
                    detail: db_too_far_ahead_message(cli_version, &path, inspect),
                }
            } else if inspect.pending_upgrades() > 0 {
                DiagnosticCheck {
                    name: "db_schema",
                    ok: true,
                    detail: format!(
                        "{file}: schema v{} (CLI supports v{}; {} migration(s) will run on start)",
                        inspect.db_user_version,
                        inspect.embedded_max_version,
                        inspect.pending_upgrades()
                    ),
                }
            } else {
                DiagnosticCheck {
                    name: "db_schema",
                    ok: true,
                    detail: format!(
                        "{file}: schema v{} matches CLI v{cli_version}",
                        inspect.db_user_version
                    ),
                }
            }
        })
        .collect()
}

fn db_too_far_ahead_message(cli_version: &str, path: &Path, inspect: DbSchemaInspect) -> String {
    let zh = detect_locale_from_env().to_string() == ZH_CN_LOCALE;
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("vibe.db");
    if zh {
        format!(
            "{file} 的 schema 版本为 v{}，但当前 CLI v{cli_version} 只支持到 v{}。\
             通常是因为用较新的 Vibe Plus 跑过网关后又用旧版 CLI 启动。\
             请升级：`bun install -g @vibe-plus/cli@latest` 或在 vibe-plus 目录执行 `bun gateway:restart`。\
             勿删除 ~/.vibe/vibe.db，除非你愿意重新配置 Provider。",
            inspect.db_user_version, inspect.embedded_max_version
        )
    } else {
        format!(
            "{file} schema is v{} but this CLI v{cli_version} only supports up to v{}. \
             A newer Vibe Plus build likely migrated the DB. \
             Upgrade with `bun install -g @vibe-plus/cli@latest` or run `bun gateway:restart` from the vibe-plus repo. \
             Do not delete ~/.vibe/vibe.db unless you are OK reconfiguring providers.",
            inspect.db_user_version, inspect.embedded_max_version
        )
    }
}

/// Codex/reqwest honor system HTTP proxies; Clash etc. often break `127.0.0.1:<port>`.
pub fn local_proxy_bypass_check(port: u16) -> DiagnosticCheck {
    let health = format!("http://127.0.0.1:{port}/health");
    let direct = curl_http_status(&health, None);
    if direct != 200 {
        return DiagnosticCheck {
            name: "local_proxy_bypass",
            ok: false,
            detail: format!(
                "direct gateway health check failed (HTTP {direct}); ensure the gateway is running"
            ),
        };
    }

    let Some(proxy) = detect_system_http_proxy() else {
        return DiagnosticCheck {
            name: "local_proxy_bypass",
            ok: true,
            detail: "no system HTTP proxy detected; localhost gateway is reachable".into(),
        };
    };

    let via_proxy = curl_http_status(&health, Some(&proxy));
    if via_proxy == 502 {
        return DiagnosticCheck {
            name: "local_proxy_bypass",
            ok: false,
            detail: format!(
                "system proxy {proxy} breaks requests to the local Vibe gateway (HTTP 502). \
                 Run: launchctl setenv NO_PROXY \"127.0.0.1,localhost,127.0.0.0/8,::1\" \
                 then fully quit and restart Codex/Cursor"
            ),
        };
    }

    DiagnosticCheck {
        name: "local_proxy_bypass",
        ok: true,
        detail: format!("system proxy {proxy} does not break localhost gateway ({via_proxy})"),
    }
}

pub fn gateway_log_len() -> Option<u64> {
    let path = paths::gateway_log_path().ok()?;
    if !path.exists() {
        return Some(0);
    }
    std::fs::metadata(&path)
        .ok()
        .map(|meta| meta.len())
}

pub fn read_gateway_log_tail(max_lines: usize) -> Option<String> {
    let path = paths::gateway_log_path().ok()?;
    format_log_tail(&path, None, max_lines)
}

/// Bytes written to the gateway log after `byte_offset` (from a pre-spawn snapshot).
pub fn read_gateway_log_since(byte_offset: u64, max_lines: usize) -> Option<String> {
    let path = paths::gateway_log_path().ok()?;
    format_log_tail(&path, Some(byte_offset), max_lines)
}

fn format_log_tail(path: &Path, byte_offset: Option<u64>, max_lines: usize) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let slice = match byte_offset {
        Some(offset) => {
            let offset = usize::try_from(offset).unwrap_or(text.len());
            if offset >= text.len() {
                return None;
            }
            text[offset..].trim()
        }
        None => text.trim(),
    };
    if slice.is_empty() {
        return None;
    }
    let lines: Vec<&str> = slice.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    let label = if byte_offset.is_some() {
        "gateway output since spawn"
    } else {
        "last"
    };
    Some(format!(
        "{label} {} line(s) from {}:\n{}",
        lines.len() - start,
        path.display(),
        lines[start..].join("\n")
    ))
}

/// Message when the detached `vibe up --foreground` child exits before `/health` is ready.
pub fn gateway_spawn_failed_message(
    port: u16,
    cli_version: &str,
    log_since_spawn: Option<&str>,
) -> String {
    let zh = detect_locale_from_env().to_string() == ZH_CN_LOCALE;
    let log_path = paths::gateway_log_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "~/.vibe/logs/gateway.log".into());

    let mut lines = Vec::new();
    if zh {
        lines.push("网关进程在就绪前已退出。".into());
    } else {
        lines.push("Gateway process exited before becoming ready.".into());
    }

    for check in collect_startup_checks(port, cli_version) {
        if check.ok {
            continue;
        }
        lines.push(format!("  • [{}] {}", check.name, check.detail));
    }

    if let Some(tail) = log_since_spawn.filter(|t| !t.trim().is_empty()) {
        lines.push(format!("  • [{tail}]"));
    } else if let Some(tail) = read_gateway_log_tail(40) {
        lines.push(format!("  • [gateway_log]\n{tail}"));
    }

    if zh {
        lines.push(format!(
            "完整日志：{log_path}；前台调试：`vibe up --foreground`"
        ));
    } else {
        lines.push(format!(
            "Full log: {log_path}; debug in foreground: `vibe up --foreground`"
        ));
    }
    lines.join("\n")
}

pub fn open_gateway_log_append() -> Result<std::fs::File> {
    let path = paths::gateway_log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create logs dir {}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open gateway log {}", path.display()))
}

fn curl_http_status(url: &str, proxy: Option<&str>) -> u16 {
    let mut cmd = std::process::Command::new("curl");
    cmd.args([
        "-s",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        "--max-time",
        "3",
    ]);
    if let Some(proxy) = proxy {
        cmd.args(["-x", proxy]);
    }
    cmd.arg(url);
    cmd.output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn detect_system_http_proxy() -> Option<String> {
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    for port in [7892_u16, 7890, 7897, 1087] {
        if local_tcp_port_listening(port) {
            return Some(format!("http://127.0.0.1:{port}"));
        }
    }
    None
}

#[cfg(unix)]
fn local_tcp_port_listening(port: u16) -> bool {
    std::process::Command::new("lsof")
        .args(["-iTCP", &format!(":{port}"), "-sTCP:LISTEN", "-t"])
        .output()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn local_tcp_port_listening(_port: u16) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_message_includes_db_hint_when_schema_ahead() {
        let msg = startup_failure_message(15917, "0.0.5");
        assert!(!msg.is_empty());
    }

    #[test]
    fn spawn_failed_message_includes_log_excerpt() {
        let msg = gateway_spawn_failed_message(
            15917,
            "0.0.5",
            Some("Error: DatabaseTooFarAhead"),
        );
        assert!(msg.contains("DatabaseTooFarAhead"));
        assert!(msg.contains("exited before becoming ready")
            || msg.contains("网关进程在就绪前已退出"));
    }
}
