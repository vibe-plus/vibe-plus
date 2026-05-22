//! Setup step: one-shot migration from CC Switch (`~/.cc-switch`) into the
//! local Vibe Plus DB.
//!
//! For each provider row that CC Switch knows about:
//!   1. Build a `ProviderInput` draft via `vibe_core::ccswitch::draft_from_ccswitch`.
//!   2. Look up an existing provider with the same (kind, base_url) — skip if
//!      it's already there.
//!   3. Otherwise `provider_insert`. If the draft carries a literal API key,
//!      also `credential_insert` so the imported provider is usable immediately.
//!
//! Read-only on the CC Switch side; never deletes or modifies `~/.cc-switch`.

use super::{CheckResult, StepOutcome};
use anyhow::{Context, Result};
use vibe_core::ccswitch::{draft_from_ccswitch, extract_default};
use vibe_db::Db;
use vibe_protocol::CredentialInput;

pub fn check() -> CheckResult {
    let snap = match extract_default() {
        Ok(s) => s,
        Err(_) => {
            return CheckResult::NotApplicable;
        }
    };
    let total = snap.providers.len();
    if total == 0 {
        return CheckResult::NotApplicable;
    }
    CheckResult::Pending {
        reason_zh: format!(
            "在 CC Switch 中发现 {total} 个供应商（{}）",
            snap.providers_by_app()
                .into_iter()
                .map(|(app, n)| format!("{app}={n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        reason_en: format!("found {total} providers in CC Switch"),
    }
}

pub fn run() -> Result<StepOutcome> {
    let snap = extract_default().context("read ~/.cc-switch")?;

    let db_path = vibe_core::paths::db_path()?;
    let dao = Db::open(&db_path).with_context(|| format!("open {}", db_path.display()))?;

    let mut imported = 0usize;
    let mut skipped_dup = 0usize;
    let mut skipped_no_url = 0usize;
    let mut with_credential = 0usize;
    let mut errors = 0usize;

    for row in &snap.providers {
        let draft = match draft_from_ccswitch(row, &snap.root) {
            Ok(Some(d)) => d,
            Ok(None) => {
                skipped_no_url += 1;
                continue;
            }
            Err(err) => {
                eprintln!("    [skip] {}/{}: {err:#}", row.app_type, row.id);
                errors += 1;
                continue;
            }
        };

        let existing = dao
            .provider_find_by_kind_and_base_url(draft.provider.kind, &draft.provider.base_url)
            .with_context(|| {
                format!(
                    "lookup existing provider for {} {}",
                    draft.provider.kind as i32, draft.provider.base_url
                )
            })?;
        if existing.is_some() {
            skipped_dup += 1;
            continue;
        }

        let provider = match dao.provider_insert(draft.provider.clone()) {
            Ok(p) => p,
            Err(err) => {
                eprintln!("    [skip] insert {}: {err:#}", draft.client);
                errors += 1;
                continue;
            }
        };
        imported += 1;

        if let Some(auth_ref) = draft.credential_auth_ref {
            let cred = CredentialInput {
                label: format!("ccswitch:{}", row.id),
                auth_ref: Some(auth_ref),
                plan_type: None,
                notes: Some(format!("imported from {}", draft.source_path)),
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
            match dao.credential_insert(&provider.id, cred, None) {
                Ok(_) => with_credential += 1,
                Err(err) => {
                    eprintln!("    [warn] credential insert for {}: {err:#}", provider.id);
                }
            }
        }
    }

    let summary_zh = format!(
        "导入 {imported} 个供应商（含凭证 {with_credential}），跳过重复 {skipped_dup}，无 base_url {skipped_no_url}，失败 {errors}"
    );
    let summary_en = format!(
        "imported {imported} providers ({with_credential} with credential), {skipped_dup} duplicates, {skipped_no_url} without base_url, {errors} errors"
    );
    Ok(StepOutcome { summary_zh, summary_en })
}
