use std::{
    collections::HashMap,
    fs,
    io::BufRead,
    path::{Path, PathBuf},
};

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
use serde_json::Value;
use vibe_db::Db;
use vibe_plugin_api::{EventSink, GatewayEvent, Plugin};
use vibe_protocol::{
    AppLogEvent, CodexThreadMeta, LogPage, ObservabilityConversation,
    ObservabilityConversationSource, ObservabilityConversationStatus, ObservabilityThreadKind,
    RequestLog, UpstreamAttemptLog, UsageRollupPage,
};

mod db;
use db::ObservabilityDb;
pub use db::{PruneStats, RetentionPolicy};

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
pub struct CodexThreadsQuery {
    ids: Option<String>,
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
    db: ObservabilityDb,
}

impl ObservabilityStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            db: ObservabilityDb::open(path)?,
        })
    }

    pub fn memory() -> Result<Self> {
        Ok(Self {
            db: ObservabilityDb::memory()?,
        })
    }

    pub fn migrate_from_legacy(&self, legacy_db: &Db) -> Result<()> {
        self.db.migrate_from_legacy(legacy_db)
    }

    pub fn migrate_from_legacy_path(&self, legacy_db_path: impl AsRef<Path>) -> Result<()> {
        self.db.migrate_from_legacy_path(legacy_db_path)
    }

    pub fn insert_request(&self, log: &RequestLog) -> Result<()> {
        self.db.insert_request(log)
    }

    pub fn update_request_client_trace(&self, log: &RequestLog) -> Result<()> {
        self.db.update_request_client_trace(log)
    }

    pub fn insert_upstream_attempt(&self, attempt: &UpstreamAttemptLog) -> Result<()> {
        self.db.insert_upstream_attempt(attempt)
    }

    pub fn insert_app_log(&self, event: &AppLogEvent) -> Result<()> {
        self.db.insert_app_log(event)
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
            self.db.request_list_filtered(
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
            self.db.request_list(limit, offset)
        }
    }

    pub fn request_get(&self, id: &str) -> Result<Option<RequestLog>> {
        self.db.request_get(id)
    }

    pub fn network_for_request(&self, id: &str) -> Result<Vec<UpstreamAttemptLog>> {
        self.db.upstream_attempts_for_request(id)
    }

    pub fn network_attempt_list(&self, limit: i64, offset: i64) -> Result<Vec<UpstreamAttemptLog>> {
        self.db.upstream_attempt_list(limit, offset)
    }

    pub fn codex_threads(&self, ids: &[String]) -> Result<Vec<CodexThreadMeta>> {
        codex_threads_for_ids(ids)
    }

    pub fn conversation_list(&self) -> Result<Vec<ObservabilityConversation>> {
        let requests = self.db.request_list(i64::MAX / 4, 0)?.items;
        let attempts = self.db.upstream_attempt_list(i64::MAX / 4, 0)?;
        conversations_from_local_history_and_logs(&requests, &attempts)
    }

    pub fn app_log_list(&self, limit: i64, since: Option<i64>) -> Result<Vec<AppLogEvent>> {
        self.db.app_log_list(limit, since)
    }

    pub fn prune(&self, policy: &RetentionPolicy) -> Result<PruneStats> {
        self.db.prune(policy)
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

    pub fn with_legacy_db<R>(&self, f: impl FnOnce(&Db) -> Result<R>) -> Result<R> {
        f(self.db.legacy())
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

pub async fn list_conversation_records(
    State(store): State<ObservabilityStore>,
) -> Result<Json<Vec<ObservabilityConversation>>, ObservabilityHttpError> {
    let rows = run_blocking(store, move |s| s.conversation_list()).await?;
    Ok(Json(rows))
}

pub async fn list_codex_thread_records(
    State(store): State<ObservabilityStore>,
    Query(q): Query<CodexThreadsQuery>,
) -> Result<Json<Vec<CodexThreadMeta>>, ObservabilityHttpError> {
    let ids = q
        .ids
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .take(500)
        .collect::<Vec<_>>();
    let rows = run_blocking(store, move |s| s.codex_threads(&ids)).await?;
    Ok(Json(rows))
}

async fn list_app_log_records(
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
        .route("/conversations", get(list_conversation_records))
        .route("/codex-threads", get(list_codex_thread_records))
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

#[derive(Debug, Clone)]
struct LocalConversationSeed {
    source: ObservabilityConversationSource,
    conversation_id: String,
    title: String,
    project_path: Option<String>,
    project_name: Option<String>,
    updated_at: i64,
    preview: String,
    archived: bool,
    parent_conversation_id: Option<String>,
    thread_kind: ObservabilityThreadKind,
    agent_nickname: Option<String>,
    /// Tokens used per source-app records (no gateway involvement required).
    local_tokens_used: i64,
    /// USD estimated against a built-in price table.
    local_estimated_cost_usd: f64,
    /// Distinct models seen on this conversation.
    models_used: Vec<String>,
    /// Distinct provider IDs from gateway logs (None for local-only chats).
    provider_ids: Vec<String>,
    /// Distinct credential IDs from gateway logs.
    credential_ids: Vec<String>,
    /// Source-app-reported lifespan in seconds.
    duration_seconds: i64,
    /// Earliest source-app timestamp seen for this conversation, used to
    /// derive duration when only per-entry timestamps are available (Claude).
    local_first_seen_at: i64,
}

#[derive(Debug, Clone, Default)]
struct ConversationLogStats {
    request_count: i64,
    attempt_count: i64,
    latest_request_id: Option<String>,
    latest_started_at: i64,
    latest_status_code: Option<i32>,
    active: bool,
    input_tokens: i64,
    output_tokens: i64,
    estimated_cost_usd: f64,
}

fn conversations_from_local_history_and_logs(
    requests: &[RequestLog],
    attempts: &[UpstreamAttemptLog],
) -> Result<Vec<ObservabilityConversation>> {
    let mut by_key: HashMap<(String, String), LocalConversationSeed> = HashMap::new();
    for seed in read_codex_conversation_seeds()? {
        by_key.insert(
            (source_key(&seed.source), seed.conversation_id.clone()),
            seed,
        );
    }
    for seed in read_claude_conversation_seeds()? {
        by_key.insert(
            (source_key(&seed.source), seed.conversation_id.clone()),
            seed,
        );
    }

    let mut stats: HashMap<(String, String), ConversationLogStats> = HashMap::new();
    for request in requests {
        let cost = request.estimated_cost_usd.parse::<f64>().unwrap_or(0.0);
        let model = request
            .upstream_model
            .as_deref()
            .or(request.requested_model.as_deref())
            .map(str::to_string);
        if let Some(thread_id) = request.thread_id.as_deref().filter(|v| !v.is_empty()) {
            let key = ("codex".to_string(), thread_id.to_string());
            let entry = stats.entry(key.clone()).or_default();
            entry.request_count += 1;
            entry.input_tokens += request.input_tokens;
            entry.output_tokens += request.output_tokens;
            entry.estimated_cost_usd += cost;
            if request.started_at >= entry.latest_started_at {
                entry.latest_started_at = request.started_at;
                entry.latest_request_id = Some(request.id.clone());
                entry.latest_status_code = request.status_code;
            }
            let seed = by_key.entry(key).or_insert_with(|| {
                seed_from_request(ObservabilityConversationSource::Codex, thread_id, request)
            });
            record_model(seed, model.as_deref());
            record_provider_credential(seed, request);
        }
        if let Some(session_id) = request.session_id.as_deref().filter(|v| !v.is_empty()) {
            let source = if looks_like_codex_id(session_id) {
                ObservabilityConversationSource::Codex
            } else {
                ObservabilityConversationSource::Claude
            };
            let key = (source_key(&source), session_id.to_string());
            let entry = stats.entry(key.clone()).or_default();
            entry.request_count += 1;
            entry.input_tokens += request.input_tokens;
            entry.output_tokens += request.output_tokens;
            entry.estimated_cost_usd += cost;
            if request.started_at >= entry.latest_started_at {
                entry.latest_started_at = request.started_at;
                entry.latest_request_id = Some(request.id.clone());
                entry.latest_status_code = request.status_code;
            }
            let seed = by_key
                .entry(key)
                .or_insert_with(|| seed_from_request(source, session_id, request));
            record_model(seed, model.as_deref());
            record_provider_credential(seed, request);
        }
    }
    for attempt in attempts {
        if let Some(thread_id) = attempt.thread_id.as_deref().filter(|v| !v.is_empty()) {
            stats
                .entry(("codex".to_string(), thread_id.to_string()))
                .or_default()
                .attempt_count += 1;
        }
        if let Some(session_id) = attempt.session_id.as_deref().filter(|v| !v.is_empty()) {
            let source_key = if looks_like_codex_id(session_id) {
                "codex"
            } else {
                "claude"
            };
            stats
                .entry((source_key.to_string(), session_id.to_string()))
                .or_default()
                .attempt_count += 1;
        }
    }

    let mut out = Vec::new();
    for ((source_slug, id), seed) in by_key {
        let stat = stats.remove(&(source_slug, id)).unwrap_or_default();
        let status = if stat.active {
            ObservabilityConversationStatus::Running
        } else if stat.request_count == 0 {
            ObservabilityConversationStatus::NoData
        } else if stat
            .latest_status_code
            .map(|s| (200..300).contains(&s))
            .unwrap_or(false)
        {
            ObservabilityConversationStatus::Ok
        } else {
            ObservabilityConversationStatus::Failed
        };
        let estimated_cost_usd = if stat.request_count > 0 {
            format!("{:.6}", stat.estimated_cost_usd)
        } else {
            String::new()
        };
        let local_estimated_cost_usd = if seed.local_estimated_cost_usd > 0.0 {
            format!("{:.6}", seed.local_estimated_cost_usd)
        } else {
            String::new()
        };
        // Duration: prefer the seed's existing value (Codex), otherwise span
        // first-seen → updated_at (Claude).
        let duration_seconds = if seed.duration_seconds > 0 {
            seed.duration_seconds
        } else if seed.local_first_seen_at > 0 && seed.updated_at > seed.local_first_seen_at {
            seed.updated_at - seed.local_first_seen_at
        } else {
            0
        };
        out.push(ObservabilityConversation {
            source: seed.source,
            conversation_id: seed.conversation_id,
            title: seed.title,
            project_path: seed.project_path,
            project_name: seed.project_name,
            updated_at: seed.updated_at.max(stat.latest_started_at),
            status,
            request_count: stat.request_count,
            attempt_count: stat.attempt_count,
            latest_request_id: stat.latest_request_id,
            preview: seed.preview,
            estimated_cost_usd,
            input_tokens: stat.input_tokens,
            output_tokens: stat.output_tokens,
            archived: seed.archived,
            parent_conversation_id: seed.parent_conversation_id,
            thread_kind: seed.thread_kind,
            agent_nickname: seed.agent_nickname,
            local_tokens_used: seed.local_tokens_used,
            local_estimated_cost_usd,
            models_used: seed.models_used,
            provider_ids: seed.provider_ids,
            credential_ids: seed.credential_ids,
            duration_seconds,
        });
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

fn seed_from_request(
    source: ObservabilityConversationSource,
    id: &str,
    request: &RequestLog,
) -> LocalConversationSeed {
    LocalConversationSeed {
        source,
        conversation_id: id.to_string(),
        title: request
            .requested_model
            .as_deref()
            .filter(|v| !v.is_empty())
            .unwrap_or(id)
            .to_string(),
        project_path: None,
        project_name: None,
        updated_at: request.started_at,
        preview: request.app.clone().unwrap_or_default(),
        archived: false,
        parent_conversation_id: None,
        thread_kind: ObservabilityThreadKind::User,
        agent_nickname: None,
        local_tokens_used: 0,
        local_estimated_cost_usd: 0.0,
        models_used: Vec::new(),
        provider_ids: Vec::new(),
        credential_ids: Vec::new(),
        duration_seconds: 0,
        local_first_seen_at: 0,
    }
}

fn source_key(source: &ObservabilityConversationSource) -> String {
    match source {
        ObservabilityConversationSource::Codex => "codex".to_string(),
        ObservabilityConversationSource::Claude => "claude".to_string(),
    }
}

fn looks_like_codex_id(id: &str) -> bool {
    id.starts_with("019")
}

fn read_codex_conversation_seeds() -> Result<Vec<LocalConversationSeed>> {
    let path = codex_state_db_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut parent_by_child: HashMap<String, String> = HashMap::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT parent_thread_id, child_thread_id FROM thread_spawn_edges")
    {
        if let Ok(rows) = stmt.query_map([], |r| {
            let p: String = r.get(0)?;
            let c: String = r.get(1)?;
            Ok((p, c))
        }) {
            for entry in rows.flatten() {
                parent_by_child.insert(entry.1, entry.0);
            }
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, title, cwd, source, model, updated_at, updated_at_ms, preview, first_user_message, archived, thread_source, agent_nickname, tokens_used, created_at
         FROM threads ORDER BY updated_at DESC LIMIT 1000",
    )?;
    let rows = stmt.query_map([], |r| {
        let id: String = r.get(0)?;
        let title: String = r.get(1)?;
        let cwd: String = r.get(2)?;
        let model: Option<String> = r.get(4).ok();
        let created_at: i64 = r.get(13).unwrap_or(0);
        let updated_at_ms: Option<i64> = r.get(6)?;
        let updated_at: i64 = updated_at_ms.map(|v| v / 1000).unwrap_or(r.get(5)?);
        let preview: String = r.get::<_, String>(7).or_else(|_| r.get(8))?;
        let archived: i64 = r.get(9).unwrap_or(0);
        let thread_source: Option<String> = r.get(10).ok();
        let agent_nickname: Option<String> = r.get(11).ok();
        let tokens_used: i64 = r.get(12).unwrap_or(0);
        let duration_seconds = (updated_at - created_at).max(0);
        let title = compact_title(&title, &preview);
        let kind = match thread_source.as_deref() {
            Some("subagent") => ObservabilityThreadKind::Subagent,
            _ => ObservabilityThreadKind::User,
        };
        let parent_conversation_id = parent_by_child.get(&id).cloned();
        let model_name = model.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let models_used = match model_name {
            Some(m) => vec![m.to_string()],
            None => Vec::new(),
        };
        // Codex stores `tokens_used` as a total. Without an input/output
        // split, estimate cost by assuming ~30% input / 70% output, which is
        // a defensible average for chat workloads.
        let estimated_cost = if tokens_used > 0 {
            if let Some(model) = model_name {
                let input_share = (tokens_used as f64 * 0.30) as i64;
                let output_share = tokens_used - input_share;
                local_estimate_cost_usd(model, input_share, output_share, 0, 0).unwrap_or(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        Ok(LocalConversationSeed {
            source: ObservabilityConversationSource::Codex,
            conversation_id: id,
            title,
            project_path: (!cwd.trim().is_empty()).then_some(cwd.clone()),
            project_name: (!cwd.trim().is_empty()).then(|| project_name_for_cwd(&cwd)),
            updated_at,
            preview,
            archived: archived != 0,
            parent_conversation_id,
            thread_kind: kind,
            agent_nickname: agent_nickname.filter(|s| !s.is_empty()),
            local_tokens_used: tokens_used.max(0),
            local_estimated_cost_usd: estimated_cost,
            models_used,
            provider_ids: Vec::new(),
            credential_ids: Vec::new(),
            duration_seconds,
            local_first_seen_at: created_at,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn read_claude_conversation_seeds() -> Result<Vec<LocalConversationSeed>> {
    let mut map: HashMap<String, LocalConversationSeed> = HashMap::new();
    read_claude_history_file(&mut map)?;
    read_claude_project_jsonl_files(&mut map)?;
    Ok(map.into_values().collect())
}

fn read_claude_history_file(map: &mut HashMap<String, LocalConversationSeed>) -> Result<()> {
    let path = claude_home_dir().join("history.jsonl");
    if !path.exists() {
        return Ok(());
    }
    let file = fs::File::open(path)?;
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(session_id) = v.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let display = v.get("display").and_then(Value::as_str).unwrap_or("");
        let project = v.get("project").and_then(Value::as_str).map(str::to_string);
        let updated_at = v
            .get("timestamp")
            .and_then(Value::as_i64)
            .map(|v| v / 1000)
            .unwrap_or(0);
        upsert_claude_seed(
            map,
            session_id,
            display,
            None,
            project.as_deref(),
            updated_at,
        );
    }
    Ok(())
}

fn read_claude_project_jsonl_files(map: &mut HashMap<String, LocalConversationSeed>) -> Result<()> {
    let root = claude_home_dir().join("projects");
    if !root.exists() {
        return Ok(());
    }
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            read_claude_project_jsonl_file(map, &path)?;
        }
    }
    Ok(())
}

fn read_claude_project_jsonl_file(
    map: &mut HashMap<String, LocalConversationSeed>,
    path: &Path,
) -> Result<()> {
    let file = fs::File::open(path)?;
    let file_mtime: i64 = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // ai-title/last-prompt entries don't carry sessionId in some shapes; fall
    // back to deriving it from the file stem (Claude names jsonl files after
    // the session UUID).
    let file_session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string);
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let session_id = v
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| file_session_id.clone());
        let Some(session_id) = session_id else {
            continue;
        };
        let cwd = v.get("cwd").and_then(Value::as_str);
        let entry_type = v.get("type").and_then(Value::as_str).unwrap_or("");
        let updated_at = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_seconds)
            .unwrap_or(file_mtime);
        let (title_override, content) = match entry_type {
            "ai-title" => {
                let t = v.get("aiTitle").and_then(Value::as_str).unwrap_or("");
                (Some(t.to_string()), String::new())
            }
            "last-prompt" => {
                let t = v.get("lastPrompt").and_then(Value::as_str).unwrap_or("");
                (None, t.to_string())
            }
            "queue-operation" => {
                let t = v.get("content").and_then(Value::as_str).unwrap_or("");
                (None, t.to_string())
            }
            "user" => (None, claude_user_content(&v).unwrap_or_default()),
            _ => (None, String::new()),
        };
        if title_override.is_some() || !content.is_empty() || cwd.is_some() {
            upsert_claude_seed(
                map,
                &session_id,
                &content,
                title_override.as_deref(),
                cwd,
                updated_at,
            );
        }
        // Pick up local usage data from assistant messages so we can show
        // tokens + estimated cost even when the chat never went through the
        // gateway.
        if entry_type == "assistant" {
            apply_claude_assistant_usage(map, &session_id, &v);
        }
    }
    Ok(())
}

fn apply_claude_assistant_usage(
    map: &mut HashMap<String, LocalConversationSeed>,
    session_id: &str,
    v: &Value,
) {
    let Some(seed) = map.get_mut(session_id) else {
        // Caller hasn't seeded this session yet (first encounter); create a
        // bare seed so we don't lose the usage data.
        map.insert(
            session_id.to_string(),
            LocalConversationSeed {
                source: ObservabilityConversationSource::Claude,
                conversation_id: session_id.to_string(),
                title: session_id.to_string(),
                project_path: None,
                project_name: None,
                updated_at: 0,
                preview: String::new(),
                archived: false,
                parent_conversation_id: None,
                thread_kind: ObservabilityThreadKind::User,
                agent_nickname: None,
                local_tokens_used: 0,
                local_estimated_cost_usd: 0.0,
                models_used: Vec::new(),
                provider_ids: Vec::new(),
                credential_ids: Vec::new(),
                duration_seconds: 0,
                local_first_seen_at: 0,
            },
        );
        return apply_claude_assistant_usage(map, session_id, v);
    };
    let model = v
        .pointer("/message/model")
        .and_then(Value::as_str)
        .map(str::to_string);
    record_model(seed, model.as_deref());
    let Some(usage) = v.pointer("/message/usage") else {
        return;
    };
    let input = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let output = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let cache_creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    seed.local_tokens_used = seed
        .local_tokens_used
        .saturating_add(input + output + cache_read + cache_creation);
    if let Some(m) = model.as_deref() {
        if let Some(cost) =
            local_estimate_cost_usd(m, input, output, cache_read, cache_creation)
        {
            seed.local_estimated_cost_usd += cost;
        }
    }
}

fn claude_user_content(v: &Value) -> Option<String> {
    if v.get("type").and_then(Value::as_str) != Some("user") {
        return None;
    }
    // Skip tool-result echoes that pollute the title.
    if v.get("toolUseResult").is_some() {
        return None;
    }
    let content = v.pointer("/message/content")?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(items) = content.as_array() {
        let parts = items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if !parts.is_empty() {
            return Some(parts.join(" "));
        }
    }
    None
}

fn upsert_claude_seed(
    map: &mut HashMap<String, LocalConversationSeed>,
    session_id: &str,
    text: &str,
    title_override: Option<&str>,
    project_path: Option<&str>,
    updated_at: i64,
) {
    let entry = map
        .entry(session_id.to_string())
        .or_insert_with(|| LocalConversationSeed {
            source: ObservabilityConversationSource::Claude,
            conversation_id: session_id.to_string(),
            title: compact_title(title_override.unwrap_or(text), session_id),
            project_path: project_path.map(str::to_string),
            project_name: project_path.map(project_name_for_cwd),
            updated_at,
            preview: text.to_string(),
            archived: false,
            parent_conversation_id: None,
            thread_kind: ObservabilityThreadKind::User,
            agent_nickname: None,
            local_tokens_used: 0,
            local_estimated_cost_usd: 0.0,
            models_used: Vec::new(),
            provider_ids: Vec::new(),
            credential_ids: Vec::new(),
            duration_seconds: 0,
            local_first_seen_at: updated_at,
        });
    if updated_at >= entry.updated_at {
        entry.updated_at = updated_at;
        if !text.trim().is_empty() {
            entry.preview = text.to_string();
        }
    }
    if updated_at > 0 && (entry.local_first_seen_at == 0 || updated_at < entry.local_first_seen_at)
    {
        entry.local_first_seen_at = updated_at;
    }
    // ai-title is the highest-quality title source: always overwrite.
    if let Some(ai_title) = title_override.filter(|s| !s.trim().is_empty()) {
        entry.title = compact_title(ai_title, ai_title);
    } else if (entry.title == "Untitled thread" || entry.title == entry.conversation_id)
        && !text.trim().is_empty()
    {
        entry.title = compact_title(text, text);
    }
    if entry.project_path.is_none() {
        entry.project_path = project_path.map(str::to_string);
        entry.project_name = project_path.map(project_name_for_cwd);
    }
}

fn claude_home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("CLAUDE_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

fn parse_rfc3339_seconds(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp())
}

fn codex_threads_for_ids(ids: &[String]) -> Result<Vec<CodexThreadMeta>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let path = codex_state_db_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )?;
    let mut out = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, title, cwd, source, model, updated_at, updated_at_ms, preview, first_user_message
         FROM threads WHERE id = ?1",
    )?;
    for id in ids {
        let row = stmt.query_row([id], |r| {
            let cwd: String = r.get(2)?;
            let updated_at_ms: Option<i64> = r.get(6)?;
            let updated_at: i64 = updated_at_ms.map(|v| v / 1000).unwrap_or(r.get(5)?);
            let title: String = r.get(1)?;
            let preview: String = r.get::<_, String>(7).or_else(|_| r.get(8))?;
            Ok(CodexThreadMeta {
                thread_id: r.get(0)?,
                title: compact_title(&title, &preview),
                project: project_name_for_cwd(&cwd),
                cwd,
                source: r.get(3)?,
                model: r.get(4)?,
                updated_at,
                preview,
            })
        });
        match row {
            Ok(meta) => out.push(meta),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(out)
}

fn codex_state_db_path() -> PathBuf {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("state_5.sqlite");
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("state_5.sqlite")
}

fn project_name_for_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

/// Pricing duplicated from [`vibe_core::usage::model_pricing`] to keep this
/// crate free of a circular dependency on vibe-core. Keep in sync.
fn local_model_pricing(model: &str) -> Option<(f64, f64)> {
    let m = model.to_ascii_lowercase();
    let m = if let Some(pos) = m.rfind('/') {
        &m[pos + 1..]
    } else {
        &m
    };
    let pair = if m.starts_with("o1-mini") {
        (3.00, 12.00)
    } else if m.starts_with("o1") {
        (15.00, 60.00)
    } else if m.starts_with("o3-mini") {
        (1.10, 4.40)
    } else if m.starts_with("o3") {
        (10.00, 40.00)
    } else if m.starts_with("o4-mini") {
        (1.10, 4.40)
    } else if m.starts_with("o4") {
        (6.00, 24.00)
    } else if m.starts_with("gpt-4o-mini") {
        (0.15, 0.60)
    } else if m.starts_with("gpt-4o") {
        (2.50, 10.00)
    } else if m.starts_with("gpt-4.1-nano") {
        (0.10, 0.40)
    } else if m.starts_with("gpt-4.1-mini") {
        (0.40, 1.60)
    } else if m.starts_with("gpt-4.1") {
        (2.00, 8.00)
    } else if m.starts_with("gpt-5") {
        (3.00, 12.00)
    } else if m.starts_with("claude-opus-4") {
        (15.00, 75.00)
    } else if m.starts_with("claude-sonnet-4") {
        (3.00, 15.00)
    } else if m.starts_with("claude-haiku-4") {
        (0.80, 4.00)
    } else if m.starts_with("deepseek-r1") || m.contains("deepseek-reasoner") {
        (0.55, 2.19)
    } else if m.starts_with("deepseek-v3") || m.starts_with("deepseek-chat") {
        (0.27, 1.10)
    } else if m.starts_with("kimi-k2") {
        (0.15, 0.60)
    } else if m.starts_with("gemini-2.5-pro") {
        (1.25, 10.00)
    } else if m.starts_with("gemini-2.5-flash") {
        (0.30, 2.50)
    } else if m.starts_with("gemini-2.0") {
        (0.10, 0.40)
    } else if m.starts_with("gemini-1.5-pro") {
        (1.25, 5.00)
    } else if m.starts_with("gemini-1.5-flash") {
        (0.075, 0.30)
    } else if m.starts_with("qwen3") {
        (0.40, 1.60)
    } else if m.starts_with("qwen2") {
        (0.20, 0.60)
    } else {
        return None;
    };
    Some(pair)
}

/// Compute USD cost for `(input, output)` tokens against the local price table.
/// Cache-read tokens are charged at ~10% of input price (rough Anthropic/OpenAI
/// convention) and cache-creation tokens at input price.
fn local_estimate_cost_usd(
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_creation_tokens: i64,
) -> Option<f64> {
    let (input_per_m, output_per_m) = local_model_pricing(model)?;
    let cost = (input_tokens as f64 * input_per_m
        + output_tokens as f64 * output_per_m
        + cache_creation_tokens as f64 * input_per_m
        + cache_read_tokens as f64 * input_per_m * 0.1)
        / 1_000_000.0;
    Some(cost)
}

fn record_model(seed: &mut LocalConversationSeed, model: Option<&str>) {
    let Some(name) = model.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return;
    };
    if !seed.models_used.iter().any(|m| m == name) {
        seed.models_used.push(name.to_string());
    }
}

fn record_provider_credential(seed: &mut LocalConversationSeed, request: &RequestLog) {
    if let Some(p) = request.provider_id.as_deref().filter(|s| !s.is_empty()) {
        if !seed.provider_ids.iter().any(|x| x == p) {
            seed.provider_ids.push(p.to_string());
        }
    }
    if let Some(c) = request.credential_id.as_deref().filter(|s| !s.is_empty()) {
        if !seed.credential_ids.iter().any(|x| x == c) {
            seed.credential_ids.push(c.to_string());
        }
    }
}

fn compact_title(title: &str, fallback: &str) -> String {
    let raw = if title.trim().is_empty() {
        fallback
    } else {
        title
    }
    .trim();
    let first_line = raw
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(raw)
        .trim();
    let mut out = String::new();
    for ch in first_line.chars().take(80) {
        out.push(ch);
    }
    if out.is_empty() {
        "Untitled thread".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use vibe_protocol::{
        RequestLog, UpstreamAttemptLog, UpstreamAttemptOutcome, UpstreamAttemptPhase,
    };

    // Serializes tests that mutate the shared CLAUDE_HOME process-global env var.
    static CLAUDE_HOME_LOCK: Mutex<()> = Mutex::new(());

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
            network_scheme: None,
            network_host: None,
            network_path: None,
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

    #[test]
    fn claude_project_jsonl_picks_up_ai_title() {
        let _guard = CLAUDE_HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let claude_root = tmp.path().join(".claude");
        let projects = claude_root.join("projects").join("proj");
        std::fs::create_dir_all(&projects).unwrap();
        let session_id = "00000000-0000-0000-0000-aaaabbbbcccc";
        let file_path = projects.join(format!("{session_id}.jsonl"));
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-05-20T10:00:00Z","sessionId":"{session_id}","content":"raw first prompt"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"2026-05-20T10:00:01Z","sessionId":"{session_id}","cwd":"/Users/me/proj","message":{{"content":"raw first prompt"}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"ai-title","sessionId":"{session_id}","aiTitle":"Streamline gateway features"}}"#
        )
        .unwrap();
        drop(f);

        std::env::set_var("CLAUDE_HOME", claude_root.to_str().unwrap());
        let seeds = read_claude_conversation_seeds().unwrap();
        std::env::remove_var("CLAUDE_HOME");

        let seed = seeds
            .into_iter()
            .find(|s| s.conversation_id == session_id)
            .expect("session should be discovered");
        assert_eq!(seed.title, "Streamline gateway features");
        assert_eq!(seed.project_path.as_deref(), Some("/Users/me/proj"));
        assert_eq!(seed.project_name.as_deref(), Some("proj"));
    }

    #[test]
    fn conversation_log_stats_include_cost_and_tokens() {
        let mut req = sample_request("r1");
        req.thread_id = Some("019deadbeef".into());
        req.input_tokens = 100;
        req.output_tokens = 50;
        req.estimated_cost_usd = "0.005".into();
        req.upstream_model = Some("gpt-5".into());

        let mut attempt = sample_attempt("a1", "r1");
        attempt.thread_id = Some("019deadbeef".into());

        let out = conversations_from_local_history_and_logs(&[req], &[attempt]).unwrap();
        let conv = out
            .iter()
            .find(|c| c.conversation_id == "019deadbeef")
            .expect("conversation should be present");
        assert_eq!(conv.request_count, 1);
        assert_eq!(conv.attempt_count, 1);
        assert_eq!(conv.input_tokens, 100);
        assert_eq!(conv.output_tokens, 50);
        assert!(conv.estimated_cost_usd.starts_with("0.005"));
        // Model collected from gateway request even without local-history seed.
        assert_eq!(conv.models_used, vec!["gpt-5".to_string()]);
    }

    #[test]
    fn claude_assistant_usage_populates_tokens_and_cost() {
        let _guard = CLAUDE_HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let claude_root = tmp.path().join(".claude");
        let projects = claude_root.join("projects").join("proj");
        std::fs::create_dir_all(&projects).unwrap();
        let session_id = "11111111-2222-3333-4444-555555555555";
        let file_path = projects.join(format!("{session_id}.jsonl"));
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","timestamp":"2026-05-21T10:00:00Z","sessionId":"{session_id}","cwd":"/p","message":{{"content":"hello"}}}}"#
        )
        .unwrap();
        // Two assistant turns with usage data on claude-opus-4.
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"2026-05-21T10:00:01Z","sessionId":"{session_id}","message":{{"model":"claude-opus-4-7","usage":{{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":2000,"cache_creation_input_tokens":3000}}}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"2026-05-21T10:00:02Z","sessionId":"{session_id}","message":{{"model":"claude-opus-4-7","usage":{{"input_tokens":500,"output_tokens":250,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        )
        .unwrap();
        drop(f);

        std::env::set_var("CLAUDE_HOME", claude_root.to_str().unwrap());
        let seeds = read_claude_conversation_seeds().unwrap();
        std::env::remove_var("CLAUDE_HOME");
        let seed = seeds
            .into_iter()
            .find(|s| s.conversation_id == session_id)
            .expect("session present");
        // sum of input+output+cache_read+cache_creation across both turns
        assert_eq!(seed.local_tokens_used, 1000 + 500 + 2000 + 3000 + 500 + 250);
        assert!(seed.local_estimated_cost_usd > 0.0);
        assert_eq!(seed.models_used, vec!["claude-opus-4-7".to_string()]);
    }
}
