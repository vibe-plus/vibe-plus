//! axum HTTP server: routes, handlers, listener.

use crate::circuit_breaker::State as CbState;
use crate::embedded::Ui;
use crate::forward;
use crate::providers::Wire;
use crate::state::AppState;
use crate::VERSION;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use vibe_protocol::{
    DashboardStats, Health, HealthSummary, LogPage, Provider, ProviderHealth, ProviderInput,
    Status, UsageSummary, WsEvent,
};

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);

    Router::new()
        // health / status
        .route("/health", get(health))
        .route("/status", get(status))
        // model APIs
        .route("/v1/messages", post(post_messages))
        .route("/v1/chat/completions", post(post_chat_completions))
        .route("/v1/responses", post(post_responses))
        // tool-specific path prefixes (strip prefix, forward as OpenAI Chat)
        .route("/codex/v1/chat/completions", post(post_chat_completions))
        .route("/opencode/v1/chat/completions", post(post_chat_completions))
        // Claude Code uses /claude/v1/messages as an Anthropic-compatible endpoint
        .route("/claude/v1/messages", post(post_messages))
        // Gemini Native passthrough — wildcard captures the full model/action path
        .route("/v1beta/models/*path", post(post_gemini))
        // providers CRUD
        .route("/_vp/providers", get(list_providers).post(create_provider))
        .route(
            "/_vp/providers/:id",
            put(update_provider).delete(delete_provider),
        )
        .route("/_vp/providers/:id/health", get(provider_health))
        // health overview
        .route("/_vp/health/providers", get(health_all_providers))
        // logs + usage + stats
        .route("/_vp/logs", get(list_logs))
        .route("/_vp/usage/summary", get(usage_summary))
        .route("/_vp/stats/dashboard", get(dashboard_stats))
        // websocket
        .route("/_vp/ws", any(ws_handler))
        // embedded UI
        .route("/_vp/ui", get(ui_index))
        .route("/_vp/ui/", get(ui_index))
        .route("/_vp/ui/*path", get(ui_asset))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "vibe-core listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Core endpoints
// ---------------------------------------------------------------------------

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

async fn status(State(state): State<AppState>) -> Result<Json<Status>, AppError> {
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let one_hour_ago = chrono::Utc::now().timestamp() - 3600;
    let recent = run_blocking(state.clone(), move |s| s.db.count_logs_since(one_hour_ago)).await?;
    Ok(Json(Status {
        version: VERSION.to_string(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        port: state.port,
        providers_total: providers.len(),
        providers_enabled: providers.iter().filter(|p| p.enabled).count(),
        requests_last_hour: recent,
    }))
}

// ---------------------------------------------------------------------------
// Model API handlers
// ---------------------------------------------------------------------------

async fn post_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::Anthropic, None, headers, body).await
}

async fn post_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::OpenaiChat, None, headers, body).await
}

async fn post_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(state, Wire::OpenaiResponses, None, headers, body).await
}

async fn post_gemini(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let upstream_path = format!("/v1beta/models/{}", path);
    forward::forward(state, Wire::GeminiNative, Some(upstream_path), headers, body).await
}

// ---------------------------------------------------------------------------
// Provider CRUD
// ---------------------------------------------------------------------------

async fn list_providers(State(state): State<AppState>) -> Result<Json<Vec<Provider>>, AppError> {
    let v = run_blocking(state, |s| s.db.provider_list()).await?;
    Ok(Json(v))
}

async fn create_provider(
    State(state): State<AppState>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let p = run_blocking(state.clone(), move |s| s.db.provider_insert(input)).await?;
    state.ws.publish(WsEvent::Hello { version: VERSION.into() });
    Ok(Json(p))
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let p = run_blocking(state, move |s| s.db.provider_update(&id, input)).await?;
    Ok(Json(p))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    run_blocking(state, move |s| s.db.provider_delete(&id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Provider health
// ---------------------------------------------------------------------------

async fn provider_health(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProviderHealth>, AppError> {
    let (db_row, circuit_state, consecutive_failures) = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let row = s.db.health_get(&id2)?;
            Ok(row)
        }
    })
    .await?
    .map(|row| {
        let cs = state.cb.state_of(&id).as_str().to_string();
        let cf = state.cb.consecutive_failures(&id) as i32;
        (row, cs, cf)
    })
    .unwrap_or_else(|| {
        let cs = state.cb.state_of(&id).as_str().to_string();
        let cf = state.cb.consecutive_failures(&id) as i32;
        (
            vibe_db::DbHealth {
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
            },
            cs,
            cf,
        )
    });

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    Ok(Json(ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy: db_row.is_healthy,
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

async fn health_all_providers(
    State(state): State<AppState>,
) -> Result<Json<HealthSummary>, AppError> {
    let rows = run_blocking(state.clone(), |s| s.db.health_list()).await?;
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;

    // Build health entries for every known provider (even those never hit)
    let mut health_map: std::collections::HashMap<String, vibe_db::DbHealth> =
        rows.into_iter().map(|r| (r.provider_id.clone(), r)).collect();

    for p in &providers {
        health_map.entry(p.id.clone()).or_insert_with(|| vibe_db::DbHealth {
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
            let cs = state.cb.state_of(&row.provider_id).as_str().to_string();
            let cf = state.cb.consecutive_failures(&row.provider_id) as i32;
            let is_healthy = cs != CbState::Open.as_str();
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
// Logs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LogQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since: Option<i64>,
    provider_id: Option<String>,
    /// "ok" | "error"
    status: Option<String>,
}

async fn list_logs(
    State(state): State<AppState>,
    Query(q): Query<LogQuery>,
) -> Result<Json<LogPage>, AppError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let status_ok: Option<bool> = match q.status.as_deref() {
        Some("ok") => Some(true),
        Some("error") => Some(false),
        _ => None,
    };
    let p = run_blocking(state, move |s| {
        s.db.log_list_filtered(limit, offset, q.since, q.provider_id.as_deref(), status_ok)
    })
    .await?;
    Ok(Json(p))
}

// ---------------------------------------------------------------------------
// Usage / stats
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UsageQuery {
    hours: Option<i64>,
}

async fn usage_summary(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageSummary>, AppError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 30);
    let s = run_blocking(state, move |s| s.db.usage_summary_last_hours(hours)).await?;
    Ok(Json(s))
}

async fn dashboard_stats(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<DashboardStats>, AppError> {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 30);
    let stats = run_blocking(state, move |s| {
        let requests_last_hour =
            s.db.count_logs_since(chrono::Utc::now().timestamp() - 3600)?;
        let requests_last_24h =
            s.db.count_logs_since(chrono::Utc::now().timestamp() - 86400)?;
        let summary_1h = s.db.usage_summary_last_hours(1)?;
        let (p50, p95) = s.db.latency_percentiles(hours)?;
        let top_models = s.db.top_models(hours, 10)?;
        let per_provider = s.db.per_provider_stats(hours)?;
        let summary_24h = s.db.usage_summary_last_hours(24)?;

        let success_rate_last_hour = {
            let total = summary_1h.requests;
            if total == 0 {
                1.0
            } else {
                // count errors by checking logs (approximate: no error field in summary)
                // use per_provider success rates weighted average
                let total_success: i64 = per_provider.iter().map(|p| p.successes).sum();
                let total_req: i64 = per_provider.iter().map(|p| p.requests).sum();
                if total_req == 0 { 1.0 } else { total_success as f64 / total_req as f64 }
            }
        };

        Ok(vibe_protocol::DashboardStats {
            requests_last_hour,
            requests_last_24h,
            success_rate_last_hour,
            avg_latency_ms: p50,
            p95_latency_ms: p95,
            input_tokens_last_24h: summary_24h.input_tokens,
            output_tokens_last_24h: summary_24h.output_tokens,
            top_models,
            per_provider,
        })
    })
    .await?;
    Ok(Json(stats))
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| ws_session(socket, state))
}

async fn ws_session(socket: WebSocket, state: AppState) {
    let (mut tx, mut rx) = socket.split();
    let mut sub = state.ws.subscribe();
    let hello = WsEvent::Hello { version: VERSION.into() };
    if let Ok(j) = serde_json::to_string(&hello) {
        let _ = tx.send(Message::Text(j)).await;
    }
    loop {
        tokio::select! {
            ev = sub.recv() => {
                let Ok(ev) = ev else { break };
                let Ok(j) = serde_json::to_string(&ev) else { continue };
                if tx.send(Message::Text(j)).await.is_err() { break; }
            }
            incoming = rx.next() => {
                match incoming {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Close(_))) => break,
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Embedded UI
// ---------------------------------------------------------------------------

async fn ui_index() -> Response {
    serve_asset("index.html").await
}

async fn ui_asset(Path(path): Path<String>) -> Response {
    serve_asset(&path).await
}

async fn serve_asset(path: &str) -> Response {
    let candidates = [path, "index.html"];
    for c in candidates {
        if let Some(file) = Ui::get(c) {
            let mime = mime_guess::from_path(c).first_or_octet_stream();
            let mut resp = (StatusCode::OK, file.data.into_owned()).into_response();
            resp.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref()).unwrap(),
            );
            return resp;
        }
    }
    (StatusCode::NOT_FOUND, "asset not found").into_response()
}

// ---------------------------------------------------------------------------
// Error & blocking helpers
// ---------------------------------------------------------------------------

pub struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self.0, "request error");
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}

async fn run_blocking<F, R>(state: AppState, f: F) -> anyhow::Result<R>
where
    F: FnOnce(&AppState) -> anyhow::Result<R> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || f(&state)).await?
}
