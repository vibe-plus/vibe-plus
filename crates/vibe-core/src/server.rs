//! axum HTTP server: routes, handlers, listener.

use crate::circuit_breaker::State as CbState;
use crate::codex_summary;
use crate::codex_upstream_ws::{StatusDecision, UpstreamWsOutcome};
use crate::codex_visual;
use crate::forward;
use crate::forward::{VibeCodexClientKind, VibeCodexVisual};
use crate::providers::Wire;
use crate::state::AppState;
use crate::transforms;
use crate::VERSION;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{DefaultBodyLimit, Path, Query, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use vibe_protocol::{
    AppLogEvent, AppLogLevel, ClientStatus, ClientTakeoverResult, CodexPlanRefreshResult,
    Credential, CredentialInput, CredentialPlanSnapshot, CredentialPoolStatus, DashboardStats,
    Health, HealthSummary, Meta, Provider, ProviderAuthPoolSummary, ProviderCodexPlanItem,
    ProviderHealth, ProviderHealthSummary, ProviderInput, ProviderUpstreamSummary, LocalCandidate,
    ProvidersOverview, RealtimeSnapshot, RequestLog, Status, Upstream, UpstreamAttemptLog,
    UsageSummary,
};

mod clients;
mod config;
mod codex_http;
mod codex_ws;
mod dashboard;
mod fetch;
mod files;
mod import;
mod models;
mod providers;
mod proxy;
mod records;

use clients::*;
use config::*;
use codex_http::*;
use codex_ws::*;
use dashboard::*;
use fetch::*;
use files::*;
use import::*;
use models::*;
use providers::*;
use proxy::*;
use records::*;

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);

    Router::new()
        // Probing "/" / favicon on the API port is normal for browsers; avoid noisy 404s in DevTools.
        .route("/", get(root_discovery))
        .route("/favicon.ico", get(favicon_placeholder))
        // health / status
        .route("/health", get(health))
        .route("/status", get(status))
        .route(
            "/_vp/config/codex",
            get(get_codex_gateway_settings).put(put_codex_gateway_settings),
        )
        .route("/_vp/meta", get(get_meta))
        // Generic model APIs (no tool prefix — for direct / legacy usage)
        .route("/v1/models", get(list_models_all))
        .route("/v1/messages", post(post_messages_plain))
        .route("/v1/chat/completions", post(post_chat_completions_plain))
        .route("/v1/responses", post(post_responses_plain))
        // ── Claude Code tool prefix (/claude/*) ─────────────────────────────
        // ANTHROPIC_BASE_URL = http://127.0.0.1:PORT/claude
        // Claude Code SDK appends /v1/messages, /v1/models, etc.
        .route("/claude/v1/messages", post(post_messages_claude))
        .route("/claude/v1/models", get(list_models_claude))
        // ── Codex tool prefix (/codex/*) ────────────────────────────────────
        // openai_base_url = http://127.0.0.1:PORT/codex/v1
        //
        // Codex CLI uses the WebSocket Responses API as its primary transport.
        // We implement a WS→HTTP bridge: accept the WS upgrade, receive the JSON
        // request body, forward via HTTP to upstream, stream SSE events back as
        // WS text messages.  Chat Completions falls back to plain POST.
        .route("/codex/v1/chat/completions", any(post_or_reject))
        .route("/codex/v1/responses", any(codex_responses_handler))
        .route("/codex/v1/models", get(list_models_openai))
        // ── OpenCode tool prefix (/opencode/*) ──────────────────────────────
        // baseURL = http://127.0.0.1:PORT/opencode/v1
        .route(
            "/opencode/v1/chat/completions",
            post(post_chat_completions_opencode),
        )
        .route("/opencode/v1/responses", post(post_responses_opencode))
        .route("/opencode/v1/models", get(list_models_openai))
        // Gemini Native passthrough — wildcard captures the full model/action path
        .route("/v1beta/models/*path", post(post_gemini))
        // providers CRUD
        .route("/_vp/providers", get(list_providers).post(create_provider))
        .route(
            "/_vp/providers/import-local",
            get(scan_import_local).post(import_local),
        )
        .route("/_vp/providers/overview", get(provider_overview))
        .route("/_vp/clients/:client/status", get(client_status))
        .route("/_vp/clients/:client/doctor", get(client_doctor))
        .route("/_vp/clients/:client/takeover", post(client_takeover))
        .route("/_vp/clients/:client/restore", post(client_restore))
        .route(
            "/_vp/providers/:id",
            put(update_provider).delete(delete_provider),
        )
        .route("/_vp/providers/health", get(provider_health_list))
        .route("/_vp/providers/:id/health", get(provider_health))
        .route("/_vp/providers/:id/pool", get(provider_pool_summary))
        .route("/_vp/pools", get(provider_pool_list))
        .route(
            "/_vp/providers/:id/circuit/reset",
            post(provider_circuit_reset),
        )
        // credentials
        .route("/_vp/credentials", get(list_credentials_all))
        .route(
            "/_vp/providers/:id/credentials",
            get(list_credentials).post(create_credential),
        )
        .route("/_vp/credentials/:id/plan", get(credential_plan_latest))
        .route(
            "/_vp/credentials/:id/plan/refresh",
            post(credential_plan_refresh),
        )
        .route(
            "/_vp/providers/:id/codex-plan",
            get(provider_codex_plan_list),
        )
        .route(
            "/_vp/providers/codex-plan",
            get(provider_codex_plan_list_all),
        )
        .route(
            "/_vp/providers/:id/codex-plan/refresh",
            post(provider_codex_plan_refresh_all),
        )
        .route(
            "/_vp/credentials/:id",
            get(get_credential)
                .put(update_credential)
                .delete(delete_credential),
        )
        .route("/_vp/credentials/:id/enable", post(enable_credential))
        .route("/_vp/credentials/:id/disable", post(disable_credential))
        .route(
            "/_vp/credentials/:id/circuit/reset",
            post(credential_circuit_reset),
        )
        .route(
            "/_vp/credentials/:id/login",
            post(credential_upstream_login),
        )
        .route(
            "/_vp/credentials/:id/groups",
            get(credential_upstream_groups),
        )
        // health overview
        .route("/_vp/health/providers", get(health_all_providers))
        // usage + stats
        .route("/_vp/usage/summary", get(usage_summary))
        .route("/_vp/stats/dashboard", get(dashboard_stats))
        .route("/_vp/realtime", get(realtime_snapshot))
        .route("/_vp/stream/realtime", get(realtime_stream))
        .route("/_vp/app-logs", get(list_app_logs))
        .route("/_vp/logs/app", get(list_app_log_records))
        .route("/_vp/records/requests", get(list_request_records))
        .route("/_vp/records/requests/:id", get(get_request_record))
        .route(
            "/_vp/records/requests/:id/network",
            get(list_request_network_records),
        )
        .route(
            "/_vp/records/network-attempts",
            get(list_network_attempt_records),
        )
        .route("/_vp/stats/usage-rollups", get(list_usage_rollups))
        .route("/_vp/codex-history/preview", get(get_codex_history_preview))
        .route("/_vp/codex-history/unify", post(post_codex_history_unify))
        // sandboxed read/write of ~/.codex and ~/.claude
        .route("/_vp/files/:scope", get(list_files))
        .route(
            "/_vp/files/:scope/file",
            get(read_file).put(write_file).delete(delete_file),
        )
        .route("/_vp/files/:scope/dir", post(create_dir))
        .route("/_vp/files/:scope/move", post(move_file))
        // local fetch proxy for the dashboard (sidesteps browser CORS)
        .route("/_vp/proxy/*path", any(proxy_forward))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        // Model requests can legitimately exceed axum's small 2 MiB default
        // extractor cap, especially Codex requests carrying large thread
        // context. Let Vibe handle routing/logging instead of returning a
        // framework-level 413 before the handler runs.
        .layer(DefaultBodyLimit::disable())
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

async fn root_discovery() -> Json<Value> {
    Json(serde_json::json!({
        "service": "vibe-plus-gateway",
        "health": "/health",
        "status": "/status",
        "meta": "/_vp/meta",
        "control_api": "/_vp/",
        "web_dev": "http://127.0.0.1:15876",
        "note": "The gateway does not host the Web UI; during development run apps/web separately outside this port (see vite.config port).",
    }))
}

async fn get_meta() -> Json<Meta> {
    Json(Meta {
        cli_version: VERSION.to_string(),
        protocol_version: crate::WEB_COMPAT_API,
        min_web_protocol: crate::MIN_WEB_COMPAT_API,
        ui_url: crate::UI_DASHBOARD_URL.to_string(),
    })
}

/// Browsers request /favicon.ico; without a static site, return 204 to avoid console 404s.
async fn favicon_placeholder() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

fn compute_realtime_snapshot(state: &AppState) -> RealtimeSnapshot {
    let transport = state.codex_transport.snapshot();
    let mut snapshot = state.realtime.snapshot(transport);
    if let Ok(providers) = state.db.provider_list() {
        let names: HashMap<String, String> = providers
            .into_iter()
            .map(|provider| (provider.id, provider.name))
            .collect();
        for provider in &mut snapshot.providers {
            if let Some(name) = names.get(&provider.provider_id) {
                provider.provider_name = name.clone();
            }
        }
    }
    snapshot
}

async fn realtime_snapshot(State(state): State<AppState>) -> Json<RealtimeSnapshot> {
    Json(compute_realtime_snapshot(&state))
}

/// SSE stream of `RealtimeSnapshot` frames, ~2 Hz.
///
/// Each connection holds its own interval timer and pulls a fresh snapshot
/// from `state.realtime` on every tick. No event bus, no fan-out hub — when
/// the client disconnects, the stream drops and the timer goes with it.
async fn realtime_stream(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let stream = tokio_stream::wrappers::IntervalStream::new(ticker).map(move |_| {
        let snapshot = compute_realtime_snapshot(&state);
        let payload = serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
        Ok::<_, std::convert::Infallible>(SseEvent::default().event("snapshot").data(payload))
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

async fn compute_status(state: AppState) -> Result<Status, AppError> {
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let codex_transport = state.codex_transport.snapshot();
    Ok(Status {
        version: VERSION.to_string(),
        web_compatibility: vibe_protocol::WebCompatibility {
            api: crate::WEB_COMPAT_API,
            min_web_api: crate::MIN_WEB_COMPAT_API,
        },
        uptime_secs: state.started_at.elapsed().as_secs(),
        port: state.port,
        providers_total: providers.len(),
        providers_enabled: providers.iter().filter(|p| p.enabled).count(),
        requests_last_hour: 0,
        codex_ws_active: codex_transport.ws_active,
        codex_ws_total: codex_transport.ws_total,
        codex_ws_requests_total: codex_transport.ws_requests_total,
        codex_http_responses_total: codex_transport.http_responses_total,
        codex_last_transport: codex_transport.last_transport,
    })
}

async fn status(State(state): State<AppState>) -> Result<Json<Status>, AppError> {
    Ok(Json(compute_status(state).await?))
}

#[cfg(test)]
mod request_body_limit_tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;
    use vibe_protocol::{CredentialVendor, ModelAlias, ProviderInput, ProviderKind};

    #[tokio::test]
    async fn codex_responses_allows_payloads_above_axum_default_body_limit() {
        let db = vibe_db::Db::memory().expect("db");
        db.provider_insert(ProviderInput {
            name: "dummy responses".into(),
            group_name: None,
            avatar_url: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "http://127.0.0.1:9".into(),
            protocols: vec![],
            host: None,
            auth_ref: None,
            enabled: true,
            priority: 100,
            supports_websocket: None,
            passthrough_mode: true,
            model_aliases: vec![ModelAlias {
                alias: "gpt-test".into(),
                upstream_model: "gpt-test".into(),
            }],
        })
        .expect("provider");

        let state = AppState::init(db, crate::config::Config::default(), 0).expect("state");
        let large_input = "x".repeat(2 * 1024 * 1024 + 64 * 1024);
        let body = serde_json::to_vec(&serde_json::json!({
            "model": "gpt-test",
            "input": large_input,
        }))
        .expect("body");

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/codex/v1/responses")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_ne!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn credential_rate_limit_only_counts_unexpired_exhaustion() {
        let now = 1_700_000_000;
        let mut credential = test_credential("cred-a", "provider-a");

        assert!(!credential_is_rate_limited(&credential, now));

        credential.rl_requests_remaining = Some(0);
        credential.rl_requests_reset_at = Some(now + 60);
        assert!(credential_is_rate_limited(&credential, now));

        credential.rl_requests_reset_at = Some(now - 1);
        assert!(!credential_is_rate_limited(&credential, now));

        credential.rl_requests_remaining = Some(10);
        credential.rl_tokens_remaining = Some(0);
        credential.rl_tokens_reset_at = Some(now + 60);
        assert!(credential_is_rate_limited(&credential, now));
    }

    #[test]
    fn provider_pool_summary_counts_availability_and_statuses() {
        let mut config = crate::config::Config::default();
        config.failover.failure_threshold = 1;
        let state = AppState::init(vibe_db::Db::memory().expect("db"), config, 0).expect("state");
        let provider = test_provider("provider-a");

        let mut available = test_credential("cred-available", &provider.id);
        available.enabled = true;
        let mut disabled = test_credential("cred-disabled", &provider.id);
        disabled.enabled = false;
        disabled.last_error = Some("previous auth error".into());
        let mut limited = test_credential("cred-limited", &provider.id);
        limited.rl_requests_remaining = Some(0);
        limited.rl_requests_reset_at = Some(chrono::Utc::now().timestamp() + 60);
        let open = test_credential("cred-open", &provider.id);
        state.cb.force_open(&open.id);

        let summary = build_provider_pool_summary(
            &state,
            &provider,
            vec![
                open.clone(),
                limited.clone(),
                disabled.clone(),
                available.clone(),
            ],
            &[vibe_db::CredentialRollingStat {
                credential_id: available.id.clone(),
                requests: 7,
                successes: 5,
                failures: 2,
                avg_latency_ms: Some(123),
            }],
            &HashMap::new(),
            24,
        );

        assert_eq!(summary.provider_id, provider.id);
        assert_eq!(summary.total_credentials, 4);
        assert_eq!(summary.enabled_credentials, 3);
        assert_eq!(summary.available_credentials, 1);
        assert_eq!(summary.rate_limited_credentials, 1);
        assert_eq!(summary.open_circuit_credentials, 1);
        assert_eq!(summary.provider_circuit_state, "open");
        assert!(summary.provider_circuit_open);
        assert_eq!(
            summary
                .credentials
                .iter()
                .map(|c| c.credential_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "cred-available",
                "cred-disabled",
                "cred-limited",
                "cred-open"
            ]
        );

        let available_status = summary
            .credentials
            .iter()
            .find(|c| c.credential_id == available.id)
            .expect("available status");
        assert_eq!(available_status.rolling_requests, 7);
        assert_eq!(available_status.rolling_successes, 5);
        assert_eq!(available_status.rolling_failures, 2);
        assert_eq!(available_status.rolling_avg_latency_ms, Some(123));

        let limited_status = summary
            .credentials
            .iter()
            .find(|c| c.credential_id == limited.id)
            .expect("limited status");
        assert!(limited_status.is_rate_limited);

        let open_status = summary
            .credentials
            .iter()
            .find(|c| c.credential_id == open.id)
            .expect("open status");
        assert!(open_status.circuit_open);
    }

    fn test_provider(id: &str) -> Provider {
        Provider {
            id: id.into(),
            name: "Provider A".into(),
            group_name: None,
            avatar_url: None,
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://api.openai.com/v1".into(),
            protocols: vec![],
            host: Some("api.openai.com".into()),
            auth_ref: None,
            enabled: true,
            priority: 10,
            supports_websocket: Some(true),
            passthrough_mode: false,
            remote_models: vec![],
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![],
            created_at: 1,
            updated_at: 1,
        }
    }

    fn test_credential(id: &str, provider_id: &str) -> Credential {
        Credential {
            id: id.into(),
            provider_id: provider_id.into(),
            label: id.into(),
            auth_ref: Some("literal:test".into()),
            plan_type: None,
            notes: None,
            enabled: true,
            priority: 1,
            oauth_access_token: None,
            oauth_has_refresh: false,
            oauth_expires_at: None,
            rl_requests_limit: None,
            rl_requests_remaining: None,
            rl_requests_reset_at: None,
            rl_tokens_limit: None,
            rl_tokens_remaining: None,
            rl_tokens_reset_at: None,
            last_used_at: None,
            last_error: None,
            consecutive_failures: 0,
            created_at: 1,
            updated_at: 1,
            auth_fingerprint: None,
            oauth_account_email: None,
            oauth_account_subject: None,
            oauth_chatgpt_plan_slug: None,
            remote_models: vec![],
            remote_models_fetched_at: None,
            balance: None,
            usage: None,
            balance_fetched_at: None,
            upstream_vendor: Some(CredentialVendor::Generic),
            upstream_username: None,
            upstream_has_session: false,
            upstream_session_expires_at: None,
            upstream_group: None,
            price_multiplier: 1.0,
            windows: vec![],
            disabled_reason: None,
            disabled_at: None,
        }
    }
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
