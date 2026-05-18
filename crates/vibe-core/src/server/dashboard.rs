use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct RollingHoursQuery {
    #[serde(default = "default_rolling_hours")]
    hours: i64,
}

pub(super) async fn provider_overview(
    State(state): State<AppState>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProvidersOverview>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    Ok(Json(build_providers_overview(state, hours).await?))
}

pub(super) async fn build_providers_overview(
    state: AppState,
    hours: i64,
) -> anyhow::Result<ProvidersOverview> {
    let (
        providers,
        health_rows,
        mut credentials_all,
        rolling_provider_stats,
        rolling_credential_stats,
        plan_snapshots,
    ) = run_blocking(state.clone(), move |s| {
        let providers = s.db.provider_list()?;
        let health_rows = s.db.health_list()?;
        let credentials = s.db.credential_list_all()?;
        let rolling_provider_stats = s.db.per_provider_stats(hours)?;
        let rolling_credential_stats = s.db.credential_stats_all(hours)?;
        let plan_credential_ids = credentials
            .iter()
            .filter(|c| {
                providers.iter().any(|p| {
                    p.id == c.provider_id && crate::router::provider_is_chatgpt_codex_official(p)
                })
            })
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let plan_snapshots = if plan_credential_ids.is_empty() {
            HashMap::new()
        } else {
            s.db.plan_snapshot_latest_map(&plan_credential_ids)?
        };
        Ok::<_, anyhow::Error>((
            providers,
            health_rows,
            credentials,
            rolling_provider_stats,
            rolling_credential_stats,
            plan_snapshots,
        ))
    })
    .await?;

    crate::oauth_identity::credentials_attach_oauth_identities(&mut credentials_all);

    let mut health_by_provider: HashMap<String, vibe_db::DbHealth> = health_rows
        .into_iter()
        .map(|r| (r.provider_id.clone(), r))
        .collect();
    let mut credentials_by_provider: HashMap<String, Vec<Credential>> = HashMap::new();
    let mut credential_ids_by_provider: HashMap<String, Vec<String>> = HashMap::new();
    for c in credentials_all {
        credential_ids_by_provider
            .entry(c.provider_id.clone())
            .or_default()
            .push(c.id.clone());
        credentials_by_provider
            .entry(c.provider_id.clone())
            .or_default()
            .push(c);
    }

    let mut rolling_by_provider: HashMap<String, vibe_protocol::ProviderStat> =
        rolling_provider_stats
            .into_iter()
            .map(|s| (s.provider_id.clone(), s))
            .collect();
    let mut rolling_by_credential: HashMap<String, vibe_db::CredentialRollingStat> =
        rolling_credential_stats
            .into_iter()
            .map(|s| (s.credential_id.clone(), s))
            .collect();
    let official_provider_ids: HashSet<String> = providers
        .iter()
        .filter(|p| crate::router::provider_is_chatgpt_codex_official(p))
        .map(|p| p.id.clone())
        .collect();
    let mut health = Vec::with_capacity(providers.len());
    let mut pools = Vec::with_capacity(providers.len());
    let mut codex_plans: HashMap<String, Vec<ProviderCodexPlanItem>> = HashMap::new();

    for p in &providers {
        let creds = credentials_by_provider
            .get(&p.id)
            .cloned()
            .unwrap_or_default();
        let cred_ids = credential_ids_by_provider
            .get(&p.id)
            .cloned()
            .unwrap_or_default();
        let row = health_by_provider
            .remove(&p.id)
            .unwrap_or_else(|| vibe_db::DbHealth {
                provider_id: p.id.clone(),
                is_healthy: true,
                consecutive_failures: 0,
                total_requests: 0,
                total_successes: 0,
                total_failures: 0,
                last_success_at: None,
                last_failure_at: None,
                last_error: None,
                avg_latency_ms: None,
                updated_at: 0,
            });
        health.push(build_provider_health_summary(
            &state,
            &p.id,
            row,
            &cred_ids,
            hours,
            rolling_by_provider.remove(&p.id),
        ));

        let credential_stats = creds
            .iter()
            .filter_map(|c| rolling_by_credential.remove(&c.id))
            .collect::<Vec<_>>();
        pools.push(build_provider_pool_summary(
            &state,
            p,
            creds.clone(),
            &credential_stats,
            &plan_snapshots,
            hours,
        ));

        if official_provider_ids.contains(&p.id) {
            codex_plans.insert(
                p.id.clone(),
                creds
                    .into_iter()
                    .map(|c| ProviderCodexPlanItem {
                        credential_id: c.id.clone(),
                        label: c.label,
                        plan: plan_snapshots.get(&c.id).cloned(),
                    })
                    .collect(),
            );
        }
    }
    pools.sort_by(|a, b| a.provider_name.cmp(&b.provider_name));

    Ok(ProvidersOverview {
        rolling_hours: hours,
        providers,
        health,
        pools,
        credentials: credentials_by_provider,
        codex_plans,
    })
}

pub(super) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn emit_app_log(state: &AppState, level: AppLogLevel, category: &str, message: String) {
    emit_app_event(
        state,
        level,
        category,
        "legacy.message",
        serde_json::json!({ "message": message }),
        message,
        None,
    );
}

pub(super) fn emit_app_event(
    state: &AppState,
    level: AppLogLevel,
    category: &str,
    event_type: &str,
    payload: serde_json::Value,
    message: String,
    detail: Option<String>,
) {
    let ev = AppLogEvent {
        ts: now_secs(),
        level,
        event_type: event_type.to_string(),
        payload,
        category: category.to_string(),
        message,
        detail,
    };
    state.app_logs.push(ev);
}

pub(super) async fn provider_health(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProviderHealthSummary>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;

    let db_row = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let row = s.db.health_get(&id2)?;
            Ok(row)
        }
    })
    .await?
    .unwrap_or_else(|| vibe_db::DbHealth {
        provider_id: id.clone(),
        is_healthy: true,
        consecutive_failures: 0,
        total_requests: 0,
        total_successes: 0,
        total_failures: 0,
        last_success_at: None,
        last_failure_at: None,
        last_error: None,
        avg_latency_ms: None,
        updated_at: 0,
    });
    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(&state, &id, &cred_ids);

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    let cumulative = ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy,
        circuit_state,
        consecutive_failures,
        total_requests: db_row.total_requests,
        total_successes: db_row.total_successes,
        total_failures: db_row.total_failures,
        success_rate,
        last_success_at: db_row.last_success_at,
        last_failure_at: db_row.last_failure_at,
        last_error: db_row.last_error,
        avg_latency_ms: db_row.avg_latency_ms,
        updated_at: db_row.updated_at,
    };

    let rolling = run_blocking(state.clone(), move |s| {
        s.db.provider_stat_single(&id, hours)
    })
    .await?;

    Ok(Json(ProviderHealthSummary {
        cumulative,
        rolling_hours: hours,
        rolling,
    }))
}

pub(super) async fn provider_health_list(
    State(state): State<AppState>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<Vec<ProviderHealthSummary>>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let rows = run_blocking(state.clone(), |s| s.db.health_list()).await?;
    let creds_all = run_blocking(state.clone(), |s| s.db.credential_list_all()).await?;
    let rolling_stats =
        run_blocking(state.clone(), move |s| s.db.per_provider_stats(hours)).await?;

    let mut row_by_provider: std::collections::HashMap<String, vibe_db::DbHealth> = rows
        .into_iter()
        .map(|r| (r.provider_id.clone(), r))
        .collect();
    let mut cred_ids_by_provider: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for c in creds_all {
        cred_ids_by_provider
            .entry(c.provider_id)
            .or_default()
            .push(c.id);
    }
    let mut rolling_by_provider = rolling_stats
        .into_iter()
        .map(|s| (s.provider_id.clone(), s))
        .collect::<std::collections::HashMap<_, _>>();

    let mut out = Vec::with_capacity(providers.len());
    for p in providers {
        let row = row_by_provider
            .remove(&p.id)
            .unwrap_or_else(|| vibe_db::DbHealth {
                provider_id: p.id.clone(),
                is_healthy: true,
                consecutive_failures: 0,
                total_requests: 0,
                total_successes: 0,
                total_failures: 0,
                last_success_at: None,
                last_failure_at: None,
                last_error: None,
                avg_latency_ms: None,
                updated_at: 0,
            });
        let cred_ids = cred_ids_by_provider.get(&p.id).cloned().unwrap_or_default();
        let (circuit_state, consecutive_failures, is_healthy) =
            effective_circuit_for_provider(&state, &p.id, &cred_ids);
        let success_rate = if row.total_requests > 0 {
            row.total_successes as f64 / row.total_requests as f64
        } else {
            1.0
        };
        out.push(ProviderHealthSummary {
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
            rolling_hours: hours,
            rolling: rolling_by_provider.remove(&p.id),
        });
    }

    Ok(Json(out))
}

pub(super) async fn provider_circuit_reset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProviderHealth>, AppError> {
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;
    if state.cb.reset(&id).is_some() {
        emit_app_log(
            &state,
            AppLogLevel::Info,
            "circuit",
            format!("Circuit reset: {id}"),
        );
    }
    for cid in &cred_ids {
        state.cb.reset(cid);
    }
    let db_row = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| s.db.health_get(&id2)
    })
    .await?
    .unwrap_or_else(|| vibe_db::DbHealth {
        provider_id: id.clone(),
        is_healthy: true,
        consecutive_failures: 0,
        total_requests: 0,
        total_successes: 0,
        total_failures: 0,
        last_success_at: None,
        last_failure_at: None,
        last_error: None,
        avg_latency_ms: None,
        updated_at: 0,
    });

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(&state, &id, &cred_ids);
    Ok(Json(ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy,
        circuit_state,
        consecutive_failures,
        total_requests: db_row.total_requests,
        total_successes: db_row.total_successes,
        total_failures: db_row.total_failures,
        success_rate,
        last_success_at: db_row.last_success_at,
        last_failure_at: db_row.last_failure_at,
        last_error: db_row.last_error,
        avg_latency_ms: db_row.avg_latency_ms,
        updated_at: db_row.updated_at,
    }))
}

pub(super) async fn health_all_providers(
    State(state): State<AppState>,
) -> Result<Json<HealthSummary>, AppError> {
    let rows = run_blocking(state.clone(), |s| s.db.health_list()).await?;
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let creds_all = run_blocking(state.clone(), |s| s.db.credential_list_all()).await?;
    let mut cred_ids_by_provider: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for c in creds_all {
        cred_ids_by_provider
            .entry(c.provider_id)
            .or_default()
            .push(c.id);
    }

    // Build health entries for every known provider (even those never hit)
    let mut health_map: std::collections::HashMap<String, vibe_db::DbHealth> = rows
        .into_iter()
        .map(|r| (r.provider_id.clone(), r))
        .collect();

    for p in &providers {
        health_map
            .entry(p.id.clone())
            .or_insert_with(|| vibe_db::DbHealth {
                provider_id: p.id.clone(),
                is_healthy: true,
                consecutive_failures: 0,
                total_requests: 0,
                total_successes: 0,
                total_failures: 0,
                last_success_at: None,
                last_failure_at: None,
                last_error: None,
                avg_latency_ms: None,
                updated_at: 0,
            });
    }

    let all: Vec<ProviderHealth> = health_map
        .into_values()
        .map(|row| {
            let cred_ids = cred_ids_by_provider
                .get(&row.provider_id)
                .cloned()
                .unwrap_or_default();
            let (cs, cf, is_healthy) =
                effective_circuit_for_provider(&state, &row.provider_id, &cred_ids);
            let success_rate = if row.total_requests > 0 {
                row.total_successes as f64 / row.total_requests as f64
            } else {
                1.0
            };
            ProviderHealth {
                provider_id: row.provider_id,
                is_healthy,
                circuit_state: cs,
                consecutive_failures: cf,
                total_requests: row.total_requests,
                total_successes: row.total_successes,
                total_failures: row.total_failures,
                success_rate,
                last_success_at: row.last_success_at,
                last_failure_at: row.last_failure_at,
                last_error: row.last_error,
                avg_latency_ms: row.avg_latency_ms,
                updated_at: row.updated_at,
            }
        })
        .collect();

    let healthy_providers = all.iter().filter(|h| h.is_healthy).count();
    let total_providers = all.len();

    Ok(Json(HealthSummary {
        providers: all,
        total_providers,
        healthy_providers,
    }))
}

// ---------------------------------------------------------------------------
// Credential CRUD
// ---------------------------------------------------------------------------

pub(super) async fn list_credentials(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<Credential>>, AppError> {
    let mut v = run_blocking(state, move |s| {
        s.db.credential_list_for_provider(&provider_id)
    })
    .await?;
    crate::oauth_identity::credentials_attach_oauth_identities(&mut v);
    Ok(Json(v))
}

pub(super) async fn list_credentials_all(
    State(state): State<AppState>,
) -> Result<Json<std::collections::HashMap<String, Vec<Credential>>>, AppError> {
    let mut creds = run_blocking(state, move |s| s.db.credential_list_all()).await?;
    crate::oauth_identity::credentials_attach_oauth_identities(&mut creds);
    let mut out: std::collections::HashMap<String, Vec<Credential>> =
        std::collections::HashMap::new();
    for cred in creds {
        out.entry(cred.provider_id.clone()).or_default().push(cred);
    }
    Ok(Json(out))
}

pub(super) async fn create_credential(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<Credential>, AppError> {
    let label = input.label.clone();
    let fp = crate::auth_fingerprint::credential_fingerprint(
        input.auth_ref.as_deref(),
        input.oauth_access_token.as_deref(),
    );
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_insert(&provider_id, input, Some(fp))
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_event(
        &state,
        AppLogLevel::Info,
        "credential",
        "credential.created",
        serde_json::json!({
            "schema": 1,
            "credential": { "id": c.id, "label": c.label },
            "provider": { "id": c.provider_id },
        }),
        format!("Credential added: {label}"),
        None,
    );
    Ok(Json(c))
}

pub(super) async fn get_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    let mut c = run_blocking(state, move |s| s.db.credential_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    Ok(Json(c))
}

pub(super) async fn update_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<Credential>, AppError> {
    let label = input.label.clone();
    let fp = crate::auth_fingerprint::credential_fingerprint(
        input.auth_ref.as_deref(),
        input.oauth_access_token.as_deref(),
    );
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_update(&id, input, Some(fp))
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_event(
        &state,
        AppLogLevel::Info,
        "credential",
        "credential.updated",
        serde_json::json!({
            "schema": 1,
            "credential": { "id": c.id, "label": c.label },
            "provider": { "id": c.provider_id },
        }),
        format!("Credential updated: {label}"),
        None,
    );
    Ok(Json(c))
}

pub(super) async fn credential_plan_latest(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<CredentialPlanSnapshot>>, AppError> {
    let snap = run_blocking(state, move |s| s.db.plan_snapshot_latest(&id)).await?;
    Ok(Json(snap))
}

pub(super) async fn refresh_codex_plan_for_credential(
    state: &AppState,
    cred: &Credential,
) -> anyhow::Result<()> {
    let Some(access) = cred.oauth_access_token.as_ref().filter(|t| !t.is_empty()) else {
        anyhow::bail!("credential has no OAuth access token");
    };
    let oauth = crate::forward::CredOAuth {
        access_token: access.clone(),
        expires_at: cred.oauth_expires_at,
    };
    let token = crate::forward::resolve_oauth_token(state, Some(cred.id.as_str()), oauth).await?;
    let acct = crate::auth_fingerprint::chatgpt_account_id_from_access_token(&token);
    let snap = crate::codex_wham_usage::fetch_wham_plan_snapshot(
        &state.http,
        &token,
        acct.as_deref(),
        &cred.id,
    )
    .await?;
    let db = state.db.clone();
    let snap_ins = snap.clone();
    tokio::task::spawn_blocking(move || db.plan_snapshot_insert(&snap_ins)).await??;
    Ok(())
}

pub(super) async fn credential_plan_refresh(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CredentialPlanSnapshot>, AppError> {
    let cred_opt = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.credential_get(&id)
    })
    .await?;
    let cred = cred_opt.ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let pid = cred.provider_id.clone();
    let provider_opt = run_blocking(state.clone(), move |s| s.db.provider_get(&pid)).await?;
    let provider = provider_opt.ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    if !crate::router::provider_is_chatgpt_codex_official(&provider) {
        return Err(anyhow::anyhow!(
            "Not a ChatGPT Codex official provider (chatgpt.com … /backend-api/…/codex)."
        )
        .into());
    }
    refresh_codex_plan_for_credential(&state, &cred).await?;
    let snap_opt = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.plan_snapshot_latest(&id)
    })
    .await?;
    let snap = snap_opt.ok_or_else(|| anyhow::anyhow!("plan snapshot missing after refresh"))?;
    Ok(Json(snap))
}

pub(super) async fn provider_codex_plan_list(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<ProviderCodexPlanItem>>, AppError> {
    let pid = provider_id.clone();
    let items = run_blocking(state.clone(), move |s| {
        let p =
            s.db.provider_get(&pid)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if !crate::router::provider_is_chatgpt_codex_official(&p) {
            return Ok(Vec::new());
        }
        let creds = s.db.credential_list_for_provider(&pid)?;
        let mut out = Vec::new();
        for c in creds {
            let plan = s.db.plan_snapshot_latest(&c.id)?;
            out.push(ProviderCodexPlanItem {
                credential_id: c.id,
                label: c.label,
                plan,
            });
        }
        Ok(out)
    })
    .await?;
    Ok(Json(items))
}

pub(super) async fn provider_codex_plan_list_all(
    State(state): State<AppState>,
) -> Result<Json<std::collections::HashMap<String, Vec<ProviderCodexPlanItem>>>, AppError> {
    let items = run_blocking(state.clone(), move |s| {
        let providers = s.db.provider_list()?;
        let official_provider_ids = providers
            .into_iter()
            .filter(crate::router::provider_is_chatgpt_codex_official)
            .map(|p| p.id)
            .collect::<std::collections::HashSet<_>>();
        if official_provider_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let creds = s.db.credential_list_all()?;
        let credential_ids = creds
            .iter()
            .filter(|c| official_provider_ids.contains(&c.provider_id))
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let plans = s.db.plan_snapshot_latest_map(&credential_ids)?;
        let mut out: std::collections::HashMap<String, Vec<ProviderCodexPlanItem>> =
            std::collections::HashMap::new();

        for c in creds {
            if !official_provider_ids.contains(&c.provider_id) {
                continue;
            }
            out.entry(c.provider_id.clone())
                .or_default()
                .push(ProviderCodexPlanItem {
                    credential_id: c.id.clone(),
                    label: c.label,
                    plan: plans.get(&c.id).cloned(),
                });
        }

        Ok(out)
    })
    .await?;
    Ok(Json(items))
}

pub(super) async fn provider_codex_plan_refresh_all(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<CodexPlanRefreshResult>, AppError> {
    let creds = run_blocking(state.clone(), move |s| {
        let p =
            s.db.provider_get(&provider_id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if !crate::router::provider_is_chatgpt_codex_official(&p) {
            anyhow::bail!("not a ChatGPT Codex official provider");
        }
        s.db.credential_list_for_provider(&provider_id)
    })
    .await?;

    let mut attempted = 0usize;
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for c in creds {
        if c.oauth_access_token
            .as_ref()
            .map_or(true, |t: &String| t.is_empty())
        {
            continue;
        }
        attempted += 1;
        match refresh_codex_plan_for_credential(&state, &c).await {
            Err(e) => errors.push(format!("{}: {e}", c.label)),
            Ok(()) => ok += 1,
        }
    }

    Ok(Json(CodexPlanRefreshResult {
        attempted,
        ok,
        errors,
    }))
}

pub(super) async fn delete_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let id_clone = id.clone();
    run_blocking(state.clone(), move |s| s.db.credential_delete(&id_clone)).await?;
    emit_app_event(
        &state,
        AppLogLevel::Warn,
        "credential",
        "credential.deleted",
        serde_json::json!({
            "schema": 1,
            "credential": { "id": id },
        }),
        format!("Credential deleted: {id}"),
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn enable_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    state.cb.reset(&id);
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_set_enabled(&id, true)
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_event(
        &state,
        AppLogLevel::Info,
        "credential",
        "credential.enabled",
        serde_json::json!({
            "schema": 1,
            "credential": { "id": c.id, "label": c.label },
            "provider": { "id": c.provider_id },
        }),
        format!("Credential enabled: {}", c.label),
        None,
    );
    Ok(Json(c))
}

pub(super) async fn disable_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    state.cb.reset(&id);
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_set_enabled(&id, false)
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_event(
        &state,
        AppLogLevel::Warn,
        "credential",
        "credential.disabled",
        serde_json::json!({
            "schema": 1,
            "credential": { "id": c.id, "label": c.label },
            "provider": { "id": c.provider_id },
        }),
        format!("Credential disabled: {}", c.label),
        None,
    );
    Ok(Json(c))
}

pub(super) async fn credential_circuit_reset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if state.cb.reset(&id).is_some() {
        emit_app_log(
            &state,
            AppLogLevel::Info,
            "circuit",
            format!("Circuit reset: {id}"),
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// App logs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct AppLogQuery {
    limit: Option<i64>,
    since: Option<i64>,
}

pub(super) async fn list_app_logs(
    State(state): State<AppState>,
    Query(q): Query<AppLogQuery>,
) -> Result<Json<Vec<AppLogEvent>>, AppError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 500);
    let since = q.since;
    Ok(Json(state.app_logs.list(limit, since)))
}

// ---------------------------------------------------------------------------
// Usage / stats
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct UsageQuery {
    hours: Option<i64>,
}

pub(super) async fn usage_summary(
    State(_state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageSummary>, AppError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 30);
    Ok(Json(UsageSummary {
        range: format!("last_{hours}h"),
        requests: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        estimated_cost_usd: "0".into(),
    }))
}

pub(super) async fn dashboard_stats(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<DashboardStats>, AppError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 30);
    Ok(Json(build_dashboard_stats(state, hours).await?))
}

pub(super) async fn build_dashboard_stats(
    state: AppState,
    hours: i64,
) -> anyhow::Result<DashboardStats> {
    let (
        window_totals,
        last_hour_totals,
        last_24h_totals,
        (_, p95_latency_ms),
        top_models,
        per_provider,
    ) = run_blocking(state, move |s| {
        let window_totals = s.db.dashboard_rolling_totals(hours)?;
        let last_hour_totals = s.db.dashboard_rolling_totals(1)?;
        let last_24h_totals = s.db.dashboard_rolling_totals(24)?;
        let latency = s.db.latency_percentiles(hours)?;
        let top_models = s.db.top_models(hours, 6)?;
        let per_provider = s.db.per_provider_stats(hours)?;
        Ok::<_, anyhow::Error>((
            window_totals,
            last_hour_totals,
            last_24h_totals,
            latency,
            top_models,
            per_provider,
        ))
    })
    .await?;

    let window_label = match hours {
        1 => "Last 1 hour".to_string(),
        5 => "Last 5 hours".to_string(),
        24 => "Last 24 hours".to_string(),
        168 => "Last 7 days".to_string(),
        720 => "Last 30 days".to_string(),
        h if h % 24 == 0 && h > 24 => format!("Last {} days", h / 24),
        h => format!("Last {h} hours"),
    };

    Ok(DashboardStats {
        window_hours: hours,
        window_label,
        requests_in_window: window_totals.requests,
        success_rate_in_window: success_rate(window_totals.successes, window_totals.requests),
        input_tokens_in_window: window_totals.input_tokens,
        output_tokens_in_window: window_totals.output_tokens,
        output_tokens_per_sec_in_window: window_totals.output_tokens_per_sec,
        decode_output_tokens_per_sec_in_window: window_totals.decode_output_tokens_per_sec,
        requests_last_hour: last_hour_totals.requests,
        requests_last_24h: last_24h_totals.requests,
        success_rate_last_hour: success_rate(last_hour_totals.successes, last_hour_totals.requests),
        avg_latency_ms: window_totals.avg_latency_ms,
        p95_latency_ms,
        input_tokens_last_24h: last_24h_totals.input_tokens,
        output_tokens_last_24h: last_24h_totals.output_tokens,
        top_models,
        per_provider,
    })
}

fn success_rate(successes: i64, requests: i64) -> f64 {
    if requests > 0 {
        successes as f64 / requests as f64
    } else {
        1.0
    }
}
