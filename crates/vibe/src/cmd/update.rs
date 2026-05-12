use anyhow::Result;

const PACKAGE: &str = "@vibe-plus/cli@latest";

pub fn run() -> Result<()> {
    let manager = package_manager();
    let (command, args): (&str, &[&str]) = match manager {
        PackageManager::Bun => ("bun", &["install", "-g", PACKAGE]),
        PackageManager::Npm => ("npm", &["install", "-g", PACKAGE]),
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
    let status = std::process::Command::new(command).args(args).status()?;
    if !status.success() {
        anyhow::bail!("{command} update failed");
    }
    println!("Done. Run `vibe --version` to confirm.");
    Ok(())
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
}
