use anyhow::Result;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

/// Default npm registry URL — if the user's configured registry differs from
/// this we assume they've set a corporate / internal mirror and leave it alone.
pub const DEFAULT_NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// Candidate mirrors: (registry_url, tcp_host, tcp_port, display_name).
pub const MIRRORS: &[(&str, &str, u16, &str)] = &[
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Npm,
    Bun,
}

pub fn package_manager() -> PackageManager {
    package_manager_from_signals(
        std::env::var_os("VIBE_MANAGED_BY_NPM").is_some(),
        std::env::var_os("VIBE_MANAGED_BY_BUN").is_some(),
        || command_exists("npm"),
        || command_exists("bun"),
    )
}

pub fn package_manager_from_signals(
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

/// Returns a registry URL to pass via `--registry`, or `None` to let the
/// package manager use whatever it already has configured.
pub fn pick_registry(manager: PackageManager) -> Option<String> {
    if let Some(configured) = read_configured_registry(manager) {
        let normalized = configured.trim_end_matches('/');
        let default_normalized = DEFAULT_NPM_REGISTRY.trim_end_matches('/');
        if normalized != default_normalized {
            println!("使用你已配置的 registry：{configured}");
            return None;
        }
    }

    let winner = fastest_mirror();
    if let Some((url, name)) = winner {
        if url != DEFAULT_NPM_REGISTRY {
            println!("使用测速最快的 npm 镜像：{name} ({url})");
            return Some(url.to_string());
        }
    }
    None
}

pub fn install_global(manager: PackageManager, package: &str) -> Result<()> {
    let registry_override = pick_registry(manager);

    let mut args: Vec<&str> = match manager {
        PackageManager::Bun => vec!["install", "-g", package],
        PackageManager::Npm => vec!["install", "-g", package],
    };

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
            "未找到 `{command}`。请先安装 Node.js 包管理器，或手动执行：`{command} install -g {package}`"
        );
    }

    println!(
        "正在安装/更新 {package}（`{}`）…",
        std::iter::once(command)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let status = std::process::Command::new(command).args(&args).status()?;
    if !status.success() {
        anyhow::bail!("{command} 安装失败（退出码 {status}）");
    }
    Ok(())
}

fn read_configured_registry(manager: PackageManager) -> Option<String> {
    let probe_cmd = match manager {
        PackageManager::Npm => "npm",
        PackageManager::Bun => {
            if command_exists("npm") {
                "npm"
            } else {
                return None;
            }
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

pub fn fastest_mirror() -> Option<(&'static str, &'static str)> {
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
    drop(tx);

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut best: Option<(&str, &str, Duration)> = None;
    while let Ok(result) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if best.as_ref().map_or(true, |(_, _, d)| result.2 < *d) {
            best = Some(result);
        }
    }

    best.map(|(url, name, lat)| {
        println!(
            "npm 镜像测速：{} — {}ms（最快）",
            name,
            lat.as_millis()
        );
        (url, name)
    })
}

pub fn fastest_endpoint(
    candidates: &[(&'static str, &'static str, u16, &'static str)],
) -> Option<&'static str> {
    use std::sync::mpsc;
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() == 1 {
        return Some(candidates[0].0);
    }

    let (tx, rx) = mpsc::channel::<(&'static str, &'static str, Duration)>();
    for &(url, host, port, name) in candidates {
        let tx = tx.clone();
        std::thread::spawn(move || {
            if let Some(lat) = tcp_latency(host, port) {
                let _ = tx.send((url, name, lat));
            }
        });
    }
    drop(tx);

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut best: Option<(&str, &str, Duration)> = None;
    while let Ok(result) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if best.as_ref().map_or(true, |(_, _, d)| result.2 < *d) {
            best = Some(result);
        }
    }

    best.map(|(url, name, lat)| {
        println!("下载源测速：{name} — {}ms（最快）", lat.as_millis());
        url
    })
}

fn tcp_latency(host: &str, port: u16) -> Option<Duration> {
    let addr_str = format!("{host}:{port}");
    let addrs: Vec<_> = addr_str.to_socket_addrs().ok()?.collect();
    let addr = addrs.into_iter().next()?;
    let start = Instant::now();
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    Some(start.elapsed())
}

pub fn command_exists(command: &str) -> bool {
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
}
