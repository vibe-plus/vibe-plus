//! Scan and import local provider candidates (CC Switch DB + future tool configs).

use crate::ccswitch::{
    ccswitch_client_id, draft_from_ccswitch, extract_default, parse_ccswitch_client_id,
    CcSwitchImportDraft,
};
use anyhow::{Context, Result};
use vibe_db::Db;
use vibe_protocol::{CredentialInput, LocalCandidate, Provider, ProviderInput, ProviderProtocol};

pub fn scan_local_candidates() -> Result<Vec<LocalCandidate>> {
    let mut out = Vec::new();
    if let Ok(snapshot) = extract_default() {
        for row in snapshot.providers {
            let Ok(Some(draft)) = draft_from_ccswitch(&row, &snapshot.root) else {
                continue;
            };
            out.push(local_candidate_from_draft(&draft));
        }
    }
    Ok(out)
}

pub fn import_local_clients(db: &Db, clients: &[String]) -> Result<Vec<Provider>> {
    let drafts = resolve_drafts(clients)?;
    let mut imported = Vec::new();
    for draft in drafts {
        let provider = if let Some(existing) = find_existing_provider(db, &draft.provider)? {
            let merged = db.provider_update(&existing.id, draft.provider)?;
            attach_ccswitch_credential(db, &merged, draft.credential_auth_ref.as_deref())?;
            merged
        } else {
            let provider = db.provider_insert(draft.provider)?;
            attach_ccswitch_credential(db, &provider, draft.credential_auth_ref.as_deref())?;
            provider
        };
        imported.push(provider);
    }
    Ok(imported)
}

fn find_existing_provider(db: &Db, input: &ProviderInput) -> Result<Option<Provider>> {
    let protocols = if input.protocols.is_empty() {
        vec![ProviderProtocol {
            kind: input.kind,
            base_url: input.base_url.clone(),
            model_aliases: Vec::new(),
        }]
    } else {
        input.protocols.clone()
    };
    for proto in protocols {
        if let Some(found) = db.provider_find_by_kind_and_base_url(proto.kind, &proto.base_url)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn attach_ccswitch_credential(db: &Db, provider: &Provider, auth_ref: Option<&str>) -> Result<()> {
    let Some(auth_ref) = auth_ref.filter(|s| !s.trim().is_empty()) else {
        return Ok(());
    };
    let cred = CredentialInput {
        label: format!("{} (CC Switch)", provider.name),
        auth_ref: Some(auth_ref.to_string()),
        plan_type: None,
        notes: Some("Imported from CC Switch".to_string()),
        enabled: true,
        priority: 100,
        oauth_access_token: None,
        oauth_refresh_token: None,
        oauth_expires_at: None,
        oauth_cached_email: None,
        oauth_cached_subject: None,
        oauth_cached_plan_slug: None,
        upstream_vendor: None,
        upstream_username: None,
        upstream_session: None,
        upstream_session_expires_at: None,
        upstream_group: None,
        price_multiplier: 1.0,
    };
    let _ = db.credential_insert(&provider.id, cred, None)?;
    Ok(())
}

fn resolve_drafts(clients: &[String]) -> Result<Vec<CcSwitchImportDraft>> {
    let snapshot = extract_default().ok();
    let mut out = Vec::new();
    for client in clients {
        if let Some((app, id)) = parse_ccswitch_client_id(client) {
            let snap = snapshot.as_ref().context("cc-switch not available")?;
            let row = snap
                .providers
                .iter()
                .find(|p| p.app_type == app && p.id == id)
                .with_context(|| format!("cc-switch provider not found: {client}"))?;
            if let Some(draft) = draft_from_ccswitch(row, &snap.root)? {
                out.push(draft);
            }
            continue;
        }
        if let Some(snap) = snapshot.as_ref() {
            for row in &snap.providers {
                if ccswitch_client_id(row.app_type.as_str(), &row.id) == *client {
                    if let Some(draft) = draft_from_ccswitch(row, &snap.root)? {
                        out.push(draft);
                    }
                }
            }
        }
    }
    Ok(out)
}

fn local_candidate_from_draft(draft: &CcSwitchImportDraft) -> LocalCandidate {
    let primary = draft
        .provider
        .protocols
        .first()
        .cloned()
        .unwrap_or_else(|| ProviderProtocol {
            kind: draft.provider.kind,
            base_url: draft.provider.base_url.clone(),
            model_aliases: Vec::new(),
        });
    LocalCandidate {
        client: draft.client.clone(),
        name: draft.provider.name.clone(),
        kind: primary.kind,
        base_url: primary.base_url,
        auth_ref: draft.credential_auth_ref.clone(),
        token_ok: draft.token_ok,
        proxy_managed: false,
        source_path: draft.source_path.clone(),
        default_aliases: draft.provider.model_aliases.clone(),
        extra_credentials: Vec::new(),
        protocols: draft.provider.protocols.clone(),
    }
}
