//! Deferred gateway maintenance: runs after the HTTP listener is up, gated by
//! persisted task records (last run time, gateway version, input fingerprint).

use std::path::{Path, PathBuf};
use std::time::Duration;

use vibe_db::{Db, GatewayMaintenanceTaskRecord};
use vibe_observability::{ObservabilityStore, RetentionPolicy};
use vibe_protocol::{CodexHistorySummary, CodexHistoryUnifyInput};

use crate::codex_history;
use crate::VERSION;

pub const TASK_CODEX_UNIFY: &str = "codex_history_unify";
pub const TASK_OBS_LEGACY_COPY: &str = "observability_legacy_copy";
pub const TASK_INLINE_BODIES: &str = "inline_body_refs";
pub const TASK_SHORT_LOG_PRUNE: &str = "short_log_prune";
pub const TASK_OBS_PRUNE: &str = "observability_prune";

const OBS_LEGACY_TASK_VERSION: &str = "attach_v1";
const INLINE_BODY_TASK_VERSION: &str = "batch_v1";
const PRUNE_INTERVAL_SECS: i64 = 6 * 3600;
const INLINE_BODY_BATCH: i64 = 10_000;
const PERIODIC_TICK_SECS: u64 = 30 * 60;

/// Run Codex unify only when the gateway version or Codex home fingerprint changed.
pub fn run_codex_unify_if_due(db: &Db, gateway_version: &str) -> Option<CodexHistorySummary> {
    if !should_run_codex_unify(db, gateway_version) {
        return None;
    }
    let stamp = codex_history::codex_home_change_stamp();
    let home = codex_history::default_codex_home().ok()?;
    if !home.exists() {
        return None;
    }
    match codex_history::unify(CodexHistoryUnifyInput {
        provider: codex_history::DEFAULT_PROVIDER_ID.to_string(),
        from_providers: Vec::new(),
        apply: true,
        no_backup: false,
        codex_home: Some(home.to_string_lossy().to_string()),
    }) {
        Ok(summary) => {
            let result = format!(
                "sqlite_rows={} rollout_fields={}",
                summary.sqlite_rows_changed, summary.rollout_fields_changed
            );
            let _ = db.maintenance_task_record_run(
                TASK_CODEX_UNIFY,
                Some(gateway_version),
                stamp.as_deref(),
                Some(&result),
            );
            Some(summary)
        }
        Err(err) => {
            tracing::warn!(error = %err, "codex history unify failed");
            let _ = db.maintenance_task_record_run(
                TASK_CODEX_UNIFY,
                Some(gateway_version),
                stamp.as_deref(),
                Some(&format!("error: {err:#}")),
            );
            None
        }
    }
}

pub fn spawn_deferred_maintenance(
    db: Db,
    observability: Option<ObservabilityStore>,
    legacy_db_path: PathBuf,
) {
    let gateway_version = VERSION.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let version = gateway_version.clone();
        let db_startup = db.clone();
        if let Err(e) = tokio::task::spawn_blocking({
            let obs = observability.clone();
            let legacy = legacy_db_path.clone();
            move || run_startup_pass(&db_startup, obs.as_ref(), &legacy, &version)
        })
        .await
        {
            tracing::warn!(?e, "startup maintenance task join failed");
        }

        loop {
            tokio::time::sleep(Duration::from_secs(PERIODIC_TICK_SECS)).await;
            let version = gateway_version.clone();
            let db_periodic = db.clone();
            if let Err(e) = tokio::task::spawn_blocking({
                let obs = observability.clone();
                let legacy = legacy_db_path.clone();
                move || run_periodic_pass(&db_periodic, obs.as_ref(), &legacy, &version)
            })
            .await
            {
                tracing::warn!(?e, "periodic maintenance task join failed");
            }
        }
    });
}

fn run_startup_pass(
    db: &Db,
    observability: Option<&ObservabilityStore>,
    legacy_db_path: &Path,
    gateway_version: &str,
) {
    if let Some(summary) = run_codex_unify_if_due(db, gateway_version) {
        let changes = summary.sqlite_rows_changed + summary.rollout_fields_changed;
        if changes > 0 {
            tracing::info!(
                sqlite_rows = summary.sqlite_rows_changed,
                rollout_fields = summary.rollout_fields_changed,
                "codex history unified (deferred startup)"
            );
        }
    }
    run_observability_legacy_copy(db, observability, legacy_db_path);
    run_inline_body_migration(db);
    run_prune_if_due(db, observability);
}

fn run_periodic_pass(
    db: &Db,
    observability: Option<&ObservabilityStore>,
    legacy_db_path: &Path,
    gateway_version: &str,
) {
    if codex_stamp_changed(db, gateway_version) {
        let _ = run_codex_unify_if_due(db, gateway_version);
    }
    if has_pending_inline_bodies(db) {
        run_inline_body_migration(db);
    }
    if !observability_legacy_done(db) {
        run_observability_legacy_copy(db, observability, legacy_db_path);
    }
    run_prune_if_due(db, observability);
}

fn should_run_codex_unify(db: &Db, gateway_version: &str) -> bool {
    codex_history::codex_home_change_stamp()
        .is_some_and(|stamp| codex_stamp_changed_with(db, gateway_version, &stamp))
}

fn codex_stamp_changed(db: &Db, gateway_version: &str) -> bool {
    codex_history::codex_home_change_stamp()
        .is_some_and(|stamp| codex_stamp_changed_with(db, gateway_version, &stamp))
}

fn codex_stamp_changed_with(db: &Db, gateway_version: &str, stamp: &str) -> bool {
    match db.maintenance_task_get(TASK_CODEX_UNIFY).ok().flatten() {
        None => true,
        Some(record) => {
            record.last_version.as_deref() != Some(gateway_version)
                || record.last_input_stamp.as_deref() != Some(stamp)
        }
    }
}

fn observability_legacy_done(db: &Db) -> bool {
    db.maintenance_task_get(TASK_OBS_LEGACY_COPY)
        .ok()
        .flatten()
        .is_some_and(|r| r.last_version.as_deref() == Some(OBS_LEGACY_TASK_VERSION))
}

fn run_observability_legacy_copy(
    db: &Db,
    observability: Option<&ObservabilityStore>,
    legacy_db_path: &Path,
) {
    let Some(obs) = observability else {
        return;
    };
    if observability_legacy_done(db) {
        return;
    }
    let mut result = String::new();
    match obs.migrate_from_legacy_path(legacy_db_path) {
        Ok(()) => {
            result.push_str("path=ok");
            if let Err(e) = obs.migrate_from_legacy(db) {
                tracing::warn!(?e, "observability row-wise legacy migration failed");
                result.push_str(";rows=err");
            } else {
                result.push_str(";rows=ok");
            }
            let _ = db.maintenance_task_record_run(
                TASK_OBS_LEGACY_COPY,
                Some(OBS_LEGACY_TASK_VERSION),
                None,
                Some(&result),
            );
            tracing::info!(%result, "observability legacy migration completed (deferred)");
        }
        Err(e) => {
            tracing::warn!(?e, "legacy observability migration failed");
            let _ = db.maintenance_task_record_run(
                TASK_OBS_LEGACY_COPY,
                Some(OBS_LEGACY_TASK_VERSION),
                None,
                Some(&format!("error: {e:#}")),
            );
        }
    }
}

fn has_pending_inline_bodies(db: &Db) -> bool {
    db.with_short(|c| {
        let pending: i64 = c.query_row(
            "SELECT 1 FROM request_logs
             WHERE request_body IS NOT NULL
                OR response_body IS NOT NULL
                OR client_response_body IS NOT NULL
             LIMIT 1",
            [],
            |r| r.get(0),
        )?;
        Ok(pending == 1)
    })
    .unwrap_or(false)
        || db
            .with_short(|c| {
                let pending: i64 = c.query_row(
                    "SELECT 1 FROM upstream_attempt_logs
                 WHERE request_body IS NOT NULL OR response_body IS NOT NULL
                 LIMIT 1",
                    [],
                    |r| r.get(0),
                )?;
                Ok(pending == 1)
            })
            .unwrap_or(false)
}

fn run_inline_body_migration(db: &Db) {
    if !has_pending_inline_bodies(db) {
        let _ = db.maintenance_task_record_run(
            TASK_INLINE_BODIES,
            Some(INLINE_BODY_TASK_VERSION),
            None,
            Some("pending=0"),
        );
        return;
    }
    match db.migrate_inline_bodies_to_body_refs(INLINE_BODY_BATCH) {
        Ok(n) if n > 0 => {
            tracing::info!(
                rows = n,
                "inline log bodies moved to filesystem refs (deferred)"
            );
            let _ = db.maintenance_task_record_run(
                TASK_INLINE_BODIES,
                Some(INLINE_BODY_TASK_VERSION),
                None,
                Some(&format!("migrated={n}")),
            );
        }
        Ok(_) => {
            let _ = db.maintenance_task_record_run(
                TASK_INLINE_BODIES,
                Some(INLINE_BODY_TASK_VERSION),
                None,
                Some("migrated=0"),
            );
        }
        Err(e) => tracing::warn!(?e, "inline body migration failed"),
    }
}

fn prune_due(record: Option<GatewayMaintenanceTaskRecord>) -> bool {
    let now = vibe_db::dao::now_secs();
    match record {
        None => true,
        Some(r) => now.saturating_sub(r.last_run_at) >= PRUNE_INTERVAL_SECS,
    }
}

fn run_prune_if_due(db: &Db, observability: Option<&ObservabilityStore>) {
    if prune_due(db.maintenance_task_get(TASK_SHORT_LOG_PRUNE).ok().flatten()) {
        match db.prune_short_logs(&vibe_db::ShortLogRetentionPolicy::default()) {
            Ok(stats) => {
                let result = format!(
                    "req={} attempt={} app={} body_files={}",
                    stats.request_rows_deleted,
                    stats.attempt_rows_deleted,
                    stats.app_rows_deleted,
                    stats.body_files_deleted
                );
                let _ = db.maintenance_task_record_run(
                    TASK_SHORT_LOG_PRUNE,
                    Some(VERSION),
                    None,
                    Some(&result),
                );
                tracing::debug!(%result, "short log prune completed (deferred)");
            }
            Err(e) => tracing::warn!(?e, "short log prune failed"),
        }
    }
    if let Some(obs) = observability {
        if prune_due(db.maintenance_task_get(TASK_OBS_PRUNE).ok().flatten()) {
            match obs.prune(&RetentionPolicy::default()) {
                Ok(stats) => {
                    let result = format!(
                        "req={} attempt={} app={}",
                        stats.request_rows_deleted,
                        stats.attempt_rows_deleted,
                        stats.app_rows_deleted
                    );
                    let _ = db.maintenance_task_record_run(
                        TASK_OBS_PRUNE,
                        Some(VERSION),
                        None,
                        Some(&result),
                    );
                    tracing::debug!(%result, "observability prune completed (deferred)");
                }
                Err(e) => tracing::warn!(?e, "observability prune failed"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_unify_skips_when_stamp_unchanged() {
        let db = Db::memory().expect("db");
        let stamp = "1:100".to_string();
        db.maintenance_task_record_run(TASK_CODEX_UNIFY, Some("0.0.6"), Some(&stamp), Some("ok"))
            .expect("record");
        assert!(!codex_stamp_changed_with(&db, "0.0.6", &stamp));
        assert!(codex_stamp_changed_with(&db, "0.0.7", &stamp));
        assert!(codex_stamp_changed_with(&db, "0.0.6", "2:200"));
    }
}
