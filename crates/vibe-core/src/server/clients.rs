use super::*;

pub(super) type ClientStatusResponse = ClientStatus;

#[derive(Debug, serde::Serialize)]
pub(super) struct ClientDoctorResponse {
    client: String,
    ok: bool,
    checks: Vec<ClientDoctorCheck>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ClientDoctorCheck {
    name: String,
    ok: bool,
    detail: String,
}

pub(super) type ClientTakeoverResponse = ClientTakeoverResult;

pub(super) async fn client_status(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientStatusResponse>, AppError> {
    let status = client_status_inner(&client, state.port)?;
    Ok(Json(status))
}

pub(super) async fn client_doctor(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientDoctorResponse>, AppError> {
    let status = client_status_inner(&client, state.port)?;
    let mut checks = Vec::new();
    checks.push(ClientDoctorCheck {
        name: "config_exists".into(),
        ok: status.config_exists,
        detail: status.config_path.clone(),
    });
    checks.push(ClientDoctorCheck {
        name: "base_url_points_to_vibe".into(),
        ok: status.taken_over,
        detail: status
            .configured_base_url
            .clone()
            .unwrap_or_else(|| "(missing)".into()),
    });
    if let Some(proxy_managed) = status.auth_proxy_managed {
        checks.push(ClientDoctorCheck {
            name: "auth_proxy_managed".into(),
            ok: proxy_managed,
            detail: if proxy_managed {
                "client token is delegated to vibe".into()
            } else {
                "client still has a direct token or no proxy marker".into()
            },
        });
    }
    checks.push(ClientDoctorCheck {
        name: "model_overrides_cleared".into(),
        ok: status.model_overrides_present.is_empty(),
        detail: if status.model_overrides_present.is_empty() {
            "no known model override env vars found".into()
        } else {
            status.model_overrides_present.join(", ")
        },
    });
    let ok = checks.iter().all(|c| c.ok);
    Ok(Json(ClientDoctorResponse { client, ok, checks }))
}

pub(super) async fn client_takeover(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientTakeoverResponse>, AppError> {
    let base_url = format!("http://127.0.0.1:{}", state.port);
    let outcome = run_blocking(state.clone(), {
        let client = client.clone();
        move |_| crate::takeover::takeover(&client, &base_url)
    })
    .await?;
    let status = client_status_inner(&client, state.port)?;
    state
        .ws
        .publish(WsEvent::ClientStatusChanged(status.clone()));
    Ok(Json(ClientTakeoverResponse {
        client: outcome.client,
        config_path: outcome.config_path,
        backup_path: outcome.backup_path,
        status,
    }))
}

pub(super) async fn client_restore(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientTakeoverResponse>, AppError> {
    let outcome = run_blocking(state.clone(), {
        let client = client.clone();
        move |_| crate::takeover::restore(&client)
    })
    .await?;
    let status = client_status_inner(&client, state.port)?;
    state
        .ws
        .publish(WsEvent::ClientStatusChanged(status.clone()));
    Ok(Json(ClientTakeoverResponse {
        client: outcome.client,
        config_path: outcome.config_path,
        backup_path: outcome.backup_path,
        status,
    }))
}

pub(super) fn client_status_inner(
    client: &str,
    port: u16,
) -> Result<ClientStatusResponse, AppError> {
    let base = format!("http://127.0.0.1:{port}");
    match client {
        "claude" => {
            let path = crate::paths::claude_settings_path()?;
            let expected = format!("{base}/claude");
            let (configured, auth_proxy, overrides, notes) = read_claude_client_config(&path)?;
            Ok(ClientStatusResponse {
                client: client.into(),
                config_path: path.display().to_string(),
                config_exists: path.exists(),
                taken_over: configured.as_deref() == Some(expected.as_str()),
                expected_base_url: expected,
                configured_base_url: configured,
                auth_proxy_managed: auth_proxy,
                model_overrides_present: overrides,
                notes,
            })
        }
        "codex" => {
            let path = crate::paths::codex_config_path()?;
            let expected = format!("{base}/codex/v1");
            let configured = read_codex_base_url(&path)?;
            Ok(ClientStatusResponse {
                client: client.into(),
                config_path: path.display().to_string(),
                config_exists: path.exists(),
                taken_over: configured.as_deref() == Some(expected.as_str()),
                expected_base_url: expected,
                configured_base_url: configured,
                auth_proxy_managed: None,
                model_overrides_present: Vec::new(),
                notes: Vec::new(),
            })
        }
        "opencode" => {
            let path = crate::paths::opencode_config_path()?;
            let expected = format!("{base}/opencode/v1");
            let configured = read_opencode_base_url(&path)?;
            Ok(ClientStatusResponse {
                client: client.into(),
                config_path: path.display().to_string(),
                config_exists: path.exists(),
                taken_over: configured.as_deref() == Some(expected.as_str()),
                expected_base_url: expected,
                configured_base_url: configured,
                auth_proxy_managed: None,
                model_overrides_present: Vec::new(),
                notes: Vec::new(),
            })
        }
        other => Err(anyhow::anyhow!(
            "unknown client: {other}. Supported: claude, codex, opencode"
        )
        .into()),
    }
}

pub(super) fn read_claude_client_config(
    path: &PathBuf,
) -> anyhow::Result<(Option<String>, Option<bool>, Vec<String>, Vec<String>)> {
    if !path.exists() {
        return Ok((None, None, Vec::new(), vec!["config file missing".into()]));
    }
    let raw = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;
    let env = v.get("env").and_then(|x| x.as_object());
    let configured = env
        .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let auth_proxy = env
        .and_then(|e| {
            e.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| e.get("ANTHROPIC_API_KEY"))
        })
        .and_then(|x| x.as_str())
        .map(|s| s == "PROXY_MANAGED");
    let overrides = [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ]
    .iter()
    .filter(|k| env.map(|e| e.contains_key(**k)).unwrap_or(false))
    .map(|s| (*s).to_string())
    .collect();
    Ok((configured, auth_proxy, overrides, Vec::new()))
}

pub(super) fn read_codex_base_url(path: &PathBuf) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let v: toml::Value = toml::from_str(&raw)?;
    let active_provider = v.get("model_provider").and_then(|x| x.as_str());
    let provider_key = active_provider.unwrap_or("vibeplus");
    Ok(v.get("model_providers")
        .and_then(|x| x.get(provider_key))
        .and_then(|x| x.get("base_url"))
        .and_then(|x| x.as_str())
        .map(str::to_string))
}

pub(super) fn read_opencode_base_url(path: &PathBuf) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;
    Ok(v.pointer("/provider/vibe/options/baseURL")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.pointer("/provider/vibe/options/baseUrl")
                .and_then(|x| x.as_str())
        })
        .map(str::to_string))
}

// ---------------------------------------------------------------------------
// Provider CRUD
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct CodexHistoryPreviewQuery {
    provider: Option<String>,
}

pub(super) async fn get_codex_history_preview(
    Query(query): Query<CodexHistoryPreviewQuery>,
) -> Result<Json<vibe_protocol::CodexHistorySummary>, AppError> {
    let provider = query
        .provider
        .unwrap_or_else(|| crate::codex_history::DEFAULT_PROVIDER_ID.to_string());
    let summary = tokio::task::spawn_blocking(move || {
        crate::codex_history::unify(vibe_protocol::CodexHistoryUnifyInput {
            provider,
            from_providers: Vec::new(),
            apply: false,
            no_backup: false,
            codex_home: None,
        })
    })
    .await??;
    Ok(Json(summary))
}

pub(super) async fn post_codex_history_unify(
    Json(mut input): Json<vibe_protocol::CodexHistoryUnifyInput>,
) -> Result<Json<vibe_protocol::CodexHistorySummary>, AppError> {
    input.apply = true;
    let summary = tokio::task::spawn_blocking(move || crate::codex_history::unify(input)).await??;
    Ok(Json(summary))
}

pub(super) type CodexAppStatusResponse = CodexAppStatus;
pub(super) type CodexAppActionResponse = CodexAppActionResult;

pub(super) async fn get_codex_app_status() -> Result<Json<CodexAppStatusResponse>, AppError> {
    let status = tokio::task::spawn_blocking(codex_app_status).await??;
    Ok(Json(status))
}

pub(super) async fn post_codex_app_open(
    State(state): State<AppState>,
) -> Result<Json<CodexAppActionResponse>, AppError> {
    let response = tokio::task::spawn_blocking(|| -> anyhow::Result<CodexAppActionResponse> {
        open_codex_app()?;
        std::thread::sleep(Duration::from_millis(450));
        Ok(CodexAppActionResponse {
            action: "open".into(),
            status: codex_app_status()?,
        })
    })
    .await??;
    state
        .ws
        .publish(WsEvent::CodexAppStatusChanged(response.status.clone()));
    Ok(Json(response))
}

pub(super) async fn post_codex_app_quit(
    State(state): State<AppState>,
) -> Result<Json<CodexAppActionResponse>, AppError> {
    let response = tokio::task::spawn_blocking(|| -> anyhow::Result<CodexAppActionResponse> {
        quit_codex_app()?;
        std::thread::sleep(Duration::from_millis(450));
        Ok(CodexAppActionResponse {
            action: "quit".into(),
            status: codex_app_status()?,
        })
    })
    .await??;
    state
        .ws
        .publish(WsEvent::CodexAppStatusChanged(response.status.clone()));
    Ok(Json(response))
}

pub(super) async fn post_codex_app_restart(
    State(state): State<AppState>,
) -> Result<Json<CodexAppActionResponse>, AppError> {
    let response = tokio::task::spawn_blocking(|| -> anyhow::Result<CodexAppActionResponse> {
        quit_codex_app()?;
        std::thread::sleep(Duration::from_millis(900));
        open_codex_app()?;
        std::thread::sleep(Duration::from_millis(450));
        Ok(CodexAppActionResponse {
            action: "restart".into(),
            status: codex_app_status()?,
        })
    })
    .await??;
    state
        .ws
        .publish(WsEvent::CodexAppStatusChanged(response.status.clone()));
    Ok(Json(response))
}

pub(super) fn codex_app_path() -> PathBuf {
    PathBuf::from("/Applications/Codex.app")
}

#[cfg(target_os = "macos")]
pub(super) fn codex_app_status() -> anyhow::Result<CodexAppStatusResponse> {
    let app_path = codex_app_path();
    let output = std::process::Command::new("ps")
        .args(["ax", "-o", "pid=,args="])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    let mut main_pid = None;
    for line in stdout.lines() {
        let Some((pid_raw, command)) = line.trim().split_once(' ') else {
            continue;
        };
        if !command.contains("/Applications/Codex.app") {
            continue;
        }
        let Ok(pid) = pid_raw.parse::<u32>() else {
            continue;
        };
        let role = classify_codex_app_process(command);
        if role == "main" {
            main_pid = Some(pid);
        }
        processes.push(CodexAppProcess {
            pid,
            role: role.into(),
            command: command.into(),
        });
    }
    processes.sort_by_key(|p| (p.role != "main", p.pid));
    Ok(CodexAppStatusResponse {
        app_path: app_path.display().to_string(),
        installed: app_path.exists(),
        running: main_pid.is_some(),
        main_pid,
        process_count: processes.len(),
        processes,
    })
}

#[cfg(target_os = "macos")]
pub(super) fn classify_codex_app_process(command: &str) -> &'static str {
    if command.starts_with("/Applications/Codex.app/Contents/MacOS/Codex") {
        "main"
    } else if command.contains("chrome_crashpad_handler") {
        "crashpad"
    } else if command.contains(" app-server") {
        "app-server"
    } else if command.contains("Helper (Renderer)") || command.contains("--type=renderer") {
        "renderer"
    } else if command.contains("Helper") {
        "helper"
    } else {
        "support"
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn codex_app_status() -> anyhow::Result<CodexAppStatusResponse> {
    let app_path = codex_app_path();
    Ok(CodexAppStatusResponse {
        app_path: app_path.display().to_string(),
        installed: app_path.exists(),
        running: false,
        main_pid: None,
        process_count: 0,
        processes: Vec::new(),
    })
}

#[cfg(target_os = "macos")]
pub(super) fn open_codex_app() -> anyhow::Result<()> {
    let status = std::process::Command::new("open")
        .args(["-a", "Codex"])
        .status()?;
    anyhow::ensure!(status.success(), "failed to open Codex.app");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn open_codex_app() -> anyhow::Result<()> {
    anyhow::bail!("Codex App control is currently implemented for macOS")
}

#[cfg(target_os = "macos")]
pub(super) fn quit_codex_app() -> anyhow::Result<()> {
    let status = std::process::Command::new("osascript")
        .args(["-e", r#"tell application "Codex" to quit"#])
        .status()?;
    anyhow::ensure!(status.success(), "failed to quit Codex.app");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn quit_codex_app() -> anyhow::Result<()> {
    anyhow::bail!("Codex App control is currently implemented for macOS")
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexFilePathQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexFileWriteInput {
    path: String,
    raw_text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexDirCreateInput {
    path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexFileMoveInput {
    from: String,
    to: String,
    overwrite: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct CodexFileEntry {
    name: String,
    path: String,
    kind: String,
    size: Option<u64>,
    mtime_ms: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct CodexFileListResponse {
    root: String,
    path: String,
    abs_path: String,
    entries: Vec<CodexFileEntry>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct CodexFileResponse {
    root: String,
    path: String,
    abs_path: String,
    exists: bool,
    mtime_ms: Option<i64>,
    raw_text: String,
}

pub(super) async fn list_codex_files(
    Query(q): Query<CodexFilePathQuery>,
) -> Result<Json<CodexFileListResponse>, AppError> {
    let rel = q.path.unwrap_or_default();
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &rel)?;
    let (mtime_path, entries) = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        move || -> anyhow::Result<(String, Vec<CodexFileEntry>)> {
            if !path.exists() {
                return Ok((relative_codex_path(&root, &path), Vec::new()));
            }
            if !path.is_dir() {
                anyhow::bail!("path is not a directory");
            }
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                let p = entry.path();
                let meta = entry.metadata()?;
                entries.push(CodexFileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: relative_codex_path(&root, &p),
                    kind: if meta.is_dir() { "dir" } else { "file" }.into(),
                    size: meta.is_file().then_some(meta.len()),
                    mtime_ms: file_mtime_ms(&meta),
                });
            }
            entries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
            Ok((relative_codex_path(&root, &path), entries))
        }
    })
    .await??;
    Ok(Json(CodexFileListResponse {
        root: root.display().to_string(),
        path: mtime_path,
        abs_path: path.display().to_string(),
        entries,
    }))
}

pub(super) async fn read_codex_file(
    Query(q): Query<CodexFilePathQuery>,
) -> Result<Json<CodexFileResponse>, AppError> {
    let rel = q.path.unwrap_or_else(|| "config.toml".into());
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &rel)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        move || -> anyhow::Result<CodexFileResponse> {
            if !path.exists() {
                return Ok(CodexFileResponse {
                    root: root.display().to_string(),
                    path: relative_codex_path(&root, &path),
                    abs_path: path.display().to_string(),
                    exists: false,
                    mtime_ms: None,
                    raw_text: String::new(),
                });
            }
            if !path.is_file() {
                anyhow::bail!("path is not a file");
            }
            let meta = std::fs::metadata(&path)?;
            let raw_text = std::fs::read_to_string(&path)?;
            Ok(CodexFileResponse {
                root: root.display().to_string(),
                path: relative_codex_path(&root, &path),
                abs_path: path.display().to_string(),
                exists: true,
                mtime_ms: file_mtime_ms(&meta),
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

pub(super) async fn write_codex_file(
    Json(input): Json<CodexFileWriteInput>,
) -> Result<Json<CodexFileResponse>, AppError> {
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &input.path)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        let raw = input.raw_text;
        move || -> anyhow::Result<CodexFileResponse> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            let meta = std::fs::metadata(&path)?;
            let raw_text = std::fs::read_to_string(&path)?;
            Ok(CodexFileResponse {
                root: root.display().to_string(),
                path: relative_codex_path(&root, &path),
                abs_path: path.display().to_string(),
                exists: true,
                mtime_ms: file_mtime_ms(&meta),
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

pub(super) async fn delete_codex_file(
    Query(q): Query<CodexFilePathQuery>,
) -> Result<StatusCode, AppError> {
    let Some(rel) = q.path else {
        return Err(anyhow::anyhow!("missing path").into());
    };
    if rel.trim().is_empty() || rel.trim() == "." {
        return Err(anyhow::anyhow!("refusing to delete codex root").into());
    }
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &rel)?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn create_codex_dir(
    Json(input): Json<CodexDirCreateInput>,
) -> Result<Json<CodexFileListResponse>, AppError> {
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &input.path)?;
    tokio::task::spawn_blocking({
        let path = path.clone();
        move || std::fs::create_dir_all(path)
    })
    .await??;
    list_codex_files(Query(CodexFilePathQuery {
        path: Some(input.path),
    }))
    .await
}

pub(super) async fn move_codex_file(
    Json(input): Json<CodexFileMoveInput>,
) -> Result<Json<CodexFileResponse>, AppError> {
    if input.from.trim().is_empty() || input.from.trim() == "." {
        return Err(anyhow::anyhow!("refusing to move codex root").into());
    }
    if input.to.trim().is_empty() || input.to.trim() == "." {
        return Err(anyhow::anyhow!("destination path is required").into());
    }
    let root = codex_home_dir();
    let from = resolve_codex_path(&root, &input.from)?;
    let to = resolve_codex_path(&root, &input.to)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let overwrite = input.overwrite.unwrap_or(false);
        move || -> anyhow::Result<CodexFileResponse> {
            if !from.exists() {
                anyhow::bail!("source path does not exist");
            }
            if to.exists() {
                if !overwrite {
                    anyhow::bail!("destination already exists");
                }
                if to.is_dir() {
                    std::fs::remove_dir_all(&to)?;
                } else {
                    std::fs::remove_file(&to)?;
                }
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from, &to)?;
            if to.is_file() {
                let meta = std::fs::metadata(&to)?;
                let raw_text = std::fs::read_to_string(&to)?;
                return Ok(CodexFileResponse {
                    root: root.display().to_string(),
                    path: relative_codex_path(&root, &to),
                    abs_path: to.display().to_string(),
                    exists: true,
                    mtime_ms: file_mtime_ms(&meta),
                    raw_text,
                });
            }
            Ok(CodexFileResponse {
                root: root.display().to_string(),
                path: relative_codex_path(&root, &to),
                abs_path: to.display().to_string(),
                exists: true,
                mtime_ms: std::fs::metadata(&to).ok().as_ref().and_then(file_mtime_ms),
                raw_text: String::new(),
            })
        }
    })
    .await??;
    Ok(Json(out))
}

pub(super) fn codex_home_dir() -> PathBuf {
    crate::codex_config::codex_config_path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(super) fn resolve_codex_path(root: &std::path::Path, rel: &str) -> anyhow::Result<PathBuf> {
    if rel.contains('\0') {
        anyhow::bail!("invalid path");
    }
    let rel_path = std::path::Path::new(rel);
    if rel_path.is_absolute() {
        anyhow::bail!("absolute paths are not allowed");
    }
    let mut out = root.to_path_buf();
    for component in rel_path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            _ => anyhow::bail!("path traversal is not allowed"),
        }
    }
    ensure_codex_path_within_root(root, &out)?;
    Ok(out)
}

pub(super) fn ensure_codex_path_within_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let canonical = if path.exists() {
        path.canonicalize()?
    } else {
        let mut ancestor = path.parent();
        let mut found = None;
        while let Some(candidate) = ancestor {
            if candidate.exists() {
                found = Some(candidate.canonicalize()?);
                break;
            }
            ancestor = candidate.parent();
        }
        found.unwrap_or_else(|| root.clone())
    };
    if !canonical.starts_with(&root) {
        anyhow::bail!("path resolves outside codex home");
    }
    Ok(())
}

pub(super) fn relative_codex_path(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into())
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------
