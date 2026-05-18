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
