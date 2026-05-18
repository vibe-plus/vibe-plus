use super::*;

// ---------------------------------------------------------------------------
// Provider CRUD
// ---------------------------------------------------------------------------

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
    let p = run_blocking(state.clone(), move |s| s.db.provider_insert(input)).await?;
    emit_app_event(
        &state,
        AppLogLevel::Info,
        "provider",
        "provider.created",
        serde_json::json!({
            "schema": 1,
            "provider": { "id": p.id, "name": p.name, "enabled": p.enabled },
        }),
        format!("Provider added: {}", p.name),
        None,
    );
    Ok(Json(p))
}

pub(super) async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let previous = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.provider_get(&id)
    })
    .await?;
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
    let event_type = match previous.as_ref().map(|old| old.enabled) {
        Some(true) if !p.enabled => "provider.disabled",
        Some(false) if p.enabled => "provider.enabled",
        _ => "provider.updated",
    };
    let message = match event_type {
        "provider.disabled" => format!("Provider disabled: {}", p.name),
        "provider.enabled" => format!("Provider enabled: {}", p.name),
        _ => format!("Provider updated: {}", p.name),
    };
    emit_app_event(
        &state,
        if event_type == "provider.disabled" {
            AppLogLevel::Warn
        } else {
            AppLogLevel::Info
        },
        "provider",
        event_type,
        serde_json::json!({
            "schema": 1,
            "provider": { "id": p.id, "name": p.name, "enabled": p.enabled },
        }),
        message,
        None,
    );
    Ok(Json(p))
}

pub(super) async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let previous = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.provider_get(&id)
    })
    .await?;
    run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.provider_delete(&id)
    })
    .await?;
    let provider_name = previous.as_ref().map(|p| p.name.as_str()).unwrap_or(&id);
    emit_app_event(
        &state,
        AppLogLevel::Warn,
        "provider",
        "provider.deleted",
        serde_json::json!({
            "schema": 1,
            "provider": { "id": id, "name": provider_name },
        }),
        format!("Provider deleted: {provider_name}"),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Upstream login / groups (NewAPI, Sub2API)
// ---------------------------------------------------------------------------

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
            .map_err(AppError)?;
            let cred_id = credential.id.clone();
            run_blocking(state.clone(), move |s| {
                s.db.credential_update_session(&cred_id, &token, None)
            })
            .await?;
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
            .map_err(AppError)?;
            let cred_id = credential.id.clone();
            run_blocking(state.clone(), move |s| {
                s.db.credential_update_session(&cred_id, &token, expires_at)
            })
            .await?;
            Ok(Json(vibe_protocol::CredentialLoginResponse {
                ok: true,
                note: None,
            }))
        }
        _ => Err(anyhow::anyhow!("login not supported for this credential vendor").into()),
    }
}

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
        .and_then(|s| crate::secrets::resolve(s).ok())
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
// Health / pool summaries
// ---------------------------------------------------------------------------

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
