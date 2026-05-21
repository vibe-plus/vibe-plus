use std::path::Path;

use anyhow::Result;
use vibe_db::Db;
use vibe_protocol::{AppLogEvent, LogPage, RequestLog, UpstreamAttemptLog, UsageRollupPage};

/// SQLite owner for observability data.
///
/// This wrapper is local to `vibe-observability`: the gateway talks to
/// `ObservabilityStore`, and the remaining legacy `vibe_db::Db` delegation can
/// be removed method by method without touching `vibe-core`.
#[derive(Clone)]
pub(crate) struct ObservabilityDb {
    legacy: Db,
}

impl ObservabilityDb {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            legacy: Db::open_observability(path)?,
        })
    }

    pub(crate) fn memory() -> Result<Self> {
        Ok(Self {
            legacy: Db::observability_memory()?,
        })
    }

    pub(crate) fn migrate_from_legacy(&self, legacy_db: &Db) -> Result<()> {
        self.legacy.copy_observability_from(legacy_db)
    }

    pub(crate) fn migrate_from_legacy_path(&self, legacy_db_path: impl AsRef<Path>) -> Result<()> {
        self.legacy.copy_observability_from_path(legacy_db_path)
    }

    pub(crate) fn insert_request(&self, log: &RequestLog) -> Result<()> {
        self.legacy.log_insert(log)
    }

    pub(crate) fn update_request_client_trace(&self, log: &RequestLog) -> Result<()> {
        self.legacy.log_update_client_trace_and_stream_fields(log)
    }

    pub(crate) fn insert_upstream_attempt(&self, attempt: &UpstreamAttemptLog) -> Result<()> {
        self.legacy.upstream_attempt_insert(attempt)
    }

    pub(crate) fn insert_app_log(&self, event: &AppLogEvent) -> Result<()> {
        self.legacy.app_log_insert(event)
    }

    pub(crate) fn request_list(&self, limit: i64, offset: i64) -> Result<LogPage> {
        self.legacy.log_list(limit, offset)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn request_list_filtered(
        &self,
        limit: i64,
        offset: i64,
        since: Option<i64>,
        provider_id: Option<&str>,
        status_ok: Option<bool>,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        trace_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<LogPage> {
        self.legacy.log_list_filtered(
            limit,
            offset,
            since,
            provider_id,
            status_ok,
            thread_id,
            turn_id,
            trace_id,
            session_id,
        )
    }

    pub(crate) fn request_get(&self, id: &str) -> Result<Option<RequestLog>> {
        self.legacy.log_get(id)
    }

    pub(crate) fn upstream_attempts_for_request(
        &self,
        id: &str,
    ) -> Result<Vec<UpstreamAttemptLog>> {
        self.legacy.upstream_attempts_for_request(id)
    }

    pub(crate) fn upstream_attempt_list(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<UpstreamAttemptLog>> {
        self.legacy.upstream_attempt_list(limit, offset)
    }

    pub(crate) fn app_log_list(&self, limit: i64, since: Option<i64>) -> Result<Vec<AppLogEvent>> {
        self.legacy.app_log_list(limit, since)
    }

    pub(crate) fn prune(&self, policy: &RetentionPolicy) -> Result<PruneStats> {
        let legacy = vibe_db::ShortLogRetentionPolicy {
            max_age_secs: policy.max_age_secs,
            max_request_rows: policy.max_request_rows,
            max_app_log_rows: policy.max_app_log_rows,
            max_db_bytes: policy.max_db_bytes,
            body_max_age_secs: policy.body_max_age_secs,
            body_max_files: policy.body_max_files,
            body_max_bytes: policy.body_max_bytes,
        };
        self.legacy.prune_short_logs(&legacy).map(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn usage_rollup_list(
        &self,
        limit: i64,
        offset: i64,
        since_day: Option<&str>,
        until_day: Option<&str>,
        scope: Option<&str>,
        provider_id: Option<&str>,
        credential_id: Option<&str>,
        upstream_id: Option<&str>,
        wire: Option<&str>,
        route_prefix: Option<&str>,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
        trace_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<UsageRollupPage> {
        self.legacy.usage_rollup_list(
            limit,
            offset,
            since_day,
            until_day,
            scope,
            provider_id,
            credential_id,
            upstream_id,
            wire,
            route_prefix,
            thread_id,
            turn_id,
            trace_id,
            session_id,
        )
    }

    pub(crate) fn legacy(&self) -> &Db {
        &self.legacy
    }
}

#[derive(Debug, Clone)]
pub struct RetentionPolicy {
    pub max_age_secs: i64,
    pub max_request_rows: i64,
    pub max_app_log_rows: i64,
    pub max_db_bytes: i64,
    pub body_max_age_secs: i64,
    pub body_max_files: usize,
    pub body_max_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_secs: 14 * 24 * 3600,
            max_request_rows: 50_000,
            max_app_log_rows: 20_000,
            max_db_bytes: 512 * 1024 * 1024,
            body_max_age_secs: 14 * 24 * 3600,
            body_max_files: 100_000,
            body_max_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PruneStats {
    pub request_rows_deleted: i64,
    pub attempt_rows_deleted: i64,
    pub app_rows_deleted: i64,
    pub body_files_deleted: usize,
    pub body_bytes_deleted: u64,
    pub db_bytes: i64,
    pub body_bytes: u64,
}

impl From<vibe_db::ShortLogPruneStats> for PruneStats {
    fn from(stats: vibe_db::ShortLogPruneStats) -> Self {
        Self {
            request_rows_deleted: stats.request_rows_deleted,
            attempt_rows_deleted: stats.attempt_rows_deleted,
            app_rows_deleted: stats.app_rows_deleted,
            body_files_deleted: stats.body_files_deleted,
            body_bytes_deleted: stats.body_bytes_deleted,
            db_bytes: stats.db_bytes,
            body_bytes: stats.body_bytes,
        }
    }
}
