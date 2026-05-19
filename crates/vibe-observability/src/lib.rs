use std::path::{Path, PathBuf};

use anyhow::Result;
use axum::{
    extract::Path as AxumPath,
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use vibe_db::Db;
use vibe_plugin_api::{EventSink, GatewayEvent, Plugin};
use vibe_protocol::{AppLogEvent, LogPage, RequestLog, UpstreamAttemptLog, UsageRollupPage};

#[derive(Debug, Deserialize)]
pub struct AppLogRecordsQuery {
    limit: Option<i64>,
    since: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RequestRecordsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since: Option<i64>,
    provider_id: Option<String>,
    status_ok: Option<bool>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    trace_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkAttemptsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UsageRollupsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since_day: Option<String>,
    until_day: Option<String>,
    scope: Option<String>,
    provider_id: Option<String>,
    credential_id: Option<String>,
    upstream_id: Option<String>,
    wire: Option<String>,
    route_prefix: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    trace_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Clone)]
pub struct ObservabilityStore {
    db: Db,
}

impl ObservabilityStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db: Db::open_observability(path)?,
        })
    }

    pub fn memory() -> Result<Self> {
        Ok(Self {
            db: Db::observability_memory()?,
        })
    }

    pub fn migrate_from_legacy(&self, legacy_db: &Db) -> Result<()> {
        self.db.copy_observability_from(legacy_db)
    }

    pub fn migrate_from_legacy_path(&self, legacy_db_path: impl AsRef<Path>) -> Result<()> {
        self.db.copy_observability_from_path(legacy_db_path)
    }

    pub fn insert_request(&self, log: &RequestLog) -> Result<()> {
        self.db.log_insert(log)
    }

    pub fn insert_upstream_attempt(&self, attempt: &UpstreamAttemptLog) -> Result<()> {
        self.db.upstream_attempt_insert(attempt)
    }

    pub fn insert_app_log(&self, event: &AppLogEvent) -> Result<()> {
        self.db.app_log_insert(event)
    }

    pub fn request_list(
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
        if since.is_some()
            || provider_id.is_some()
            || status_ok.is_some()
            || thread_id.is_some()
            || turn_id.is_some()
            || trace_id.is_some()
            || session_id.is_some()
        {
            self.db.log_list_filtered(
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
        } else {
            self.db.log_list(limit, offset)
        }
    }

    pub fn request_get(&self, id: &str) -> Result<Option<RequestLog>> {
        self.db.log_get(id)
    }

    pub fn network_for_request(&self, id: &str) -> Result<Vec<UpstreamAttemptLog>> {
        self.db.upstream_attempts_for_request(id)
    }

    pub fn network_attempt_list(&self, limit: i64, offset: i64) -> Result<Vec<UpstreamAttemptLog>> {
        self.db.upstream_attempt_list(limit, offset)
    }

    pub fn app_log_list(&self, limit: i64, since: Option<i64>) -> Result<Vec<AppLogEvent>> {
        self.db.app_log_list(limit, since)
    }

    pub fn prune(
        &self,
        policy: &vibe_db::ShortLogRetentionPolicy,
    ) -> Result<vibe_db::ShortLogPruneStats> {
        self.db.prune_short_logs(policy)
    }

    pub fn usage_rollup_list(
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
        self.db.usage_rollup_list(
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
}

#[derive(Clone)]
pub struct ObservabilityPlugin {
    store: ObservabilityStore,
}

impl ObservabilityPlugin {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            store: ObservabilityStore::open(path)?,
        })
    }

    pub fn from_store(store: ObservabilityStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> ObservabilityStore {
        self.store.clone()
    }
}

impl EventSink for ObservabilityPlugin {
    fn emit(&self, event: GatewayEvent) {
        let store = self.store.clone();
        tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || match event {
                GatewayEvent::RequestFinished(log) => store.insert_request(&log),
                GatewayEvent::UpstreamAttemptFinished(attempt) => {
                    store.insert_upstream_attempt(&attempt)
                }
                GatewayEvent::AppLog(event) => store.insert_app_log(&event),
            })
            .await;
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!(?e, "observability plugin persist failed"),
                Err(e) => tracing::warn!(?e, "observability plugin persist task failed"),
            }
        });
    }
}

impl<StateT> Plugin<StateT> for ObservabilityPlugin
where
    StateT: Clone + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "observability"
    }
}

pub fn default_db_path(vibe_dir: impl AsRef<Path>) -> PathBuf {
    vibe_dir.as_ref().join("observability.db")
}

pub async fn list_request_records(
    State(store): State<ObservabilityStore>,
    Query(q): Query<RequestRecordsQuery>,
) -> Result<Json<LogPage>, ObservabilityHttpError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let provider_id = q.provider_id.filter(|v| !v.trim().is_empty());
    let page = run_blocking(store, move |s| {
        s.request_list(
            limit,
            offset,
            q.since,
            provider_id.as_deref(),
            q.status_ok,
            q.thread_id.as_deref(),
            q.turn_id.as_deref(),
            q.trace_id.as_deref(),
            q.session_id.as_deref(),
        )
    })
    .await?;
    Ok(Json(page))
}

pub async fn get_request_record(
    State(store): State<ObservabilityStore>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<RequestLog>, ObservabilityHttpError> {
    let row = run_blocking(store, move |s| s.request_get(&id)).await?;
    row.map(Json)
        .ok_or_else(|| ObservabilityHttpError::not_found("request record not found"))
}

pub async fn list_request_network_records(
    State(store): State<ObservabilityStore>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Vec<UpstreamAttemptLog>>, ObservabilityHttpError> {
    let rows = run_blocking(store, move |s| s.network_for_request(&id)).await?;
    Ok(Json(rows))
}

pub async fn list_network_attempt_records(
    State(store): State<ObservabilityStore>,
    Query(q): Query<NetworkAttemptsQuery>,
) -> Result<Json<Vec<UpstreamAttemptLog>>, ObservabilityHttpError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = run_blocking(store, move |s| s.network_attempt_list(limit, offset)).await?;
    Ok(Json(rows))
}

pub async fn list_app_log_records(
    State(store): State<ObservabilityStore>,
    Query(q): Query<AppLogRecordsQuery>,
) -> Result<Json<Vec<AppLogEvent>>, ObservabilityHttpError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 500);
    let rows = run_blocking(store, move |s| s.app_log_list(limit, q.since)).await?;
    Ok(Json(rows))
}

pub async fn list_usage_rollups(
    State(store): State<ObservabilityStore>,
    Query(q): Query<UsageRollupsQuery>,
) -> Result<Json<UsageRollupPage>, ObservabilityHttpError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 2000);
    let offset = q.offset.unwrap_or(0).max(0);
    let page = run_blocking(store, move |s| {
        s.usage_rollup_list(
            limit,
            offset,
            q.since_day.as_deref(),
            q.until_day.as_deref(),
            q.scope.as_deref(),
            q.provider_id.as_deref(),
            q.credential_id.as_deref(),
            q.upstream_id.as_deref(),
            q.wire.as_deref(),
            q.route_prefix.as_deref(),
            q.thread_id.as_deref(),
            q.turn_id.as_deref(),
            q.trace_id.as_deref(),
            q.session_id.as_deref(),
        )
    })
    .await?;
    Ok(Json(page))
}

pub fn router() -> Router<ObservabilityStore> {
    Router::new()
        .route("/requests", get(list_request_records))
        .route("/requests/:id", get(get_request_record))
        .route("/requests/:id/network", get(list_request_network_records))
        .route("/network-attempts", get(list_network_attempt_records))
        .route("/app-logs", get(list_app_log_records))
        .route("/usage-rollups", get(list_usage_rollups))
}

#[derive(Debug)]
pub struct ObservabilityHttpError {
    status: StatusCode,
    message: String,
}

impl ObservabilityHttpError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl IntoResponse for ObservabilityHttpError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

async fn run_blocking<T>(
    store: ObservabilityStore,
    f: impl FnOnce(ObservabilityStore) -> Result<T> + Send + 'static,
) -> Result<T, ObservabilityHttpError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || f(store))
        .await
        .map_err(|e| ObservabilityHttpError::internal(e.to_string()))?
        .map_err(|e| ObservabilityHttpError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibe_protocol::{
        RequestLog, UpstreamAttemptLog, UpstreamAttemptOutcome, UpstreamAttemptPhase,
    };

    fn sample_request(id: &str) -> RequestLog {
        RequestLog {
            id: id.to_string(),
            started_at: 1,
            app: None,
            provider_id: Some("p1".into()),
            thread_id: None,
            turn_id: None,
            trace_id: None,
            session_id: None,
            requested_model: Some("m".into()),
            upstream_model: Some("m".into()),
            status_code: Some(200),
            error: None,
            latency_ms: Some(10),
            first_token_ms: None,
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            audio_input_tokens: 0,
            audio_output_tokens: 0,
            accepted_prediction_tokens: 0,
            rejected_prediction_tokens: 0,
            cost_items: None,
            estimated_cost_usd: "0".into(),
            wire: Some("openai-responses".into()),
            route_prefix: None,
            credential_id: None,
            cb_key: None,
            upstream_http_status: Some(200),
            upstream_error_preview: None,
            dedupe_key: None,
            client_transport: None,
            request_headers: None,
            request_body: None,
            response_body: None,
            client_response_body: None,
            stream_kind: None,
            stream_terminal_seen: None,
            stream_end_reason: None,
            stream_error_detail: None,
            upstream_first_byte_ms: None,
            client_first_write_ms: None,
            last_upstream_event_ms: None,
            last_client_write_ms: None,
            upstream_chunk_count: 0,
            upstream_bytes: 0,
            client_chunk_count: 0,
            client_bytes: 0,
            sse_event_count: 0,
            sse_data_count: 0,
            sse_comment_count: 0,
            sse_keepalive_count: 0,
            sse_done_count: 0,
            parse_error_count: 0,
            first_keepalive_ms: None,
            last_keepalive_ms: None,
            max_gap_between_upstream_events_ms: None,
            max_gap_between_data_events_ms: None,
            keepalive_after_last_data_count: 0,
            last_data_event_ms: None,
            bridge_mode: None,
            status_injected: false,
            terminal_injected: false,
            upstream_terminal_type: None,
        }
    }

    fn sample_attempt(id: &str, request_id: &str) -> UpstreamAttemptLog {
        UpstreamAttemptLog {
            attempt_id: id.into(),
            request_id: request_id.into(),
            attempt_index: 0,
            wave_index: 0,
            wave_size: 1,
            upstream_id: None,
            started_at: 1,
            ended_at: Some(2),
            provider_id: Some("p1".into()),
            credential_id: None,
            thread_id: None,
            turn_id: None,
            trace_id: None,
            session_id: None,
            wire: Some("openai-responses".into()),
            route_prefix: None,
            requested_model: Some("m".into()),
            upstream_model: Some("m".into()),
            phase: UpstreamAttemptPhase::Completed,
            outcome: UpstreamAttemptOutcome::Success,
            status_code: Some(200),
            upstream_http_status: Some(200),
            error_summary: None,
            latency_ms: Some(10),
            first_token_ms: None,
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            audio_input_tokens: 0,
            audio_output_tokens: 0,
            accepted_prediction_tokens: 0,
            rejected_prediction_tokens: 0,
            cost_items: None,
            estimated_cost_usd: "0".into(),
            upstream_first_byte_ms: None,
            client_first_write_ms: None,
            last_upstream_event_ms: None,
            last_client_write_ms: None,
            upstream_chunk_count: 0,
            upstream_bytes: 0,
            client_chunk_count: 0,
            client_bytes: 0,
            sse_event_count: 0,
            sse_data_count: 0,
            sse_comment_count: 0,
            sse_keepalive_count: 0,
            sse_done_count: 0,
            parse_error_count: 0,
            first_keepalive_ms: None,
            last_keepalive_ms: None,
            max_gap_between_upstream_events_ms: None,
            max_gap_between_data_events_ms: None,
            keepalive_after_last_data_count: 0,
            last_data_event_ms: None,
            bridge_mode: None,
            status_injected: false,
            terminal_injected: false,
            upstream_terminal_type: None,
            active_upstream_decode_tps_peak: None,
            active_downstream_emit_tps_peak: None,
            request_headers: None,
            request_body: None,
            response_headers: None,
            response_body: None,
        }
    }

    #[test]
    fn store_persists_and_queries_observability_records() {
        let store = ObservabilityStore::memory().unwrap();
        let req = sample_request("r1");
        store.insert_request(&req).unwrap();
        store.insert_request(&req).unwrap();
        store
            .insert_upstream_attempt(&sample_attempt("a1", "r1"))
            .unwrap();
        store
            .insert_upstream_attempt(&sample_attempt("a1", "r1"))
            .unwrap();

        assert_eq!(
            store
                .request_list(10, 0, None, None, None, None, None, None, None)
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(store.network_for_request("r1").unwrap().len(), 1);
        assert_eq!(store.network_attempt_list(10, 0).unwrap().len(), 1);
    }
}
