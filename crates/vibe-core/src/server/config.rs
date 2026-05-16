use super::*;

#[derive(Debug, Serialize)]
pub(super) struct ToolConfigRawResponse {
    tool: String,
    path: String,
    exists: bool,
    mtime_ms: Option<i64>,
    raw_text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ToolConfigRawUpdateInput {
    raw_text: String,
}

pub(super) fn resolve_tool_config_path(tool: &str) -> anyhow::Result<PathBuf> {
    match tool {
        "codex" => Ok(crate::codex_config::codex_config_path()),
        "claude" => crate::paths::claude_settings_path(),
        _ => anyhow::bail!("unsupported tool: {tool}"),
    }
}

pub(super) fn file_mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

pub(super) async fn get_tool_config_raw(Path(tool): Path<String>) -> Response {
    let path = match resolve_tool_config_path(&tool) {
        Ok(p) => p,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    let read_result = tokio::task::spawn_blocking({
        let path = path.clone();
        move || -> anyhow::Result<(bool, Option<i64>, String)> {
            if !path.exists() {
                return Ok((false, None, String::new()));
            }
            let meta = std::fs::metadata(&path)?;
            let raw = std::fs::read_to_string(&path)?;
            Ok((true, file_mtime_ms(&meta), raw))
        }
    })
    .await;

    let (exists, mtime_ms, raw_text) = match read_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(ToolConfigRawResponse {
        tool,
        path: path.to_string_lossy().to_string(),
        exists,
        mtime_ms,
        raw_text,
    })
    .into_response()
}

pub(super) async fn put_tool_config_raw(
    Path(tool): Path<String>,
    Json(input): Json<ToolConfigRawUpdateInput>,
) -> Response {
    let path = match resolve_tool_config_path(&tool) {
        Ok(p) => p,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    if tool == "codex" && toml::from_str::<toml::Value>(&input.raw_text).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid TOML in codex config").into_response();
    }
    if tool == "claude" && serde_json::from_str::<serde_json::Value>(&input.raw_text).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid JSON in claude settings").into_response();
    }

    let write_result = tokio::task::spawn_blocking({
        let path = path.clone();
        let raw = input.raw_text.clone();
        move || -> anyhow::Result<(bool, Option<i64>, String)> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            let meta = std::fs::metadata(&path)?;
            let saved = std::fs::read_to_string(&path)?;
            Ok((true, file_mtime_ms(&meta), saved))
        }
    })
    .await;

    let (exists, mtime_ms, raw_text) = match write_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(ToolConfigRawResponse {
        tool,
        path: path.to_string_lossy().to_string(),
        exists,
        mtime_ms,
        raw_text,
    })
    .into_response()
}

pub(super) async fn get_codex_config_settings() -> Result<Json<CodexConfigSettings>, AppError> {
    let path = crate::codex_config::codex_config_path();
    let settings =
        tokio::task::spawn_blocking(move || crate::codex_config::read_settings(&path)).await??;
    Ok(Json(settings))
}

pub(super) async fn put_codex_config_settings(
    Json(input): Json<CodexConfigSettingsInput>,
) -> Result<Json<CodexConfigSettings>, AppError> {
    let path = crate::codex_config::codex_config_path();
    let settings =
        tokio::task::spawn_blocking(move || crate::codex_config::write_settings(&path, input))
            .await??;
    Ok(Json(settings))
}
