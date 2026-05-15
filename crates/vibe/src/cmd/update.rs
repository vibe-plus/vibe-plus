use anyhow::Result;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

const PACKAGE: &str = "@vibe-plus/cli@latest";

/// Default npm registry URL — if the user's configured registry differs from
/// this we assume they've set a corporate / internal mirror and leave it alone.
const DEFAULT_NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// Candidate mirrors: (registry_url, tcp_host, tcp_port, display_name).
/// The first entry is always the official registry so we have a baseline.
const MIRRORS: &[(&str, &str, u16, &str)] = &[
    (
        "https://registry.npmjs.org/",
        "registry.npmjs.org",
        443,
        "npmjs.org (official)",
    ),
    (
        "https://registry.npmmirror.com/",
        "registry.npmmirror.com",
        443,
        "npmmirror.com (淘宝)",
    ),
    (
        "https://mirrors.cloud.tencent.com/npm/",
        "mirrors.cloud.tencent.com",
        443,
        "Tencent Cloud mirror",
    ),
    (
        "https://mirrors.huaweicloud.com/repository/npm/",
        "mirrors.huaweicloud.com",
        443,
        "Huawei Cloud mirror",
    ),
];

pub fn run() -> Result<()> {
    let manager = package_manager();

    // If the user already has a non-default registry configured (corporate
    // intranet, private Verdaccio, etc.) we must not override it.
    let registry_override = pick_registry(manager);

    let mut args: Vec<&str> = match manager {
        PackageManager::Bun => vec!["install", "-g", PACKAGE],
        PackageManager::Npm => vec!["install", "-g", PACKAGE],
    };

    // Build a temp String so we can push a &str pointing into it.
    let registry_flag: String;
    if let Some(ref url) = registry_override {
        registry_flag = url.clone();
        args.push("--registry");
        args.push(&registry_flag);
    }

    let command = match manager {
        PackageManager::Bun => "bun",
        PackageManager::Npm => "npm",
    };

    if !command_exists(command) {
        anyhow::bail!(
            "`{command}` was not found on PATH. Reinstall or update Vibe Plus with `{command} install -g {PACKAGE}`."
        );
    }

    println!(
        "Updating vibe to latest version with `{}`…",
        std::iter::once(command)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let status = std::process::Command::new(command).args(&args).status()?;
    if !status.success() {
        anyhow::bail!("{command} update failed");
    }
    println!("Done. Run `vibe --version` to confirm.");
    Ok(())
}

/// Returns a registry URL to pass via `--registry`, or `None` to let the
/// package manager use whatever it already has configured.
fn pick_registry(manager: PackageManager) -> Option<String> {
    // If the user has a custom registry set, don't touch it.
    if let Some(configured) = read_configured_registry(manager) {
        let normalized = configured.trim_end_matches('/');
        let default_normalized = DEFAULT_NPM_REGISTRY.trim_end_matches('/');
        if normalized != default_normalized {
            println!("Using your configured registry: {configured}");
            return None;
        }
    }

    // Speed-test all mirrors in parallel and pick the fastest.
    let winner = fastest_mirror();
    if let Some((url, name)) = winner {
        if url != DEFAULT_NPM_REGISTRY {
            println!("Using fastest mirror: {name} ({url})");
            return Some(url.to_string());
        }
    }
    None
}

/// Ask the package manager for its currently configured registry.
fn read_configured_registry(manager: PackageManager) -> Option<String> {
    // npm config get registry works for both npm and bun (bun reads .npmrc too).
    // We always probe npm because even bun users typically have .npmrc.
    let probe_cmd = match manager {
        PackageManager::Npm => "npm",
        // bun also supports `bun pm get-registry` but it's not stable across
        // versions; reading via npm is more reliable.
        PackageManager::Bun => {
            if command_exists("npm") { "npm" } else { return None; }
        }
    };
    std::process::Command::new(probe_cmd)
        .args(["config", "get", "registry"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "undefined")
}

/// TCP-connect to each mirror host concurrently, return the (url, name) of the
/// fastest one that responded within 3 seconds. Returns `None` only if every
/// host is unreachable (offline).
fn fastest_mirror() -> Option<(&'static str, &'static str)> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<(&'static str, &'static str, Duration)>();

    for &(url, host, port, name) in MIRRORS {
        let tx = tx.clone();
        std::thread::spawn(move || {
            if let Some(lat) = tcp_latency(host, port) {
                let _ = tx.send((url, name, lat));
            }
        });
    }
    drop(tx); // allow rx to drain when all threads finish

    // Collect all results that arrive within 4 seconds total.
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut best: Option<(&str, &str, Duration)> = None;
    while let Ok(result) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if best.as_ref().map_or(true, |(_, _, d)| result.2 < *d) {
            best = Some(result);
        }
    }

    best.map(|(url, name, lat)| {
        println!(
            "Mirror speed test: {} — {}ms (fastest)",
            name,
            lat.as_millis()
        );
        (url, name)
    })
}

/// Measure TCP connect latency to host:port. Returns None on failure.
fn tcp_latency(host: &str, port: u16) -> Option<Duration> {
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<_> = addr_str.to_socket_addrs().ok()?.collect();
    let addr = addrs.into_iter().next()?;
    let start = Instant::now();
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    Some(start.elapsed())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Npm,
    Bun,
}

fn package_manager() -> PackageManager {
    package_manager_from_signals(
        std::env::var_os("VIBE_MANAGED_BY_NPM").is_some(),
        std::env::var_os("VIBE_MANAGED_BY_BUN").is_some(),
        || command_exists("npm"),
        || command_exists("bun"),
    )
}

fn package_manager_from_signals(
    managed_by_npm: bool,
    managed_by_bun: bool,
    npm_exists: impl FnOnce() -> bool,
    bun_exists: impl FnOnce() -> bool,
) -> PackageManager {
    if managed_by_bun {
        return PackageManager::Bun;
    }

    if managed_by_npm {
        return PackageManager::Npm;
    }

    if npm_exists() {
        PackageManager::Npm
    } else if bun_exists() {
        PackageManager::Bun
    } else {
        PackageManager::Npm
    }
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new(command)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_by_bun_takes_precedence() {
        assert_eq!(
            package_manager_from_signals(true, true, || true, || true),
            PackageManager::Bun
        );
    }

    #[test]
    fn managed_by_npm_takes_precedence() {
        assert_eq!(
            package_manager_from_signals(true, false, || false, || true),
            PackageManager::Npm
        );
    }

    #[test]
    fn falls_back_to_bun_when_npm_is_missing() {
        assert_eq!(
            package_manager_from_signals(false, false, || false, || true),
            PackageManager::Bun
        );
    }

    #[test]
    fn default_registry_is_recognized() {
        // Normalized comparison must treat trailing slash as equal.
        let configured = "https://registry.npmjs.org/".to_string();
        let normalized = configured.trim_end_matches('/');
        let default_normalized = DEFAULT_NPM_REGISTRY.trim_end_matches('/');
        assert_eq!(normalized, default_normalized);
    }

    #[test]
    fn custom_registry_is_recognized() {
        let configured = "https://npm.corp.internal/".to_string();
        let normalized = configured.trim_end_matches('/');
        let default_normalized = DEFAULT_NPM_REGISTRY.trim_end_matches('/');
        assert_ne!(normalized, default_normalized);
    }
}
