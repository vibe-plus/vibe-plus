use super::*;

pub(super) async fn scan_local_providers() -> Json<Vec<local_import::LocalCandidate>> {
    Json(local_import::scan())
}

pub(super) fn resolve_secret(s: &str) -> String {
    if s.starts_with("literal:") {
        s["literal:".len()..].to_string()
    } else {
        crate::secrets::resolve(s).unwrap_or_else(|_| s.to_string())
    }
}

pub(super) fn provider_to_input(p: &Provider) -> ProviderInput {
    ProviderInput {
        name: p.name.clone(),
        group_name: p.group_name.clone(),
        avatar_url: p.avatar_url.clone(),
        kind: p.kind,
        base_url: p.base_url.clone(),
        protocols: p.effective_protocols(),
        host: p.host.clone(),
        auth_ref: p.auth_ref.clone(),
        enabled: p.enabled,
        priority: p.priority,
        supports_websocket: p.supports_websocket,
        passthrough_mode: p.passthrough_mode,
        model_aliases: p.model_aliases.clone(),
    }
}

pub(super) fn model_aliases_equal(
    a: &[vibe_protocol::ModelAlias],
    b: &[vibe_protocol::ModelAlias],
) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(a, b)| a.alias == b.alias && a.upstream_model == b.upstream_model)
}

/// For re-imports with the same fingerprint, merge tokens / cached identity to avoid skipping fresh local OAuth material.
pub(super) fn merge_codex_credential_on_reimport(
    existing: &Credential,
    incoming: CredentialInput,
) -> CredentialInput {
    CredentialInput {
        label: existing.label.clone(),
        auth_ref: existing.auth_ref.clone(),
        plan_type: existing.plan_type.clone(),
        notes: existing.notes.clone(),
        enabled: existing.enabled,
        priority: existing.priority,
        oauth_access_token: incoming
            .oauth_access_token
            .or(existing.oauth_access_token.clone()),
        oauth_refresh_token: incoming.oauth_refresh_token,
        oauth_expires_at: incoming.oauth_expires_at.or(existing.oauth_expires_at),
        oauth_cached_email: incoming
            .oauth_cached_email
            .or(existing.oauth_account_email.clone()),
        oauth_cached_subject: incoming
            .oauth_cached_subject
            .or(existing.oauth_account_subject.clone()),
        oauth_cached_plan_slug: incoming
            .oauth_cached_plan_slug
            .or(existing.oauth_chatgpt_plan_slug.clone()),
        upstream_vendor: incoming
            .upstream_vendor
            .or(existing.upstream_vendor.clone()),
        upstream_username: incoming
            .upstream_username
            .or(existing.upstream_username.clone()),
        upstream_session: incoming.upstream_session,
        upstream_session_expires_at: incoming.upstream_session_expires_at,
        upstream_group: incoming.upstream_group.or(existing.upstream_group.clone()),
        price_multiplier: if incoming.price_multiplier != 1.0 {
            incoming.price_multiplier
        } else {
            existing.price_multiplier
        },
    }
}

/// `POST /_vp/providers/import-local`
/// body: `["claude", "codex"]` — list of client names to import
///
/// For each candidate:
///   1. If a provider with the same kind + base_url already exists:
///      - Codex: merge local `auth*.json` into credential rows, deduplicated by fingerprint
///      - Claude: refresh this upstream `auth_ref` from local Claude Code config (settings / credentials / .env / process environment)
///      - Others: skip
///   2. Otherwise insert provider, then credentials (Codex: each auth*.json -> one oauth_* row)
pub(super) async fn import_local_providers(
    State(state): State<AppState>,
    Json(clients): Json<Vec<String>>,
) -> Result<Json<Vec<Provider>>, AppError> {
    let candidates = local_import::scan();
    let mut created = Vec::new();
    for c in candidates.iter().filter(|c| clients.contains(&c.client)) {
        let plan = local_import::candidate_to_plan(c)?;
        let kind = plan.provider.kind;
        let base = plan.provider.base_url.clone();
        let dup = run_blocking(state.clone(), {
            let base = base.clone();
            move |s| s.db.provider_find_by_kind_and_base_url(kind, &base)
        })
        .await?;
        if let Some(existing) = dup {
            // Existing same kind + base_url: Codex merges credentials; Claude refreshes provider-level auth_ref.
            if c.client.as_str() == "codex" {
                let pid = existing.id.clone();
                for cred in plan.credentials {
                    let fp = crate::auth_fingerprint::credential_fingerprint(
                        cred.auth_ref.as_deref(),
                        cred.oauth_access_token.as_deref(),
                    );
                    let has = run_blocking(state.clone(), {
                        let pid = pid.clone();
                        let fp = fp.clone();
                        move |s| s.db.credential_has_fingerprint_for_provider(&pid, &fp)
                    })
                    .await?;
                    if has {
                        let existing_opt = run_blocking(state.clone(), {
                            let pid = pid.clone();
                            let fp = fp.clone();
                            move |s| s.db.credential_get_by_provider_and_fingerprint(&pid, &fp)
                        })
                        .await?;
                        if let Some(existing) = existing_opt {
                            let cred_id_log = existing.id.clone();
                            let cred_id = existing.id.clone();
                            let merged = merge_codex_credential_on_reimport(&existing, cred);
                            run_blocking(state.clone(), {
                                let fp = fp.clone();
                                move |s| s.db.credential_update(&cred_id, merged, Some(fp))
                            })
                            .await?;
                            tracing::info!(
                                provider_id = %pid,
                                fingerprint = %fp,
                                cred_id = %cred_id_log,
                                "import-local: merged OAuth material into existing credential (same fingerprint)"
                            );
                        } else {
                            tracing::warn!(
                                provider_id = %pid,
                                fingerprint = %fp,
                                "import-local: fingerprint reported duplicate but credential row missing"
                            );
                        }
                        continue;
                    }
                    let pid2 = pid.clone();
                    run_blocking(state.clone(), move |s| {
                        s.db.credential_insert(&pid2, cred, Some(fp))
                    })
                    .await?;
                }
                if let Some(p) =
                    run_blocking(state.clone(), move |s| s.db.provider_get(&pid)).await?
                {
                    created.push(p);
                }
            } else if c.client.as_str() == "claude" {
                let pid = existing.id.clone();
                let scan_auth = plan.provider.auth_ref.clone();
                let existing_provider = run_blocking(state.clone(), {
                    let pid = pid.clone();
                    move |s| s.db.provider_get(&pid)
                })
                .await?
                .ok_or_else(|| anyhow::anyhow!("import-local: duplicate provider row missing"))?;
                let mut input = provider_to_input(&existing_provider);
                let mut changed = false;
                if let Some(ref ar) = scan_auth {
                    if input.auth_ref.as_ref() != Some(ar) {
                        input.auth_ref = Some(ar.clone());
                        changed = true;
                    }
                } else if let Some(ar) = local_import::anthropic_env_auth_ref() {
                    input.auth_ref = Some(ar);
                    changed = true;
                }
                let p = if changed {
                    run_blocking(state.clone(), move |s| s.db.provider_update(&pid, input)).await?
                } else {
                    existing_provider
                };
                created.push(p);
            } else if c.client.starts_with("ccs:") || c.client.starts_with("ccs-db:") {
                let pid = existing.id.clone();
                let existing_provider = run_blocking(state.clone(), {
                    let pid = pid.clone();
                    move |s| s.db.provider_get(&pid)
                })
                .await?
                .ok_or_else(|| anyhow::anyhow!("import-local: duplicate provider row missing"))?;
                let mut input = provider_to_input(&existing_provider);
                let mut changed = false;
                if let Some(ref ar) = plan.provider.auth_ref {
                    if input.auth_ref.as_ref() != Some(ar) {
                        input.auth_ref = Some(ar.clone());
                        changed = true;
                    }
                }
                if !model_aliases_equal(&input.model_aliases, &plan.provider.model_aliases) {
                    input.model_aliases = plan.provider.model_aliases.clone();
                    changed = true;
                }
                if input.name == existing_provider.name
                    && !existing_provider.name.starts_with("CCS ")
                    && plan.provider.name.starts_with("CCS ")
                {
                    input.name = plan.provider.name.clone();
                    changed = true;
                }
                let p = if changed {
                    run_blocking(state.clone(), move |s| s.db.provider_update(&pid, input)).await?
                } else {
                    existing_provider
                };
                created.push(p);
            } else {
                tracing::info!(%base, ?kind, "import-local: skipped duplicate provider");
            }
            continue;
        }
        let credentials = plan.credentials;
        let provider_input = plan.provider;
        let p = run_blocking(state.clone(), move |s| s.db.provider_insert(provider_input)).await?;
        let pid = p.id.clone();
        for cred in credentials {
            let pid2 = pid.clone();
            let fp = crate::auth_fingerprint::credential_fingerprint(
                cred.auth_ref.as_deref(),
                cred.oauth_access_token.as_deref(),
            );
            run_blocking(state.clone(), move |s| {
                s.db.credential_insert(&pid2, cred, Some(fp))
            })
            .await?;
        }
        created.push(p);
    }
    publish_providers_overview_soon(state);
    Ok(Json(created))
}

pub(super) async fn upsert_import_plan(
    state: AppState,
    plan: local_import::ImportPlan,
) -> Result<Provider, AppError> {
    let kind = plan.provider.kind;
    let base = plan.provider.base_url.clone();
    let dup = run_blocking(state.clone(), {
        let base = base.clone();
        move |s| s.db.provider_find_by_kind_and_base_url(kind, &base)
    })
    .await?;

    if let Some(existing) = dup {
        let pid = existing.id.clone();
        let mut input = provider_to_input(&existing);
        input.name = plan.provider.name;
        input.auth_ref = plan.provider.auth_ref;
        // Do NOT overwrite enabled from the plan — only the user can toggle providers on/off.
        input.priority = plan.provider.priority;
        input.model_aliases = plan.provider.model_aliases;
        let provider = run_blocking(state.clone(), {
            let pid = pid.clone();
            move |s| s.db.provider_update(&pid, input)
        })
        .await?;

        for cred in plan.credentials {
            let pid2 = provider.id.clone();
            let fp = crate::auth_fingerprint::credential_fingerprint(
                cred.auth_ref.as_deref(),
                cred.oauth_access_token.as_deref(),
            );
            let has = run_blocking(state.clone(), {
                let pid2 = pid2.clone();
                let fp = fp.clone();
                move |s| s.db.credential_has_fingerprint_for_provider(&pid2, &fp)
            })
            .await?;
            if !has {
                run_blocking(state.clone(), move |s| {
                    s.db.credential_insert(&pid2, cred, Some(fp))
                })
                .await?;
            }
        }
        publish_providers_overview_soon(state);
        return Ok(provider);
    }

    let credentials = plan.credentials;
    let provider =
        run_blocking(state.clone(), move |s| s.db.provider_insert(plan.provider)).await?;
    for cred in credentials {
        let pid = provider.id.clone();
        let fp = crate::auth_fingerprint::credential_fingerprint(
            cred.auth_ref.as_deref(),
            cred.oauth_access_token.as_deref(),
        );
        run_blocking(state.clone(), move |s| {
            s.db.credential_insert(&pid, cred, Some(fp))
        })
        .await?;
    }
    publish_providers_overview_soon(state);
    Ok(provider)
}

pub(super) async fn import_ccs_profile_bundle(
    State(state): State<AppState>,
    Json(bundle): Json<Value>,
) -> Result<Json<Provider>, AppError> {
    let plan = local_import::ccs_bundle_to_plan(&bundle)?;
    Ok(Json(upsert_import_plan(state, plan).await?))
}

#[derive(Debug, Deserialize)]
pub(super) struct CcSwitchImportRequest {
    url: String,
}

pub(super) async fn import_cc_switch_deeplink(
    State(state): State<AppState>,
    Json(input): Json<CcSwitchImportRequest>,
) -> Result<Json<Provider>, AppError> {
    let plan = local_import::cc_switch_deeplink_to_plan(&input.url)?;
    Ok(Json(upsert_import_plan(state, plan).await?))
}
