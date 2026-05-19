use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct AppLogRecordsQuery {
    limit: Option<i64>,
    since: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RequestRecordsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since: Option<i64>,
    provider_id: Option<String>,
    status_ok: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct NetworkAttemptsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UsageRollupsQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since_day: Option<String>,
    until_day: Option<String>,
    scope: Option<String>,
    provider_id: Option<String>,
    credential_id: Option<String>,
    upstream_id: Option<String>,
}

/// Long-retention daily aggregate stats. Raw logs may be pruned; these rollups should survive.
pub(super) async fn list_usage_rollups(
    State(state): State<AppState>,
    Query(q): Query<UsageRollupsQuery>,
) -> Result<Json<vibe_protocol::UsageRollupPage>, AppError> {
    let limit = q.limit.unwrap_or(500).clamp(1, 2000);
    let offset = q.offset.unwrap_or(0).max(0);
    let page = run_blocking(state, move |s| {
        s.db.usage_rollup_list(
            limit,
            offset,
            q.since_day.as_deref(),
            q.until_day.as_deref(),
            q.scope.as_deref(),
            q.provider_id.as_deref(),
            q.credential_id.as_deref(),
            q.upstream_id.as_deref(),
        )
    })
    .await?;
    Ok(Json(page))
}

/// Application/runtime logs: human-oriented events emitted by Vibe+ itself.
pub(super) async fn list_app_log_records(
    State(state): State<AppState>,
    Query(q): Query<AppLogRecordsQuery>,
) -> Result<Json<Vec<AppLogEvent>>, AppError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 500);
    let since = q.since;
    Ok(Json(state.app_logs.list(limit, since)))
}

/// User request records: one row per inbound client request handled by the gateway.
pub(super) async fn list_request_records(
    State(state): State<AppState>,
    Query(q): Query<RequestRecordsQuery>,
) -> Result<Json<vibe_protocol::LogPage>, AppError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let since = q.since;
    let provider_id = q.provider_id.filter(|v| !v.trim().is_empty());
    let status_ok = q.status_ok;
    let page = run_blocking(state, move |s| {
        if since.is_some() || provider_id.is_some() || status_ok.is_some() {
            s.db.log_list_filtered(limit, offset, since, provider_id.as_deref(), status_ok)
        } else {
            s.db.log_list(limit, offset)
        }
    })
    .await?;
    Ok(Json(page))
}

/// Full user request record, including stored request/response bodies when enabled.
pub(super) async fn get_request_record(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RequestLog>, AppError> {
    let row = run_blocking(state, move |s| s.db.log_get(&id)).await?;
    row.map(Json)
        .ok_or_else(|| anyhow::anyhow!("request record not found").into())
}

/// Network records for one request: every upstream attempt, grouped by wave_index client-side.
pub(super) async fn list_request_network_records(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<UpstreamAttemptLog>>, AppError> {
    let rows = run_blocking(state, move |s| s.db.upstream_attempts_for_request(&id)).await?;
    Ok(Json(rows))
}

/// Network attempt records across all requests, newest first.
pub(super) async fn list_network_attempt_records(
    State(state): State<AppState>,
    Query(q): Query<NetworkAttemptsQuery>,
) -> Result<Json<Vec<UpstreamAttemptLog>>, AppError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = run_blocking(state, move |s| s.db.upstream_attempt_list(limit, offset)).await?;
    Ok(Json(rows))
}
