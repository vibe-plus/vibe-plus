use anyhow::Result;
use vibe_core::{diagnostics, paths, VERSION};
use vibe_i18n::text_env;

use super::{configured_port, gateway};

pub async fn run() -> Result<()> {
    println!("=== {} ===\n", text_env("cli-doctor-title"));

    let port = configured_port();
    let base_url = gateway::local_base_url(port);

    print_check(
        "cli_version",
        true,
        &format!(
            "v{VERSION} ({})",
            std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".into())
        ),
    );

    for check in diagnostics::collect_startup_checks(port, VERSION) {
        print_check(check.name, check.ok, &check.detail);
    }

    if let Ok(log_path) = paths::gateway_log_path() {
        let detail = if log_path.exists() {
            format!("{}", log_path.display())
        } else {
            format!("{} (not created yet)", log_path.display())
        };
        print_check("gateway_log_path", true, &detail);
    }

    let pid_path = paths::pid_path()?;
    let running = if pid_path.exists() {
        let pid_s = std::fs::read_to_string(&pid_path).unwrap_or_default();
        let pid: u32 = pid_s.trim().parse().unwrap_or(0);
        #[cfg(unix)]
        let alive = pid > 0 && unsafe { libc::kill(pid as i32, 0) == 0 };
        #[cfg(not(unix))]
        let alive = false;
        if alive {
            print_check("process", true, &format!("running (pid {pid})"));
            true
        } else {
            print_check(
                "process",
                false,
                "pid file exists but process is dead (stale pid file)",
            );
            if let Some(tail) = diagnostics::read_gateway_log_tail(20) {
                println!("      {tail}");
            }
            false
        }
    } else {
        print_check("process", false, "not running");
        if let Some(tail) = diagnostics::read_gateway_log_tail(20) {
            println!("      {tail}");
        }
        false
    };

    if running {
        match reqwest::get(format!("{base_url}/health")).await {
            Ok(r) if r.status().is_success() => {
                print_check("gateway_health", true, &format!("reachable at {base_url}"));
                if let Some(remote) = gateway::fetch_running_version(&base_url).await {
                    let ok = remote == VERSION;
                    print_check(
                        "gateway_version",
                        ok,
                        &if ok {
                            format!("v{remote}")
                        } else {
                            format!(
                                "gateway reports v{remote}, CLI is v{VERSION} — run `vibe` to restart"
                            )
                        },
                    );
                }
            }
            Ok(r) => {
                print_check(
                    "gateway_health",
                    false,
                    &format!("unhealthy at {base_url} (HTTP {})", r.status()),
                );
            }
            Err(err) => {
                print_check(
                    "gateway_health",
                    false,
                    &format!("unreachable at {base_url}: {err}"),
                );
            }
        }
    } else {
        print_check(
            "gateway_health",
            false,
            &format!("skipped (not running at {base_url})"),
        );
    }

    let db_path = paths::db_path()?;
    if db_path.exists() {
        print_check("db_file", true, &db_path.display().to_string());
    } else {
        print_check("db_file", false, "not created yet (run `vibe up` first)");
    }

    println!();
    let any_fail = !diagnostics::collect_startup_checks(port, VERSION)
        .iter()
        .all(|c| c.ok)
        || !running;
    if any_fail {
        println!("Fix the [!!] items above, then run `vibe up` or `vibe up --foreground` for live errors.");
        println!("Dashboard (when gateway is up): `vibe ui` → providers / request logs.");
    } else {
        println!("All checks passed. Dashboard: `vibe ui`.");
    }
    Ok(())
}

fn print_check(name: &str, ok: bool, detail: &str) {
    let tag = if ok { "[ok] " } else { "[!!] " };
    println!("{tag} {name}: {detail}");
}
