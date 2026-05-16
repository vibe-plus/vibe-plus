use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct ProviderSyncInput {
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProviderSyncPreview {
    provider: Provider,
    display_name: String,
    avatar_url: Option<String>,
    balance: Option<ProviderBalanceSnapshot>,
    usage: Option<ProviderBalanceSnapshot>,
    supported_protocols: Vec<String>,
    platform_guess: Option<String>,
    note: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SyncBranding {
    display_name: Option<String>,
    avatar_url: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SyncFinancials {
    balance: Option<ProviderBalanceSnapshot>,
    usage: Option<ProviderBalanceSnapshot>,
}

pub(super) async fn refresh_provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Provider>, AppError> {
    let provider = run_blocking(state.clone(), move |s| s.db.provider_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    let refreshed = refresh_remote_models_for_provider(&state, &provider).await?;
    publish_providers_overview_soon(state);
    Ok(Json(refreshed))
}

pub(crate) async fn refresh_remote_models_for_provider_if_supported(
    state: &AppState,
    provider: &Provider,
) -> anyhow::Result<Provider> {
    match provider.kind {
        vibe_protocol::ProviderKind::GeminiNative => Ok(provider.clone()),
        _ => refresh_remote_models_for_provider(state, provider).await,
    }
}

pub(super) fn pick_active_secret_for_provider(
    state: &AppState,
    provider_id: &str,
) -> anyhow::Result<Option<String>> {
    let creds = state.db.credential_list_for_provider(provider_id)?;
    let cred = creds
        .into_iter()
        .filter(|c| c.enabled)
        .min_by_key(|c| c.priority)
        .or_else(|| {
            state
                .db
                .credential_list_for_provider(provider_id)
                .ok()
                .and_then(|list| list.into_iter().next())
        });
    let Some(cred) = cred else {
        return Ok(None);
    };
    if let Some(ref auth_ref) = cred.auth_ref {
        return Ok(Some(crate::secrets::resolve(auth_ref)?));
    }
    if let Some(ref access) = cred.oauth_access_token {
        return Ok(Some(access.clone()));
    }
    Ok(None)
}

pub(super) async fn fetch_model_ids_for_protocol(
    http: &reqwest::Client,
    proto: &vibe_protocol::ProviderProtocol,
    secret: &str,
) -> anyhow::Result<Vec<String>> {
    let names =
        crate::intake::fetch_upstream_model_ids(http, proto.kind, &proto.base_url, secret).await;
    if names.is_empty() && proto.kind != vibe_protocol::ProviderKind::GeminiNative {
        anyhow::bail!("model list refresh returned no models");
    }
    Ok(names)
}

pub(super) async fn refresh_remote_models_for_provider(
    state: &AppState,
    provider: &Provider,
) -> anyhow::Result<Provider> {
    let secret = if let Some(auth_ref) = provider.auth_ref.as_deref() {
        crate::secrets::resolve(auth_ref).ok()
    } else {
        None
    };
    let secret = match secret {
        Some(s) if !s.is_empty() => Some(s),
        _ => pick_active_secret_for_provider(state, &provider.id)?,
    };
    let Some(secret) = secret else {
        anyhow::bail!("no credential secret available for model list refresh");
    };

    let mut names = Vec::<String>::new();
    for proto in provider.effective_protocols() {
        if let Ok(mut batch) = fetch_model_ids_for_protocol(&state.http, &proto, &secret).await {
            for id in batch.drain(..) {
                if !names.contains(&id) {
                    names.push(id);
                }
            }
        }
    }
    names.sort();
    names.dedup();
    let id = provider.id.clone();
    let fetched_at = chrono::Utc::now().timestamp();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.provider_update_remote_models(&id, names, fetched_at)
    })
    .await?;
    Ok(updated)
}

pub(super) async fn refresh_credential_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<vibe_protocol::Credential>, AppError> {
    let credential = run_blocking(state.clone(), move |s| s.db.credential_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let provider = run_blocking(state.clone(), {
        let pid = credential.provider_id.clone();
        move |s| s.db.provider_get(&pid)
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("provider not found"))?;

    let secret = if let Some(auth_ref) = credential.auth_ref.as_deref() {
        crate::secrets::resolve(auth_ref)?
    } else if let Some(access) = credential.oauth_access_token.as_deref() {
        access.to_string()
    } else {
        return Err(anyhow::anyhow!("credential has no resolvable secret").into());
    };

    let mut names = Vec::<String>::new();
    for proto in provider.effective_protocols() {
        if let Ok(mut batch) = fetch_model_ids_for_protocol(&state.http, &proto, &secret).await {
            for mid in batch.drain(..) {
                if !names.contains(&mid) {
                    names.push(mid);
                }
            }
        }
    }
    names.sort();
    names.dedup();
    let fetched_at = chrono::Utc::now().timestamp();
    let cred_id = credential.id.clone();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.credential_update_remote_models(&cred_id, names, fetched_at)
    })
    .await?;

    let _ = refresh_remote_models_for_provider(&state, &provider).await;
    publish_providers_overview_soon(state);
    Ok(Json(updated))
}

pub(super) async fn refresh_credential_balance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<vibe_protocol::Credential>, AppError> {
    let credential = run_blocking(state.clone(), move |s| s.db.credential_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let provider = run_blocking(state.clone(), {
        let pid = credential.provider_id.clone();
        move |s| s.db.provider_get(&pid)
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("provider not found"))?;

    let base_url = provider
        .effective_protocols()
        .into_iter()
        .next()
        .map(|p| p.base_url.clone())
        .unwrap_or_else(|| provider.base_url.clone());

    use vibe_protocol::CredentialVendor;
    match credential.upstream_vendor.as_ref() {
        Some(CredentialVendor::NewApi) => {
            let cred_id_s = credential.id.clone();
            let session = run_blocking(state.clone(), move |s| {
                s.db.credential_get_session(&cred_id_s)
            })
            .await?;
            let token = session
                .as_deref()
                .or(credential.auth_ref.as_deref())
                .map(|s| resolve_secret(s))
                .unwrap_or_default();
            let balance =
                crate::providers::newapi::fetch_balance(&state.http, &base_url, &token).await;
            let windows =
                crate::providers::newapi::fetch_key_usage(&state.http, &base_url, &token).await;
            let fetched_at = chrono::Utc::now().timestamp();
            let cred_id = credential.id.clone();
            let updated = run_blocking(state.clone(), move |s| {
                s.db.credential_update_financials(&cred_id, balance, None, fetched_at)?;
                s.db.credential_update_windows(&cred_id, &windows)
            })
            .await?;
            publish_providers_overview_soon(state);
            return Ok(Json(updated));
        }
        Some(CredentialVendor::Sub2Api) => {
            let cred_id_s2 = credential.id.clone();
            let session2 = run_blocking(state.clone(), move |s| {
                s.db.credential_get_session(&cred_id_s2)
            })
            .await?;
            let token = session2.as_deref().unwrap_or_default();
            if token.is_empty() {
                return Err(
                    anyhow::anyhow!("sub2api credential has no session — login first").into(),
                );
            }
            let balance =
                crate::providers::sub2api::fetch_balance(&state.http, &base_url, token).await;
            let windows =
                crate::providers::sub2api::fetch_windows(&state.http, &base_url, token).await;
            let fetched_at = chrono::Utc::now().timestamp();
            let cred_id = credential.id.clone();
            let updated = run_blocking(state.clone(), move |s| {
                s.db.credential_update_financials(&cred_id, balance, None, fetched_at)?;
                s.db.credential_update_windows(&cred_id, &windows)
            })
            .await?;
            publish_providers_overview_soon(state);
            return Ok(Json(updated));
        }
        _ => {}
    }

    // Generic path (original logic)
    let secret = if let Some(auth_ref) = credential.auth_ref.as_deref() {
        crate::secrets::resolve(auth_ref)?
    } else if let Some(access) = credential.oauth_access_token.as_deref() {
        access.to_string()
    } else {
        return Err(anyhow::anyhow!("credential has no resolvable secret").into());
    };

    let primary = provider.effective_protocols().into_iter().next();
    let Some(proto) = primary else {
        return Err(anyhow::anyhow!("provider has no protocols").into());
    };
    let financials =
        crate::intake::fetch_financials_for_base(&state.http, proto.kind, &proto.base_url, &secret)
            .await;
    let fetched_at = chrono::Utc::now().timestamp();
    let cred_id = credential.id.clone();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.credential_update_financials(
            &cred_id,
            financials.balance.clone(),
            financials.usage.clone(),
            fetched_at,
        )
    })
    .await?;
    publish_providers_overview_soon(state);
    Ok(Json(updated))
}

// ---------------------------------------------------------------------------
// Vendor auto-detection
// ---------------------------------------------------------------------------

/// `POST /_vp/providers/:id/detect-vendor`
/// Probe the provider's base URL and detect whether it runs NewAPI or Sub2API.
/// Automatically writes the result to all unset credentials of this provider.
pub(super) async fn detect_provider_vendor(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let provider = run_blocking(state.clone(), move |s| s.db.provider_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    let base_url = provider
        .effective_protocols()
        .into_iter()
        .next()
        .map(|p| p.base_url.clone())
        .unwrap_or_else(|| provider.base_url.clone());

    let timeout = std::time::Duration::from_secs(6);
    let upstream_vendor =
        crate::intake::probe_upstream_vendor(&state.http, &base_url, timeout).await;

    let updated_count = if let Some(ref vendor) = upstream_vendor {
        let pid = provider.id.clone();
        let vendor_str = vendor.clone();
        run_blocking(state.clone(), move |s| {
            s.db.credentials_set_vendor_for_provider(&pid, &vendor_str)
        })
        .await?
    } else {
        0
    };

    publish_providers_overview_soon(state);
    Ok(Json(serde_json::json!({
        "upstream_vendor": upstream_vendor,
        "updated_credentials": updated_count,
        "base_url": base_url,
    })))
}

// ---------------------------------------------------------------------------
// Upstream login / groups
// ---------------------------------------------------------------------------

/// `POST /_vp/credentials/:id/login`
/// Trigger upstream password-based login for NewAPI or Sub2API credentials,
/// storing the resulting session token in the database.
pub(super) async fn credential_upstream_login(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<vibe_protocol::CredentialLoginRequest>,
) -> Result<Json<vibe_protocol::CredentialLoginResponse>, AppError> {
    let credential = run_blocking(state.clone(), move |s| s.db.credential_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let provider = run_blocking(state.clone(), {
        let pid = credential.provider_id.clone();
        move |s| s.db.provider_get(&pid)
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    let base_url = provider
        .effective_protocols()
        .into_iter()
        .next()
        .map(|p| p.base_url.clone())
        .unwrap_or_else(|| provider.base_url.clone());

    use vibe_protocol::CredentialVendor;
    match credential.upstream_vendor.as_ref() {
        Some(CredentialVendor::NewApi) => {
            let token = crate::providers::newapi::login(
                &state.http,
                &base_url,
                &body.username,
                &body.password,
            )
            .await
            .map_err(|e| AppError(e))?;
            let cred_id = credential.id.clone();
            run_blocking(state.clone(), move |s| {
                s.db.credential_update_session(&cred_id, &token, None)
            })
            .await?;
            publish_providers_overview_soon(state);
            Ok(Json(vibe_protocol::CredentialLoginResponse {
                ok: true,
                note: None,
            }))
        }
        Some(CredentialVendor::Sub2Api) => {
            let (token, expires_at) = crate::providers::sub2api::login(
                &state.http,
                &base_url,
                &body.username,
                &body.password,
            )
            .await
            .map_err(|e| AppError(e))?;
            let cred_id = credential.id.clone();
            run_blocking(state.clone(), move |s| {
                s.db.credential_update_session(&cred_id, &token, expires_at)
            })
            .await?;
            publish_providers_overview_soon(state);
            Ok(Json(vibe_protocol::CredentialLoginResponse {
                ok: true,
                note: None,
            }))
        }
        _ => Err(anyhow::anyhow!("login not supported for this credential vendor").into()),
    }
}

/// `GET /_vp/credentials/:id/groups`
/// Fetch available upstream groups for NewAPI or Sub2API credentials.
pub(super) async fn credential_upstream_groups(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<vibe_protocol::UpstreamGroupInfo>>, AppError> {
    let credential = run_blocking(state.clone(), move |s| s.db.credential_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let provider = run_blocking(state.clone(), {
        let pid = credential.provider_id.clone();
        move |s| s.db.provider_get(&pid)
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    let base_url = provider
        .effective_protocols()
        .into_iter()
        .next()
        .map(|p| p.base_url.clone())
        .unwrap_or_else(|| provider.base_url.clone());
    let cred_id_g = credential.id.clone();
    let session_g = run_blocking(state.clone(), move |s| {
        s.db.credential_get_session(&cred_id_g)
    })
    .await?;
    let token = session_g
        .as_deref()
        .or(credential.auth_ref.as_deref())
        .map(resolve_secret)
        .unwrap_or_default();

    use vibe_protocol::CredentialVendor;
    let groups = match credential.upstream_vendor.as_ref() {
        Some(CredentialVendor::NewApi) => {
            crate::providers::newapi::fetch_groups(&state.http, &base_url, &token).await
        }
        Some(CredentialVendor::Sub2Api) => {
            crate::providers::sub2api::fetch_groups(&state.http, &base_url, &token).await
        }
        _ => vec![],
    };
    Ok(Json(groups))
}

// ---------------------------------------------------------------------------
// Local import (scan installed tools → ready-to-use ProviderInput)
// ---------------------------------------------------------------------------

/// `GET /_vp/providers/import-local`
/// Scan locally installed Claude Code / Codex CLI and return importable candidates.
/// No database writes; filesystem reads only.

pub(super) async fn list_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<Provider>>, AppError> {
    let v = run_blocking(state, |s| s.db.provider_list()).await?;
    Ok(Json(v))
}

pub(super) async fn create_provider(
    State(state): State<AppState>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let name = input.name.clone();
    let p = run_blocking(state.clone(), move |s| s.db.provider_insert(input)).await?;
    emit_app_log(
        &state,
        AppLogLevel::Info,
        "provider",
        format!("Provider added: {name}"),
    );
    publish_providers_overview_soon(state);
    Ok(Json(p))
}

pub(super) async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let name = input.name.clone();
    let enabled = input.enabled;
    let id_for_update = id.clone();
    let p = run_blocking(state.clone(), move |s| {
        s.db.provider_update(&id_for_update, input)
    })
    .await?;
    // Bind toggles to circuit state: after switching, clear circuit state for the provider and its credentials,
    // avoiding UI enabled state while requests remain blocked by historical circuit breaks.
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;
    state.cb.reset(&id);
    for cid in cred_ids {
        state.cb.reset(&cid);
    }
    let msg = if enabled {
        format!("Provider enabled: {name}")
    } else {
        format!("Provider disabled: {name}")
    };
    emit_app_log(&state, AppLogLevel::Info, "provider", msg);
    publish_providers_overview_soon(state);
    Ok(Json(p))
}

pub(super) async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let id_clone = id.clone();
    run_blocking(state.clone(), move |s| s.db.provider_delete(&id_clone)).await?;
    emit_app_log(
        &state,
        AppLogLevel::Warn,
        "provider",
        format!("Provider deleted: {id}"),
    );
    publish_providers_overview_soon(state);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(super) struct ProviderTestInput {
    model: Option<String>,
    stream: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ProviderTestResponse {
    ok: bool,
    status: u16,
    latency_ms: i64,
    log_id: Option<String>,
    body_preview: String,
}

pub(super) async fn provider_test(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderTestInput>,
) -> Result<Json<ProviderTestResponse>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| {
            s.db.provider_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;
    let model = input.model.unwrap_or_else(|| {
        provider
            .model_aliases
            .first()
            .map(|a| a.alias.clone())
            .unwrap_or_else(|| match provider.kind {
                vibe_protocol::ProviderKind::Anthropic => "claude-sonnet-4-5".into(),
                vibe_protocol::ProviderKind::GeminiNative => "gemini-2.5-pro".into(),
                _ => "gpt-5.3-codex".into(),
            })
    });
    let stream = input.stream.unwrap_or(false);
    let started = Instant::now();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        HeaderName::from_static("x-vibe-provider-test"),
        HeaderValue::from_static("1"),
    );
    let (wire, route_prefix, body) = match provider.kind {
        vibe_protocol::ProviderKind::Anthropic => (
            Wire::Anthropic,
            Some("provider-test".into()),
            serde_json::json!({
                "model": model,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": stream
            }),
        ),
        vibe_protocol::ProviderKind::GeminiNative => (
            Wire::GeminiNative,
            Some("provider-test".into()),
            serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "ping"}]}]
            }),
        ),
        vibe_protocol::ProviderKind::OpenaiChat => (
            Wire::OpenaiChat,
            Some("provider-test".into()),
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": stream
            }),
        ),
        vibe_protocol::ProviderKind::OpenaiResponses => (
            Wire::OpenaiResponses,
            Some("provider-test".into()),
            serde_json::json!({
                "model": model,
                "input": "ping",
                "stream": stream
            }),
        ),
    };
    let path = if provider.kind == vibe_protocol::ProviderKind::GeminiNative {
        Some(format!("/v1beta/models/{model}:generateContent"))
    } else {
        None
    };
    let response = forward::forward(
        state,
        wire,
        path,
        headers,
        Bytes::from(serde_json::to_vec(&body)?),
        route_prefix,
    )
    .await;
    let log_id = response
        .extensions()
        .get::<forward::VibeLogId>()
        .map(|x| x.0.clone());
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let preview = String::from_utf8_lossy(&bytes).chars().take(600).collect();
    Ok(Json(ProviderTestResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        latency_ms: started.elapsed().as_millis() as i64,
        log_id,
        body_preview: preview,
    }))
}

pub(super) const SPEEDTEST_DEFAULT_TIMEOUT_SECS: u64 = 8;
pub(super) const SPEEDTEST_MIN_TIMEOUT_SECS: u64 = 2;
pub(super) const SPEEDTEST_MAX_TIMEOUT_SECS: u64 = 30;

pub(super) fn sanitize_speedtest_timeout(timeout_secs: Option<u64>) -> u64 {
    timeout_secs
        .unwrap_or(SPEEDTEST_DEFAULT_TIMEOUT_SECS)
        .clamp(SPEEDTEST_MIN_TIMEOUT_SECS, SPEEDTEST_MAX_TIMEOUT_SECS)
}

pub(super) fn speedtest_error_result(
    url: String,
    checked_at: i64,
    error: impl Into<String>,
) -> ProviderSpeedtestResult {
    ProviderSpeedtestResult {
        url,
        ok: false,
        latency_ms: None,
        status: None,
        error: Some(error.into()),
        checked_at,
    }
}

pub(super) async fn run_endpoint_speedtest(
    raw_url: &str,
    timeout_secs: Option<u64>,
) -> ProviderSpeedtestResult {
    let checked_at = chrono::Utc::now().timestamp();
    let trimmed = raw_url.trim().to_string();
    if trimmed.is_empty() {
        return speedtest_error_result(trimmed, checked_at, "URL cannot be empty");
    }

    let parsed_url = match url::Url::parse(&trimmed) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => parsed,
        Ok(parsed) => {
            return speedtest_error_result(
                trimmed,
                checked_at,
                format!("Unsupported URL scheme: {}", parsed.scheme()),
            );
        }
        Err(err) => {
            return speedtest_error_result(trimmed, checked_at, format!("Invalid URL: {err}"))
        }
    };

    let timeout = Duration::from_secs(sanitize_speedtest_timeout(timeout_secs));
    let client = match reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(30))
        .timeout(timeout)
        .build()
    {
        Ok(client) => client,
        Err(err) => return speedtest_error_result(trimmed, checked_at, err.to_string()),
    };

    // Same logic as CC Switch: warm up once and ignore the result, then time the second request.
    let _ = client.get(parsed_url.clone()).timeout(timeout).send().await;

    let start = Instant::now();
    match client.get(parsed_url).timeout(timeout).send().await {
        Ok(resp) => ProviderSpeedtestResult {
            url: trimmed,
            ok: true,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(resp.status().as_u16()),
            error: None,
            checked_at,
        },
        Err(err) => {
            let error_message = if err.is_timeout() {
                "Request timed out".to_string()
            } else if err.is_connect() {
                "Connection failed".to_string()
            } else {
                err.to_string()
            };
            ProviderSpeedtestResult {
                url: trimmed,
                ok: false,
                latency_ms: None,
                status: err.status().map(|s| s.as_u16()),
                error: Some(error_message),
                checked_at,
            }
        }
    }
}

pub(super) async fn provider_speedtest(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderSpeedtestInput>,
) -> Result<Json<Provider>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| {
            s.db.provider_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;

    let result = run_endpoint_speedtest(&provider.base_url, input.timeout_secs).await;
    let provider_id = provider.id.clone();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.provider_update_speedtest(&provider_id, result)
    })
    .await?;
    publish_providers_overview_soon(state);
    Ok(Json(updated))
}

pub(super) async fn provider_probe(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderSpeedtestInput>,
) -> Result<Json<Provider>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| {
            s.db.provider_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;

    let result = run_protocol_probe(&state, &provider, input.timeout_secs).await;
    let provider_id = provider.id.clone();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.provider_update_speedtest(&provider_id, result)
    })
    .await?;
    publish_providers_overview_soon(state);
    Ok(Json(updated))
}

pub(super) async fn run_protocol_probe(
    state: &AppState,
    provider: &Provider,
    timeout_secs: Option<u64>,
) -> ProviderSpeedtestResult {
    let checked_at = chrono::Utc::now().timestamp();
    let timeout = Duration::from_secs(sanitize_speedtest_timeout(timeout_secs));
    let model = probe_model(provider);
    let secret = match probe_secret(state, provider).await {
        Ok(secret) => secret,
        Err(err) => {
            return speedtest_error_result(provider.base_url.clone(), checked_at, err.to_string());
        }
    };
    let (wire, body, upstream_path, url) = probe_request(provider, &model);
    let adapter = crate::providers::select(provider);
    let req = match adapter.build(
        provider,
        secret.as_deref(),
        &state.http,
        wire,
        &body,
        upstream_path.as_deref(),
    ) {
        Ok(req) => req,
        Err(err) => return speedtest_error_result(url, checked_at, err.to_string()),
    };

    let start = Instant::now();
    let resp = match req.timeout(timeout).send().await {
        Ok(resp) => resp,
        Err(err) => {
            let error_message = if err.is_timeout() {
                "Request timed out".to_string()
            } else if err.is_connect() {
                "Connection failed".to_string()
            } else {
                err.to_string()
            };
            return ProviderSpeedtestResult {
                url,
                ok: false,
                latency_ms: None,
                status: err.status().map(|s| s.as_u16()),
                error: Some(error_message),
                checked_at,
            };
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let preview = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(220)
            .collect::<String>();
        return ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: Some(if preview.trim().is_empty() {
                format!("HTTP {}", status.as_u16())
            } else {
                format!("HTTP {}: {}", status.as_u16(), preview)
            }),
            checked_at,
        };
    }

    let mut stream = resp.bytes_stream();
    match tokio::time::timeout(timeout, stream.next()).await {
        Ok(Some(Ok(chunk))) if !chunk.is_empty() => ProviderSpeedtestResult {
            url,
            ok: true,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: None,
            checked_at,
        },
        Ok(Some(Ok(_))) => ProviderSpeedtestResult {
            url,
            ok: true,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: None,
            checked_at,
        },
        Ok(Some(Err(err))) => ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: Some(format!("stream read failed: {err}")),
            checked_at,
        },
        Ok(None) => ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: Some("No response data received".into()),
            checked_at,
        },
        Err(_) => ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: None,
            status: Some(status.as_u16()),
            error: Some("Request timed out".into()),
            checked_at,
        },
    }
}

pub(super) async fn provider_sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderSyncInput>,
) -> Result<Json<ProviderSyncPreview>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            s.db.provider_get(&id2)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;

    let scope = input.scope.unwrap_or_else(|| "all".into());
    let mut updated_input = provider_to_input(&provider);
    let mut branding = SyncBranding::default();
    let mut financials = SyncFinancials::default();
    let mut note_parts: Vec<String> = Vec::new();

    if matches!(scope.as_str(), "all" | "brand") {
        if let Ok(found) = sync_fetch_branding(&state.http, &provider.base_url).await {
            if let Some(name) = found.display_name.clone() {
                updated_input.name = name.clone();
            }
            if let Some(url) = found.avatar_url.clone() {
                updated_input.avatar_url = Some(url);
            }
            branding = found;
            note_parts.push("brand".into());
        }
    }

    let supported_protocols = if matches!(scope.as_str(), "all" | "protocol") {
        note_parts.push("protocol".into());
        sync_detect_supported_protocols(&state, &provider).await
    } else {
        vec![provider_kind_to_api_label(provider.kind).to_string()]
    };

    if matches!(scope.as_str(), "all" | "usage") {
        if let Ok(secret) = probe_secret(&state, &provider).await {
            if let Some(secret) = secret {
                financials = sync_fetch_financials(&state.http, &provider, &secret).await;
                note_parts.push("usage".into());
            }
        }
    }

    let mut updated = if matches!(scope.as_str(), "all" | "models") {
        note_parts.push("models".into());
        refresh_remote_models_for_provider_if_supported(&state, &provider)
            .await
            .unwrap_or(provider.clone())
    } else {
        provider.clone()
    };

    updated_input.model_aliases = updated.model_aliases.clone();
    updated = run_blocking(state.clone(), {
        let id3 = id.clone();
        let input3 = updated_input.clone();
        move |s| s.db.provider_update(&id3, input3)
    })
    .await?;

    Ok(Json(ProviderSyncPreview {
        display_name: branding
            .display_name
            .clone()
            .unwrap_or_else(|| updated.name.clone()),
        avatar_url: branding
            .avatar_url
            .clone()
            .or_else(|| updated.avatar_url.clone()),
        balance: financials.balance,
        usage: financials.usage,
        supported_protocols,
        platform_guess: sync_guess_platform(&branding, &provider.base_url),
        note: if note_parts.is_empty() {
            "synced".into()
        } else {
            note_parts.join(", ")
        },
        provider: updated,
    }))
}

pub(super) async fn sync_detect_supported_protocols(
    state: &AppState,
    provider: &Provider,
) -> Vec<String> {
    let mut out = Vec::new();
    for kind in [
        vibe_protocol::ProviderKind::OpenaiResponses,
        vibe_protocol::ProviderKind::OpenaiChat,
        vibe_protocol::ProviderKind::Anthropic,
        vibe_protocol::ProviderKind::GeminiNative,
    ] {
        let mut candidate = provider.clone();
        candidate.kind = kind;
        let result = run_protocol_probe(state, &candidate, Some(5)).await;
        if result.ok || matches!(result.status, Some(400) | Some(401) | Some(403)) {
            out.push(provider_kind_to_api_label(kind).to_string());
        }
    }
    if out.is_empty() {
        out.push(provider_kind_to_api_label(provider.kind).to_string());
    }
    out
}

pub(super) fn provider_kind_to_api_label(kind: vibe_protocol::ProviderKind) -> &'static str {
    match kind {
        vibe_protocol::ProviderKind::OpenaiResponses => "openai-responses",
        vibe_protocol::ProviderKind::OpenaiChat => "openai-chat",
        vibe_protocol::ProviderKind::Anthropic => "anthropic",
        vibe_protocol::ProviderKind::GeminiNative => "gemini-native",
    }
}

pub(super) async fn sync_fetch_branding(
    http: &reqwest::Client,
    raw_url: &str,
) -> anyhow::Result<SyncBranding> {
    let url = reqwest::Url::parse(raw_url)?;
    let resp = tokio::time::timeout(
        Duration::from_secs(6),
        http.get(url.clone())
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send(),
    )
    .await??;
    if !resp.status().is_success() {
        anyhow::bail!("branding fetch status {}", resp.status());
    }
    let body = resp.text().await?;
    let doc = Html::parse_document(&body);
    let meta_selector = Selector::parse("meta").unwrap();
    let title_selector = Selector::parse("title").unwrap();
    let link_selector = Selector::parse("link").unwrap();
    let mut meta_by_key = HashMap::<String, String>::new();
    for meta in doc.select(&meta_selector) {
        let value = meta.value();
        let content = value.attr("content").unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        for key in [value.attr("property"), value.attr("name")]
            .into_iter()
            .flatten()
        {
            let key = key.trim().to_ascii_lowercase();
            if !key.is_empty() {
                meta_by_key
                    .entry(key)
                    .or_insert_with(|| content.to_string());
            }
        }
    }
    let title = doc
        .select(&title_selector)
        .next()
        .map(|n| n.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty());
    let display_name = [
        "og:site_name",
        "application-name",
        "apple-mobile-web-app-title",
        "og:title",
        "twitter:title",
    ]
    .iter()
    .find_map(|k| meta_by_key.get(*k).cloned())
    .or_else(|| {
        title.clone().map(|t| {
            t.split(['|', '-', '—', '·'])
                .next()
                .unwrap_or(&t)
                .trim()
                .to_string()
        })
    });
    let avatar_url = ["og:image", "twitter:image", "msapplication-tileimage"]
        .iter()
        .find_map(|k| {
            meta_by_key
                .get(*k)
                .and_then(|v| url.join(v).ok().map(|u| u.to_string()))
        })
        .or_else(|| {
            for link in doc.select(&link_selector) {
                let rel = link.value().attr("rel").unwrap_or("").to_ascii_lowercase();
                if !(rel.contains("icon") || rel.contains("apple-touch-icon")) {
                    continue;
                }
                if let Some(href) = link.value().attr("href") {
                    if let Ok(joined) = url.join(href) {
                        return Some(joined.to_string());
                    }
                }
            }
            url.join("/favicon.ico").ok().map(|u| u.to_string())
        });
    Ok(SyncBranding {
        display_name,
        avatar_url,
        title,
    })
}

pub(super) fn sync_guess_platform(branding: &SyncBranding, base_url: &str) -> Option<String> {
    let title = branding.title.as_deref().unwrap_or("");
    let avatar = branding.avatar_url.as_deref().unwrap_or("");
    if title.contains("AI API Gateway") && avatar.contains("/logo.png") {
        return Some("sub2api-like".into());
    }
    let base = base_url.to_ascii_lowercase();
    if base.contains("newapi") || base.contains("freeapi") {
        return Some("newapi-like".into());
    }
    None
}

pub(super) async fn sync_fetch_financials(
    http: &reqwest::Client,
    provider: &Provider,
    secret: &str,
) -> SyncFinancials {
    if !matches!(
        provider.kind,
        vibe_protocol::ProviderKind::OpenaiChat | vibe_protocol::ProviderKind::OpenaiResponses
    ) {
        return SyncFinancials::default();
    }
    let base = provider
        .base_url
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string();
    let mut out = SyncFinancials::default();
    let headers = |req: reqwest::RequestBuilder| req.bearer_auth(secret);
    if let Ok(Some(v)) =
        send_sync_credit(headers(http.get(format!("{base}/api/user/credit_grants")))).await
    {
        out.balance = Some(v);
    }
    if let Ok(Some(v)) =
        send_sync_usage(headers(http.get(format!("{base}/dashboard/billing/usage")))).await
    {
        out.usage = Some(v);
    }
    out
}

pub(super) async fn send_sync_credit(
    req: reqwest::RequestBuilder,
) -> anyhow::Result<Option<ProviderBalanceSnapshot>> {
    let resp = tokio::time::timeout(Duration::from_secs(6), req.send()).await??;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let value = resp.json::<serde_json::Value>().await?;
    let total = value
        .pointer("/total_granted")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    let used = value
        .pointer("/total_used")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    let remaining = value
        .pointer("/total_available")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    if total.is_none() && used.is_none() && remaining.is_none() {
        return Ok(None);
    }
    Ok(Some(ProviderBalanceSnapshot {
        currency: "USD".into(),
        balance: remaining.clone(),
        remaining,
        used,
        total,
        period: None,
        note: Some("credit grants".into()),
    }))
}

pub(super) async fn send_sync_usage(
    req: reqwest::RequestBuilder,
) -> anyhow::Result<Option<ProviderBalanceSnapshot>> {
    let resp = tokio::time::timeout(Duration::from_secs(6), req.send()).await??;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let value = resp.json::<serde_json::Value>().await?;
    let used = value
        .get("total_usage")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    if used.is_none() {
        return Ok(None);
    }
    Ok(Some(ProviderBalanceSnapshot {
        currency: "USD".into(),
        balance: None,
        remaining: None,
        used,
        total: None,
        period: Some("current window".into()),
        note: Some("usage".into()),
    }))
}

pub(super) async fn probe_secret(
    state: &AppState,
    provider: &Provider,
) -> anyhow::Result<Option<String>> {
    let credentials = state.db.credential_list_for_provider(&provider.id)?;
    if let Some(cred) = credentials.into_iter().find(|cred| cred.enabled) {
        if let Some(token) = cred.oauth_access_token.filter(|s| !s.trim().is_empty()) {
            return Ok(Some(token));
        }
        if let Some(auth_ref) = cred.auth_ref.filter(|s| !s.trim().is_empty()) {
            return Ok(Some(crate::secrets::resolve(&auth_ref)?));
        }
    }
    match provider
        .auth_ref
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(auth_ref) => Ok(Some(crate::secrets::resolve(auth_ref)?)),
        None => Ok(None),
    }
}

pub(super) fn probe_model(provider: &Provider) -> String {
    provider
        .model_aliases
        .first()
        .map(|a| a.upstream_model.clone())
        .or_else(|| provider.remote_models.first().cloned())
        .unwrap_or_else(|| match provider.kind {
            vibe_protocol::ProviderKind::Anthropic => "claude-sonnet-4-5".into(),
            vibe_protocol::ProviderKind::GeminiNative => "gemini-2.5-pro".into(),
            vibe_protocol::ProviderKind::OpenaiChat => "gpt-5.4".into(),
            vibe_protocol::ProviderKind::OpenaiResponses => "gpt-5.3-codex".into(),
        })
}

pub(super) fn probe_request(
    provider: &Provider,
    model: &str,
) -> (Wire, Vec<u8>, Option<String>, String) {
    let base = provider.base_url.trim_end_matches('/');
    match provider.kind {
        vibe_protocol::ProviderKind::Anthropic => {
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": true
            });
            (
                Wire::Anthropic,
                serde_json::to_vec(&body).unwrap_or_default(),
                None,
                format!("{base}/v1/messages"),
            )
        }
        vibe_protocol::ProviderKind::GeminiNative => {
            let path = format!("/v1beta/models/{model}:streamGenerateContent");
            let body = serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "ping"}]}]
            });
            (
                Wire::GeminiNative,
                serde_json::to_vec(&body).unwrap_or_default(),
                Some(path.clone()),
                format!("{base}{path}"),
            )
        }
        vibe_protocol::ProviderKind::OpenaiChat => {
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": true,
                "max_tokens": 1
            });
            (
                Wire::OpenaiChat,
                serde_json::to_vec(&body).unwrap_or_default(),
                None,
                format!("{base}/v1/chat/completions"),
            )
        }
        vibe_protocol::ProviderKind::OpenaiResponses => {
            let body = serde_json::json!({
                "model": model,
                "input": [{"role": "user", "content": "ping"}],
                "stream": true,
                "max_output_tokens": 16,
                "store": false
            });
            let path = if provider.base_url.contains("chatgpt.com/backend-api") {
                "/responses"
            } else {
                "/v1/responses"
            };
            (
                Wire::OpenaiResponses,
                serde_json::to_vec(&body).unwrap_or_default(),
                None,
                format!("{base}{path}"),
            )
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RollingHoursQuery {
    #[serde(default = "default_rolling_hours")]
    hours: i64,
}

pub(super) fn default_rolling_hours() -> i64 {
    24
}

pub(super) fn cb_state_rank(state: CbState) -> i32 {
    match state {
        CbState::Open => 3,
        CbState::HalfOpen => 2,
        CbState::Closed => 1,
    }
}

pub(super) fn effective_circuit_for_provider(
    state: &AppState,
    provider_id: &str,
    credential_ids: &[String],
) -> (String, i32, bool) {
    let mut worst = state.cb.state_of(provider_id);
    let mut max_failures = state.cb.consecutive_failures(provider_id) as i32;

    for cid in credential_ids {
        let s = state.cb.state_of(cid);
        if cb_state_rank(s) > cb_state_rank(worst) {
            worst = s;
        }
        let cf = state.cb.consecutive_failures(cid) as i32;
        if cf > max_failures {
            max_failures = cf;
        }
    }

    let is_healthy = worst != CbState::Open;
    (worst.as_str().to_string(), max_failures, is_healthy)
}

pub(super) fn build_provider_health_summary(
    state: &AppState,
    provider_id: &str,
    row: vibe_db::DbHealth,
    cred_ids: &[String],
    rolling_hours: i64,
    rolling: Option<vibe_protocol::ProviderStat>,
) -> ProviderHealthSummary {
    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(state, provider_id, cred_ids);
    let success_rate = if row.total_requests > 0 {
        row.total_successes as f64 / row.total_requests as f64
    } else {
        1.0
    };
    ProviderHealthSummary {
        cumulative: ProviderHealth {
            provider_id: row.provider_id,
            is_healthy,
            circuit_state,
            consecutive_failures,
            total_requests: row.total_requests,
            total_successes: row.total_successes,
            total_failures: row.total_failures,
            success_rate,
            last_success_at: row.last_success_at,
            last_failure_at: row.last_failure_at,
            last_error: row.last_error,
            avg_latency_ms: row.avg_latency_ms,
            updated_at: row.updated_at,
        },
        rolling_hours,
        rolling,
    }
}

pub(super) fn credential_is_rate_limited(c: &Credential, now_secs: i64) -> bool {
    let req_exhausted = c.rl_requests_remaining == Some(0)
        && c.rl_requests_reset_at
            .map(|t| t > now_secs)
            .unwrap_or(false);
    let tok_exhausted = c.rl_tokens_remaining == Some(0)
        && c.rl_tokens_reset_at.map(|t| t > now_secs).unwrap_or(false);
    req_exhausted || tok_exhausted
}

pub(super) fn build_provider_pool_summary(
    state: &AppState,
    provider: &Provider,
    credentials: Vec<Credential>,
    rolling_stats: &[vibe_db::CredentialRollingStat],
    plan_snapshots: &std::collections::HashMap<String, CredentialPlanSnapshot>,
    rolling_hours: i64,
) -> ProviderAuthPoolSummary {
    let now = chrono::Utc::now().timestamp();
    let mut total_credentials: i64 = 0;
    let mut enabled_credentials: i64 = 0;
    let mut available_credentials: i64 = 0;
    let mut rate_limited_credentials: i64 = 0;
    let mut open_circuit_credentials: i64 = 0;
    let mut statuses: Vec<CredentialPoolStatus> = Vec::new();

    let stat_map: std::collections::HashMap<&str, &vibe_db::CredentialRollingStat> = rolling_stats
        .iter()
        .map(|s| (s.credential_id.as_str(), s))
        .collect();

    let mut cred_ids: Vec<String> = Vec::new();
    let mut provider_last_error: Option<String> = None;
    for c in credentials {
        total_credentials += 1;
        if c.enabled {
            enabled_credentials += 1;
        }
        if provider_last_error.is_none() {
            provider_last_error = c.last_error.clone();
        }
        cred_ids.push(c.id.clone());
        let circuit_state = state.cb.state_of(&c.id).as_str().to_string();
        let circuit_open = circuit_state == CbState::Open.as_str();
        let circuit_open_remaining_secs = state.cb.open_remaining_secs(&c.id).map(|v| v as i64);
        if circuit_open {
            open_circuit_credentials += 1;
        }
        let plan_exhausted = plan_snapshots.get(&c.id).is_some_and(|snap| {
            let primary = snap
                .codex_primary_used_percent
                .or(snap.codex_5h_used_percent)
                .or(snap.codex_7d_used_percent)
                .unwrap_or(0.0);
            primary >= 99.95
        });
        let is_rate_limited = credential_is_rate_limited(&c, now) || plan_exhausted;
        if is_rate_limited {
            rate_limited_credentials += 1;
        }
        let credential_available = c.enabled && !circuit_open && !is_rate_limited;
        if credential_available {
            available_credentials += 1;
        }
        let stat = stat_map.get(c.id.as_str());
        statuses.push(CredentialPoolStatus {
            credential_id: c.id.clone(),
            label: c.label,
            enabled: c.enabled,
            auth_mode: if c.oauth_access_token.as_ref().is_some_and(|v| !v.is_empty()) {
                "oauth".into()
            } else {
                "auth_ref".into()
            },
            circuit_state,
            circuit_open,
            circuit_open_remaining_secs,
            consecutive_failures: state.cb.consecutive_failures(&c.id) as i32,
            is_rate_limited,
            rl_requests_remaining: c.rl_requests_remaining,
            rl_requests_reset_at: c.rl_requests_reset_at,
            rl_tokens_remaining: c.rl_tokens_remaining,
            rl_tokens_reset_at: c.rl_tokens_reset_at,
            oauth_expires_at: c.oauth_expires_at,
            last_error: c.last_error,
            last_used_at: c.last_used_at,
            rolling_requests: stat.map(|x| x.requests).unwrap_or(0),
            rolling_successes: stat.map(|x| x.successes).unwrap_or(0),
            rolling_failures: stat.map(|x| x.failures).unwrap_or(0),
            rolling_avg_latency_ms: stat.and_then(|x| x.avg_latency_ms),
        });
    }
    statuses.sort_by(|a, b| a.credential_id.cmp(&b.credential_id));
    let (provider_circuit_state, _, _) =
        effective_circuit_for_provider(state, &provider.id, &cred_ids);
    let provider_circuit_open = provider_circuit_state == CbState::Open.as_str();
    let provider_circuit_open_remaining_secs = cred_ids
        .iter()
        .filter_map(|cid| state.cb.open_remaining_secs(cid))
        .max()
        .map(|v| v as i64);

    ProviderAuthPoolSummary {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        kind: provider.kind,
        rolling_hours,
        total_credentials,
        enabled_credentials,
        available_credentials,
        rate_limited_credentials,
        open_circuit_credentials,
        provider_circuit_open_remaining_secs,
        provider_circuit_state,
        provider_circuit_open,
        provider_last_error,
        credentials: statuses,
    }
}

pub(super) async fn provider_pool_summary(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProviderAuthPoolSummary>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let (provider, creds, rolling_stats, plan_snapshots) = run_blocking(state.clone(), {
        let provider_id = id.clone();
        move |s| {
            let p =
                s.db.provider_get(&provider_id)?
                    .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
            let creds = s.db.credential_list_for_provider(&provider_id)?;
            let stat = s.db.credential_stats_for_provider(&provider_id, hours)?;
            let plan_credential_ids = creds.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
            let plan_snapshots = s.db.plan_snapshot_latest_map(&plan_credential_ids)?;
            Ok::<
                (
                    Provider,
                    Vec<Credential>,
                    Vec<vibe_db::CredentialRollingStat>,
                    std::collections::HashMap<String, CredentialPlanSnapshot>,
                ),
                anyhow::Error,
            >((p, creds, stat, plan_snapshots))
        }
    })
    .await?;
    Ok(Json(build_provider_pool_summary(
        &state,
        &provider,
        creds,
        &rolling_stats,
        &plan_snapshots,
        hours,
    )))
}

pub(super) async fn provider_pool_list(
    State(state): State<AppState>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<Vec<ProviderAuthPoolSummary>>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let mut out = Vec::new();
    for p in providers {
        let provider_id = p.id.clone();
        let (creds, rolling_stats, plan_snapshots) = run_blocking(state.clone(), move |s| {
            let creds = s.db.credential_list_for_provider(&provider_id)?;
            let stat = s.db.credential_stats_for_provider(&provider_id, hours)?;
            let plan_credential_ids = creds.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
            let plan_snapshots = s.db.plan_snapshot_latest_map(&plan_credential_ids)?;
            Ok::<
                (
                    Vec<Credential>,
                    Vec<vibe_db::CredentialRollingStat>,
                    std::collections::HashMap<String, CredentialPlanSnapshot>,
                ),
                anyhow::Error,
            >((creds, stat, plan_snapshots))
        })
        .await?;
        out.push(build_provider_pool_summary(
            &state,
            &p,
            creds,
            &rolling_stats,
            &plan_snapshots,
            hours,
        ));
    }
    out.sort_by(|a, b| a.provider_name.cmp(&b.provider_name));
    Ok(Json(out))
}
