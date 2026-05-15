use anyhow::Context as _;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::npm_registry;

const CODEX_DMG_URL_ARM64: &str = "https://persistent.oaistatic.com/codex-app-prod/Codex.dmg";
const CODEX_DMG_URL_X64: &str =
    "https://persistent.oaistatic.com/codex-app-prod/Codex-latest-x64.dmg";

#[cfg(target_os = "windows")]
const CODEX_WINDOWS_INSTALLER_URL: &str =
    "https://get.microsoft.com/installer/download/9PLM9XGG6VKS?cid=website_cta_psi";
#[cfg(target_os = "windows")]
const CODEX_MICROSOFT_STORE_WEB_URL: &str = "https://apps.microsoft.com/detail/9plm9xgg6vks";

const CODEX_DMG_DOWNLOAD_CANDIDATES: &[(&str, &str, u16, &str)] = &[
    (
        CODEX_DMG_URL_ARM64,
        "persistent.oaistatic.com",
        443,
        "OpenAI CDN (arm64)",
    ),
    (
        CODEX_DMG_URL_X64,
        "persistent.oaistatic.com",
        443,
        "OpenAI CDN (x64)",
    ),
];

pub async fn install_or_update() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return install_macos().await;
    }
    #[cfg(target_os = "windows")]
    {
        return install_windows().await;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("Codex Desktop 安装目前支持 macOS 与 Windows");
    }
}

#[cfg(target_os = "macos")]
async fn install_macos() -> anyhow::Result<()> {
    if let Some(path) = find_existing_codex_app() {
        println!(
            "已检测到 Codex.app：{}",
            path.display()
        );
        if try_brew_cask("codex", true).await? {
            println!("已通过 Homebrew 更新 Codex Desktop。");
            return Ok(());
        }
        println!("正在从官方 CDN 下载最新安装包…");
    } else if try_brew_cask("codex", false).await? {
        println!("已通过 Homebrew 安装 Codex Desktop。");
        return Ok(());
    }

    let dmg_url = pick_mac_dmg_url();
    let installed = download_and_install_dmg(&dmg_url).await?;
    println!(
        "Codex Desktop 已安装：{}",
        installed.display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn pick_mac_dmg_url() -> String {
    let arch_url = if is_apple_silicon_mac() {
        CODEX_DMG_URL_ARM64
    } else {
        CODEX_DMG_URL_X64
    };

    let arch_label = if is_apple_silicon_mac() {
        "arm64"
    } else {
        "x64"
    };

    let candidates: Vec<(&str, &str, u16, &str)> = CODEX_DMG_DOWNLOAD_CANDIDATES
        .iter()
        .filter(|(url, _, _, _)| *url == arch_url)
        .copied()
        .collect();

    let url = npm_registry::fastest_endpoint(&candidates)
        .unwrap_or(arch_url)
        .to_string();
    println!("目标架构：{arch_label}，下载地址：{url}");
    url
}

#[cfg(target_os = "macos")]
fn is_apple_silicon_mac() -> bool {
    std::env::consts::ARCH == "aarch64"
}

#[cfg(target_os = "macos")]
fn find_existing_codex_app() -> Option<PathBuf> {
    candidate_codex_app_paths()
        .into_iter()
        .find(|p| p.is_dir())
}

#[cfg(target_os = "macos")]
fn candidate_codex_app_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("/Applications/Codex.app")];
    if let Some(home) = home_dir() {
        paths.push(home.join("Applications").join("Codex.app"));
    }
    paths
}

#[cfg(target_os = "macos")]
async fn try_brew_cask(name: &str, upgrade: bool) -> anyhow::Result<bool> {
    if !npm_registry::command_exists("brew") {
        return Ok(false);
    }

    let subcmd = if upgrade { "upgrade" } else { "install" };
    println!("尝试通过 Homebrew {subcmd} --cask {name}…");
    let status = Command::new("brew")
        .args([subcmd, "--cask", name])
        .status()
        .await
        .context("调用 brew 失败")?;
    Ok(status.success())
}

#[cfg(target_os = "macos")]
async fn download_and_install_dmg(dmg_url: &str) -> anyhow::Result<PathBuf> {
    let tmp_root = std::env::temp_dir().join(format!(
        "vibe-codex-install-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&tmp_root).with_context(|| {
        format!("无法创建临时目录 {}", tmp_root.display())
    })?;

    let dmg_path = tmp_root.join("Codex.dmg");
    download_dmg(dmg_url, &dmg_path).await?;

    println!("正在挂载安装包…");
    let mount_point = mount_dmg(&dmg_path).await?;
    let result = async {
        let app_in_volume = find_codex_app_in_mount(&mount_point)
            .context("在 DMG 中未找到 Codex.app")?;
        install_codex_app_bundle(&app_in_volume).await
    }
    .await;

    if let Err(err) = detach_dmg(&mount_point).await {
        eprintln!(
            "警告：卸载 DMG 失败（{}）：{err}",
            mount_point.display()
        );
    }

    let _ = std::fs::remove_dir_all(&tmp_root);
    result
}

#[cfg(target_os = "macos")]
async fn download_dmg(url: &str, dest: &Path) -> anyhow::Result<()> {
    println!("正在下载 Codex Desktop 安装包…");
    let status = Command::new("curl")
        .args(["-fL", "--retry", "3", "--retry-delay", "1", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .await
        .context("调用 curl 失败")?;
    anyhow::ensure!(status.success(), "curl 下载失败（{status}）");
    Ok(())
}

#[cfg(target_os = "macos")]
async fn mount_dmg(dmg_path: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly"])
        .arg(dmg_path)
        .output()
        .await
        .context("调用 hdiutil attach 失败")?;

    if !output.status.success() {
        anyhow::bail!(
            "hdiutil attach 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_hdiutil_mount_point(&stdout)
        .map(PathBuf::from)
        .with_context(|| format!("无法解析挂载点：\n{stdout}"))
}

#[cfg(target_os = "macos")]
async fn detach_dmg(mount_point: &Path) -> anyhow::Result<()> {
    let status = Command::new("hdiutil")
        .args(["detach"])
        .arg(mount_point)
        .status()
        .await
        .context("调用 hdiutil detach 失败")?;
    anyhow::ensure!(status.success(), "hdiutil detach 失败（{status}）");
    Ok(())
}

#[cfg(target_os = "macos")]
fn find_codex_app_in_mount(mount_point: &Path) -> anyhow::Result<PathBuf> {
    let direct = mount_point.join("Codex.app");
    if direct.is_dir() {
        return Ok(direct);
    }
    for entry in std::fs::read_dir(mount_point)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "app") && path.is_dir() {
            return Ok(path);
        }
    }
    anyhow::bail!("在 {} 中未找到 .app", mount_point.display())
}

#[cfg(target_os = "macos")]
async fn install_codex_app_bundle(src_app: &Path) -> anyhow::Result<PathBuf> {
    for applications_dir in candidate_applications_dirs()? {
        println!(
            "正在安装到 {}…",
            applications_dir.display()
        );
        std::fs::create_dir_all(&applications_dir)?;
        let dest_app = applications_dir.join("Codex.app");
        if dest_app.is_dir() {
            std::fs::remove_dir_all(&dest_app).with_context(|| {
                format!("无法移除旧版本 {}", dest_app.display())
            })?;
        }
        let status = Command::new("ditto")
            .arg(src_app)
            .arg(&dest_app)
            .status()
            .await
            .context("调用 ditto 失败")?;
        if status.success() {
            return Ok(dest_app);
        }
    }
    anyhow::bail!("无法将 Codex.app 安装到任何 Applications 目录")
}

#[cfg(target_os = "macos")]
fn candidate_applications_dirs() -> anyhow::Result<Vec<PathBuf>> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = home_dir() {
        dirs.push(home.join("Applications"));
    }
    Ok(dirs)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn parse_hdiutil_mount_point(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        if !line.contains("/Volumes/") {
            return None;
        }
        if let Some((_, mount)) = line.rsplit_once('\t') {
            return Some(mount.trim().to_string());
        }
        line.split_whitespace()
            .find(|field| field.starts_with("/Volumes/"))
            .map(str::to_string)
    })
}

#[cfg(target_os = "windows")]
async fn install_windows() -> anyhow::Result<()> {
    if codex_app_installed_windows().await? {
        println!("已检测到 Codex Desktop，请在 Microsoft Store 或应用内检查更新。");
        return Ok(());
    }

    println!("未检测到 Codex Desktop，正在打开官方安装程序…");
    open_url(CODEX_WINDOWS_INSTALLER_URL)
        .await
        .or_else(|_| open_url(CODEX_MICROSOFT_STORE_WEB_URL))
        .await?;
    println!("请在安装向导完成后运行 `vibe i app` 验证。");
    Ok(())
}

#[cfg(target_os = "windows")]
async fn codex_app_installed_windows() -> anyhow::Result<bool> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Get-StartApps -Name 'Codex' | Select-Object -First 1 -ExpandProperty AppID",
        ])
        .output()
        .await
        .context("调用 powershell 失败")?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

#[cfg(target_os = "windows")]
async fn open_url(url: &str) -> anyhow::Result<()> {
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "& { param($target) Start-Process -FilePath $target }",
            url,
        ])
        .status()
        .await
        .with_context(|| format!("无法打开 {url}"))?;
    anyhow::ensure!(status.success(), "打开 {url} 失败（{status}）");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::parse_hdiutil_mount_point;

    #[test]
    #[cfg(target_os = "macos")]
    fn parses_hdiutil_mount_point() {
        let output = "/dev/disk2s1\tApple_HFS\tCodex\t/Volumes/Codex\n";
        assert_eq!(
            parse_hdiutil_mount_point(output).as_deref(),
            Some("/Volumes/Codex")
        );
    }
}
