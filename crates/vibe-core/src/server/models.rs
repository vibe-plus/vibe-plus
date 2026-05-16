use super::*;

pub(super) async fn list_models_all(State(state): State<AppState>) -> Response {
    model_list_openai(&state, None).await
}

/// `/claude/v1/models` — Anthropic providers only, in Anthropic SDK format
/// Claude Code Anthropic SDK expects `{data:[...], has_more, first_id, last_id}`
pub(super) async fn list_models_claude(State(state): State<AppState>) -> Response {
    let providers = match state.db.provider_list() {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for p in providers
        .iter()
        .filter(|p| p.enabled && p.kind == vibe_protocol::ProviderKind::Anthropic)
    {
        let names: Vec<String> = if !p.remote_models.is_empty() {
            p.remote_models.clone()
        } else {
            p.model_aliases.iter().map(|a| a.alias.clone()).collect()
        };
        for name in names {
            if seen.insert(name.clone()) {
                data.push(serde_json::json!({
                    "id": name,
                    "display_name": name,
                    "type": "model",
                    "created_at": "2025-01-01T00:00:00Z"
                }));
            }
        }
    }
    data.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or(""))
    });

    let first = data
        .first()
        .and_then(|m| m["id"].as_str())
        .map(String::from);
    let last = data.last().and_then(|m| m["id"].as_str()).map(String::from);

    Json(serde_json::json!({
        "data": data,
        "has_more": false,
        "first_id": first,
        "last_id": last
    }))
    .into_response()
}

/// `/codex/v1/models` and `/opencode/v1/models`
/// OpenAI-compatible / OpenAI-Responses providers only, in OpenAI format
pub(super) async fn list_models_openai(State(state): State<AppState>) -> Response {
    use vibe_protocol::ProviderKind;
    model_list_openai(
        &state,
        Some(&[ProviderKind::OpenaiChat, ProviderKind::OpenaiResponses]),
    )
    .await
}

pub(super) async fn model_list_openai(
    state: &AppState,
    kinds: Option<&[vibe_protocol::ProviderKind]>,
) -> Response {
    let providers = match state.db.provider_list() {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for p in providers
        .iter()
        .filter(|p| p.enabled && kinds.map_or(true, |ks| ks.contains(&p.kind)))
    {
        let names: Vec<String> = if !p.remote_models.is_empty() {
            p.remote_models.clone()
        } else {
            p.model_aliases.iter().map(|a| a.alias.clone()).collect()
        };
        for name in names {
            if seen.insert(name.clone()) {
                data.push(serde_json::json!({
                    "id": name,
                    "slug": name,                    // Codex v0.129+
                    "display_name": name,            // Codex v0.129+
                    "supported_reasoning_levels": [],       // Codex v0.129+
                    "shell_type": "default",                // Codex v0.129+ (enum: default|local|unified_exec|disabled|shell_command)
                    "visibility": "list",                   // Codex v0.129+ (enum: list|hide|none)
                    "supported_in_api": true,               // Codex v0.129+
                    "priority": 0,                          // Codex v0.129+
                    "base_instructions": "",                // Codex v0.129+ (must be string, not null)
                    "supports_reasoning_summaries": false,  // Codex v0.129+
                    "support_verbosity": false,             // Codex v0.129+
                    // Align with codex-rs ModelFamily conservative defaults.
                    "truncation_policy": {"mode": "bytes", "limit": 10000},
                    "supports_parallel_tool_calls": false,
                    "experimental_supported_tools": [],
                    "object": "model",
                    "created": 0,
                    "owned_by": "vibe-plus"
                }));
            }
        }
    }
    data.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or(""))
    });

    // Codex v0.129+ expects a top-level "models" field that is an array of ModelInfo
    // objects (same structure as "data"), not a plain string array.
    Json(serde_json::json!({
        "object": "list",
        "data": data,
        "models": data           // Codex v0.129+ compatibility: same objects as data
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// Model API handlers
// ---------------------------------------------------------------------------
