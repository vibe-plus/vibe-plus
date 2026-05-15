//! axum HTTP server: routes, handlers, listener.

use crate::circuit_breaker::State as CbState;
use crate::codex_config::{CodexConfigSettings, CodexConfigSettingsInput};
use crate::codex_summary;
use crate::codex_upstream_ws::{StatusDecision, UpstreamWsOutcome};
use crate::codex_visual;
use crate::forward;
use crate::forward::{VibeCodexClientKind, VibeCodexVisual};
use crate::local_import;
use crate::providers::Wire;
use crate::router;
use crate::state::AppState;
use crate::stream_trace::{empty_stream_fields, StreamTraceStats};
use crate::transforms;
use crate::VERSION;
use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{DefaultBodyLimit, Path, Query, State, WebSocketUpgrade};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use vibe_protocol::{
    AppLogEvent, AppLogLevel, ClientStatus, ClientTakeoverResult, CodexAppActionResult,
    CodexAppStatus, CodexPlanRefreshResult, Credential, CredentialInput,
    CredentialPlanSnapshot, CredentialPoolStatus, DashboardStats, Health, HealthSummary, LogPage,
    Meta, Provider, ProviderAuthPoolSummary, ProviderBalanceSnapshot, ProviderCodexPlanItem,
    ProviderHealth, ProviderHealthSummary, ProviderInput, ProviderSpeedtestInput,
    ProviderSpeedtestResult, ProvidersOverview, ProvidersOverviewCodexPlansChunk,
    ProvidersOverviewCredentialsChunk, ProvidersOverviewHealthChunk, ProvidersOverviewPoolsChunk,
    ProvidersOverviewProvidersChunk, ProvidersOverviewStreamEnded, ProvidersOverviewStreamStarted,
    Status, UpstreamAttemptLog, UsageSummary, WsEvent,
};
#[cfg(target_os = "macos")]
use vibe_protocol::CodexAppProcess;

#[derive(Debug, Deserialize)]
struct ProviderSyncInput {
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ProviderSyncPreview {
    provider: Provider,
    display_name: String,
    avatar_url: Option<String>,
    balance: Option<ProviderBalanceSnapshot>,
    usage: Option<ProviderBalanceSnapshot>,
    supported_protocols: Vec<String>,
    platform_guess: Option<String>,
    note: String,
}

#[derive(Debug, Clone, Default)]
struct SyncBranding {
    display_name: Option<String>,
    avatar_url: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SyncFinancials {
    balance: Option<ProviderBalanceSnapshot>,
    usage: Option<ProviderBalanceSnapshot>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum WsClientMessage {
    Snapshot {
        request_id: Option<String>,
        topic: String,
        hours: Option<i64>,
        client: Option<String>,
    },
}

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
        .route("/_vp/meta", get(get_meta))
        .route("/_vp/config", get(get_config).put(put_config))
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
        .route("/codex/v1/responses/compact", any(codex_responses_handler))
        // Codex sometimes sends double-prefix paths (openai_base_url already has /v1)
        .route("/codex/v1/v1/responses", any(codex_responses_handler))
        .route("/codex/v1/v1/chat/completions", any(post_or_reject))
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
        // providers CRUD + local import
        .route("/_vp/providers", get(list_providers).post(create_provider))
        .route(
            "/_vp/providers/import-local",
            get(scan_local_providers).post(import_local_providers),
        )
        .route("/_vp/providers/import-ccs", post(import_ccs_profile_bundle))
        .route(
            "/_vp/providers/import-ccswitch",
            post(import_cc_switch_deeplink),
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
        .route("/_vp/providers/:id/test", post(provider_test))
        .route("/_vp/providers/:id/probe", post(provider_probe))
        .route("/_vp/providers/:id/speedtest", post(provider_speedtest))
        .route("/_vp/providers/:id/sync", post(provider_sync))
        .route("/_vp/providers/:id/pool", get(provider_pool_summary))
        .route(
            "/_vp/providers/:id/models/refresh",
            post(refresh_provider_models),
        )
        .route("/_vp/pools", get(provider_pool_list))
        .route("/_vp/routes", get(list_routes).post(create_route))
        .route("/_vp/routes/explain", get(explain_route))
        .route("/_vp/routes/:id", put(update_route).delete(delete_route))
        .route(
            "/_vp/providers/:id/circuit/reset",
            post(provider_circuit_reset),
        )
        // Smart intake: clipboard/paste detects credential candidates -> concurrently probe all providers -> one-click persistence
        .route("/_vp/intake/probe", post(crate::intake::probe_handler))
        .route("/_vp/intake/import", post(crate::intake::import_handler))
        .route(
            "/_vp/intake/preview-remote",
            post(crate::intake::remote_preview_handler),
        )
        .route(
            "/_vp/intake/import-remote",
            post(crate::intake::remote_import_handler),
        )
        .route(
            "/_vp/intake/vendor-probe",
            post(crate::intake::vendor_probe_handler),
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
        // health overview
        .route("/_vp/health/providers", get(health_all_providers))
        // logs + usage + stats
        .route("/_vp/logs/:id", get(get_request_log))
        .route("/_vp/logs/:id/attempts", get(list_request_attempts))
        .route("/_vp/logs/:id/stream-trace", get(get_log_stream_trace))
        .route("/_vp/logs", get(list_logs))
        .route("/_vp/attempts/:id", get(get_upstream_attempt))
        .route("/_vp/attempts", get(list_upstream_attempts))
        .route("/_vp/usage/summary", get(usage_summary))
        .route("/_vp/stats/dashboard", get(dashboard_stats))
        .route(
            "/_vp/tool-configs/:tool/raw",
            get(get_tool_config_raw).put(put_tool_config_raw),
        )
        .route(
            "/_vp/tool-configs/codex/settings",
            get(get_codex_config_settings).put(put_codex_config_settings),
        )
        .route("/_vp/codex-history/preview", get(get_codex_history_preview))
        .route("/_vp/codex-history/unify", post(post_codex_history_unify))
        .route("/_vp/codex-app/status", get(get_codex_app_status))
        .route("/_vp/codex-app/open", post(post_codex_app_open))
        .route("/_vp/codex-app/quit", post(post_codex_app_quit))
        .route("/_vp/codex-app/restart", post(post_codex_app_restart))
        .route("/_vp/codex-files", get(list_codex_files))
        .route(
            "/_vp/codex-files/file",
            get(read_codex_file)
                .put(write_codex_file)
                .delete(delete_codex_file),
        )
        .route("/_vp/codex-files/dir", post(create_codex_dir))
        .route("/_vp/codex-files/move", post(move_codex_file))
        .route("/_vp/app-logs", get(list_app_logs))
        // websocket
        .route("/_vp/ws", any(ws_handler))
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
        "websocket": "/_vp/ws",
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
        ui_url: crate::UI_BASE_URL.to_string(),
    })
}

/// Browsers request /favicon.ico; without a static site, return 204 to avoid console 404s.
async fn favicon_placeholder() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

async fn compute_status(state: AppState) -> Result<Status, AppError> {
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let one_hour_ago = chrono::Utc::now().timestamp() - 3600;
    let recent = run_blocking(state.clone(), move |s| s.db.count_logs_since(one_hour_ago)).await?;
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
        requests_last_hour: recent,
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

async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<crate::config::Config>, AppError> {
    let path = crate::paths::config_path()?;
    let cfg =
        tokio::task::spawn_blocking(move || crate::config::Config::load_or_init(&path)).await??;
    state.set_codex_summary_config(cfg.codex.summary.clone());
    state.set_codex_route_status_enabled(cfg.codex.route_status_enabled);
    state.set_claude_config(cfg.claude.clone());
    Ok(Json(cfg))
}

async fn put_config(
    State(state): State<AppState>,
    Json(input): Json<crate::config::Config>,
) -> Result<Json<crate::config::Config>, AppError> {
    let path = crate::paths::config_path()?;
    let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<crate::config::Config> {
        input.save(&path)?;
        crate::config::Config::load_or_init(&path)
    })
    .await??;
    let current = (*state.config).clone();
    if current.server.port != saved.server.port || current.server.host != saved.server.host {
        tracing::warn!(
            "config server address changed on disk; restart vibe for server host/port changes"
        );
    }
    state.set_codex_summary_config(saved.codex.summary.clone());
    state.set_codex_route_status_enabled(saved.codex.route_status_enabled);
    state.set_claude_config(saved.claude.clone());
    Ok(Json(saved))
}

async fn list_routes(
    State(state): State<AppState>,
) -> Result<Json<Vec<vibe_protocol::Route>>, AppError> {
    let routes = run_blocking(state, |s| s.db.route_list()).await?;
    Ok(Json(routes))
}

async fn create_route(
    State(state): State<AppState>,
    Json(input): Json<vibe_protocol::RouteInput>,
) -> Result<Json<vibe_protocol::Route>, AppError> {
    let route = run_blocking(state.clone(), move |s| s.db.route_insert(input)).await?;
    publish_routes_changed_soon(state);
    Ok(Json(route))
}

async fn update_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<vibe_protocol::RouteInput>,
) -> Result<Json<vibe_protocol::Route>, AppError> {
    let route = run_blocking(state.clone(), move |s| s.db.route_update(&id, input)).await?;
    publish_routes_changed_soon(state);
    Ok(Json(route))
}

async fn delete_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    run_blocking(state.clone(), move |s| s.db.route_delete(&id)).await?;
    publish_routes_changed_soon(state);
    Ok(StatusCode::NO_CONTENT)
}

fn publish_routes_changed_soon(state: AppState) {
    tokio::spawn(async move {
        match run_blocking(state.clone(), |s| s.db.route_list()).await {
            Ok(routes) => state.ws.publish(WsEvent::RoutesChanged { routes }),
            Err(e) => tracing::warn!(?e, "build routes ws event failed"),
        }
    });
}

#[derive(Debug, Deserialize)]
struct RouteExplainQuery {
    model: String,
    wire: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct RouteExplainPick {
    provider_id: String,
    provider_name: String,
    provider_kind: String,
    upstream_model: String,
    priority: i32,
}

#[derive(Debug, serde::Serialize)]
struct RouteExplainResponse {
    requested_model: String,
    wire: String,
    matched_route: Option<vibe_protocol::Route>,
    candidates: Vec<RouteExplainPick>,
}

async fn explain_route(
    State(state): State<AppState>,
    Query(q): Query<RouteExplainQuery>,
) -> Result<Json<RouteExplainResponse>, AppError> {
    let wire = wire_from_str(q.wire.as_deref().unwrap_or("openai-responses"))?;
    let (providers, routes) = run_blocking(state, |s| {
        Ok::<_, anyhow::Error>((s.db.provider_list()?, s.db.route_list()?))
    })
    .await?;
    let (matched_route, candidates) =
        router::candidates_with_routes(&providers, &routes, wire, &q.model);
    Ok(Json(RouteExplainResponse {
        requested_model: q.model,
        wire: wire_name(wire).into(),
        matched_route,
        candidates: candidates
            .into_iter()
            .map(|p| RouteExplainPick {
                provider_id: p.provider.id,
                provider_name: p.provider.name,
                provider_kind: format!("{:?}", p.provider.kind),
                upstream_model: p.upstream_model,
                priority: p.provider.priority,
            })
            .collect(),
    }))
}

fn wire_from_str(s: &str) -> Result<Wire, AppError> {
    Ok(match s {
        "anthropic" => Wire::Anthropic,
        "openai-chat" => Wire::OpenaiChat,
        "openai-responses" => Wire::OpenaiResponses,
        "gemini-native" => Wire::GeminiNative,
        other => {
            return Err(anyhow::anyhow!(
                "unknown wire {other}; expected anthropic, openai-chat, openai-responses, gemini-native"
            )
            .into())
        }
    })
}

fn wire_name(wire: Wire) -> &'static str {
    match wire {
        Wire::Anthropic => "anthropic",
        Wire::OpenaiChat => "openai-chat",
        Wire::OpenaiResponses => "openai-responses",
        Wire::GeminiNative => "gemini-native",
    }
}

// ---------------------------------------------------------------------------
// Model discovery
// ---------------------------------------------------------------------------

/// `/v1/models` — all enabled aliases in OpenAI format (fallback / generic clients)
async fn list_models_all(State(state): State<AppState>) -> Response {
    model_list_openai(&state, None).await
}

/// `/claude/v1/models` — Anthropic providers only, in Anthropic SDK format
/// Claude Code Anthropic SDK expects `{data:[...], has_more, first_id, last_id}`
async fn list_models_claude(State(state): State<AppState>) -> Response {
    let providers = match state.db.provider_list() {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for p in providers
        .iter()
        .filter(|p| p.enabled && p.kind == vibe_protocol::ProviderKind::Anthropic)
    {
        let names: Vec<String> = if !p.remote_models.is_empty() {
            p.remote_models.clone()
        } else {
            p.model_aliases.iter().map(|a| a.alias.clone()).collect()
        };
        for name in names {
            if seen.insert(name.clone()) {
                data.push(serde_json::json!({
                    "id": name,
                    "display_name": name,
                    "type": "model",
                    "created_at": "2025-01-01T00:00:00Z"
                }));
            }
        }
    }
    data.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or(""))
    });

    let first = data
        .first()
        .and_then(|m| m["id"].as_str())
        .map(String::from);
    let last = data.last().and_then(|m| m["id"].as_str()).map(String::from);

    Json(serde_json::json!({
        "data": data,
        "has_more": false,
        "first_id": first,
        "last_id": last
    }))
    .into_response()
}

/// `/codex/v1/models` and `/opencode/v1/models`
/// OpenAI-compatible / OpenAI-Responses providers only, in OpenAI format
async fn list_models_openai(State(state): State<AppState>) -> Response {
    use vibe_protocol::ProviderKind;
    model_list_openai(
        &state,
        Some(&[ProviderKind::OpenaiChat, ProviderKind::OpenaiResponses]),
    )
    .await
}

async fn model_list_openai(
    state: &AppState,
    kinds: Option<&[vibe_protocol::ProviderKind]>,
) -> Response {
    let providers = match state.db.provider_list() {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();

    for p in providers
        .iter()
        .filter(|p| p.enabled && kinds.map_or(true, |ks| ks.contains(&p.kind)))
    {
        let names: Vec<String> = if !p.remote_models.is_empty() {
            p.remote_models.clone()
        } else {
            p.model_aliases.iter().map(|a| a.alias.clone()).collect()
        };
        for name in names {
            if seen.insert(name.clone()) {
                data.push(serde_json::json!({
                    "id": name,
                    "slug": name,                    // Codex v0.129+
                    "display_name": name,            // Codex v0.129+
                    "supported_reasoning_levels": [],       // Codex v0.129+
                    "shell_type": "default",                // Codex v0.129+ (enum: default|local|unified_exec|disabled|shell_command)
                    "visibility": "list",                   // Codex v0.129+ (enum: list|hide|none)
                    "supported_in_api": true,               // Codex v0.129+
                    "priority": 0,                          // Codex v0.129+
                    "base_instructions": "",                // Codex v0.129+ (must be string, not null)
                    "supports_reasoning_summaries": false,  // Codex v0.129+
                    "support_verbosity": false,             // Codex v0.129+
                    // Align with codex-rs ModelFamily conservative defaults.
                    "truncation_policy": {"mode": "bytes", "limit": 10000},
                    "supports_parallel_tool_calls": false,
                    "experimental_supported_tools": [],
                    "object": "model",
                    "created": 0,
                    "owned_by": "vibe-plus"
                }));
            }
        }
    }
    data.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b["id"].as_str().unwrap_or(""))
    });

    // Codex v0.129+ expects a top-level "models" field that is an array of ModelInfo
    // objects (same structure as "data"), not a plain string array.
    Json(serde_json::json!({
        "object": "list",
        "data": data,
        "models": data           // Codex v0.129+ compatibility: same objects as data
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// Model API handlers
// ---------------------------------------------------------------------------

async fn post_messages_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::Anthropic,
        None,
        headers,
        body,
        Some("plain-v1".into()),
    )
    .await
}

async fn post_messages_claude(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::Anthropic,
        None,
        headers,
        body,
        Some("claude-v1".into()),
    )
    .await
}

async fn post_chat_completions_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiChat,
        None,
        headers,
        body,
        Some("plain-v1".into()),
    )
    .await
}

async fn post_chat_completions_opencode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiChat,
        None,
        headers,
        body,
        Some("opencode-v1".into()),
    )
    .await
}

async fn post_responses_plain(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiResponses,
        None,
        headers,
        body,
        Some("plain-v1".into()),
    )
    .await
}

async fn post_responses_opencode(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward::forward(
        state,
        Wire::OpenaiResponses,
        None,
        headers,
        body,
        Some("opencode-v1".into()),
    )
    .await
}

// ---------------------------------------------------------------------------
// Codex WebSocket + HTTP handler for /responses
//
// Codex CLI uses WebSocket as primary transport for /v1/responses:
//   1. Client sends one JSON message  = the HTTP request body
//   2. Server streams back SSE-style events as individual WS text messages
//   3. Server closes the socket when the response is complete
//
// For plain HTTP POST we still forward via `forward`, then may translate upstream Chat SSE
// into Responses-shaped SSE frames for Codex HTTP clients (`C2R`).
// ---------------------------------------------------------------------------

fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

/// Unified handler for /codex/v1/responses (and compact/double-prefix variants).
/// Accepts both WS upgrades and plain HTTP POST.
async fn codex_responses_handler(
    ws_upgrade: Option<WebSocketUpgrade>,
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(upgrade) = ws_upgrade {
        state.codex_transport.ws_opened();
        let mut ws_headers = headers.clone();
        ws_headers.insert(
            HeaderName::from_static("x-vibe-client-transport"),
            HeaderValue::from_static("ws"),
        );
        upgrade.on_upgrade(move |socket| codex_ws_bridge(socket, state, ws_headers))
    } else {
        // Plain HTTP POST — Codex may send the WS-envelope format even over HTTP.
        // Strip the {"type":"response.create",...} envelope so forward() sees a
        // clean Responses API body with a top-level "model" field.
        let streaming = request_body_streams(&body);
        let client_transport = if streaming { "http-sse" } else { "http" };
        state.codex_transport.http_response_request(streaming);
        let mut headers = headers;
        headers.insert(
            HeaderName::from_static("x-vibe-client-transport"),
            HeaderValue::from_static(client_transport),
        );
        let stripped = transforms::strip_ws_envelope(&body);
        let should_show_status = transforms::responses_input_ends_with_user_message(&stripped);
        let turn_id = codex_summary::turn_id_from_request(&body)
            .or_else(|| codex_summary::turn_id_from_request(&stripped));
        let request_started_instant = Instant::now();
        let upstream = forward::forward(
            state.clone(),
            Wire::OpenaiResponses,
            None,
            headers,
            stripped,
            Some("codex-v1".into()),
        )
        .await;
        codex_plain_http_maybe_chat_to_responses_sse(
            state,
            upstream,
            request_started_instant,
            should_show_status,
            turn_id,
        )
        .await
    }
}

fn request_body_streams(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/stream")
                .or_else(|| v.pointer("/response/stream"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CodexHttpSseMode {
    Undecided,
    Passthrough,
    C2r,
}

#[derive(Clone, Debug)]
struct CodexStatusInjection {
    visual: codex_visual::CodexVisualContext,
    ttfs_ms: i64,
    emitted: bool,
    suppress_status: bool,
}

impl CodexStatusInjection {
    fn new(
        visual: Option<codex_visual::CodexVisualContext>,
        ttfs_ms: i64,
        suppress_status: bool,
    ) -> Option<Self> {
        visual.map(|visual| Self {
            visual,
            ttfs_ms,
            emitted: false,
            suppress_status,
        })
    }

    fn next_frames(&mut self, response_id: &str) -> Vec<String> {
        if self.emitted {
            return Vec::new();
        }
        self.emitted = true;
        let mut frames = Vec::new();
        if let Some(event) = codex_visual::coding_plan_rate_limit_event(&self.visual) {
            frames.push(event);
        }
        if !self.suppress_status {
            frames.extend(codex_visual::status_message_events(
                &self.visual,
                response_id,
                self.ttfs_ms,
            ));
        }
        frames
    }
}

fn codex_status_dedupe_key(
    turn_id: Option<&str>,
    visual: &codex_visual::CodexVisualContext,
) -> String {
    format!(
        "{}|{}",
        turn_id.unwrap_or("__unknown_turn__"),
        codex_visual::route_signature(visual)
    )
}

fn codex_status_ttl(turn_id: Option<&str>) -> Duration {
    if turn_id.is_some() {
        Duration::from_secs(30 * 60)
    } else {
        Duration::from_secs(90)
    }
}

fn should_emit_codex_route_status(
    state: &AppState,
    turn_id: Option<&str>,
    visual: &codex_visual::CodexVisualContext,
) -> bool {
    state.remember_codex_status_key(
        codex_status_dedupe_key(turn_id, visual),
        codex_status_ttl(turn_id),
    )
}

#[cfg(test)]
mod codex_status_tests {
    use super::*;

    fn visual(upstream_model: &str) -> codex_visual::CodexVisualContext {
        codex_visual::CodexVisualContext {
            provider_id: "p1".into(),
            credential_id: Some("cred-1".into()),
            requested_model: "gpt-5.5".into(),
            upstream_model: upstream_model.into(),
            ..Default::default()
        }
    }

    #[test]
    fn extracts_turn_id_from_ws_envelope_client_metadata() {
        let body = serde_json::json!({
            "type": "response.create",
            "response": {
                "model": "gpt-5.5",
                "client_metadata": {
                    "x-codex-turn-metadata": "{\"turn_id\":\"turn-123\",\"turn_started_at_unix_ms\":1}"
                }
            }
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        assert_eq!(
            codex_summary::turn_id_from_request(&bytes).as_deref(),
            Some("turn-123")
        );
    }

    #[test]
    fn dedupe_key_changes_when_route_changes() {
        let first = codex_status_dedupe_key(Some("turn-1"), &visual("gpt-5.5"));
        let same = codex_status_dedupe_key(Some("turn-1"), &visual("gpt-5.5"));
        let changed = codex_status_dedupe_key(Some("turn-1"), &visual("kimi-k2"));
        assert_eq!(first, same);
        assert_ne!(first, changed);
    }

    #[test]
    fn unknown_turn_uses_short_ttl_fallback() {
        assert!(codex_status_ttl(None) < codex_status_ttl(Some("turn-1")));
    }
}

fn codex_frame_is_response_created(frame_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(frame_json)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
        .is_some_and(|t| t == "response.created")
}

fn codex_sse_block_has_response_created(block: &str) -> bool {
    block.lines().any(|raw_line| {
        let line = raw_line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            return false;
        };
        codex_frame_is_response_created(payload.trim())
    })
}

/// Extract `response.id` from a `response.created` frame JSON, so that injected
/// status events use the upstream's actual response_id in Passthrough mode.
fn extract_response_created_id(frame_json: &str) -> Option<String> {
    let v = serde_json::from_str::<serde_json::Value>(frame_json).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("response.created") {
        return None;
    }
    v.pointer("/response/id")
        .and_then(|id| id.as_str())
        .map(str::to_string)
}

fn codex_sse_block_extract_response_id(block: &str) -> Option<String> {
    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        if let Some(id) = extract_response_created_id(payload.trim()) {
            return Some(id);
        }
    }
    None
}

/// Inspect one SSE frame (delimiter `\n\n`) and decide whether it looks like **Chat Completions** JSON.
///
/// Returns:
/// - `Some(true)`  — contains `choices` (typical upstream Chat SSE)
/// - `Some(false)` — contains structured JSON without `choices` (likely Responses-native)
/// - `None`        — heartbeat / comments / `[DONE]` / empty — stay undecided
fn classify_codex_upstream_sse_frame(block: &str) -> Option<bool> {
    let mut saw_data = false;
    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        saw_data = true;
        let d = payload.trim();
        if d.is_empty() || d == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(d) else {
            return Some(false);
        };
        if v.get("choices").is_some() {
            return Some(true);
        }
        return Some(false);
    }
    if saw_data {
        Some(false)
    } else {
        None
    }
}

/// Codex **`/codex/v1/responses` HTTP**: if upstream is Chat SSE, convert **SSE -> Responses SSE** here and write `request_logs.client_response_body`.
async fn codex_plain_http_maybe_chat_to_responses_sse(
    state: AppState,
    upstream: Response,
    request_started_instant: Instant,
    should_show_status: bool,
    summary_turn_id: Option<String>,
) -> Response {
    let (parts, body) = upstream.into_parts();
    let log_row_id = parts
        .extensions
        .get::<forward::VibeLogId>()
        .map(|x| x.0.clone());
    let visual = parts
        .extensions
        .get::<VibeCodexVisual>()
        .map(|x| x.0.clone());
    let codex_client_kind = parts
        .extensions
        .get::<VibeCodexClientKind>()
        .map(|x| x.0)
        .unwrap_or(codex_summary::CodexClientKind::Unknown);

    if !parts.status.is_success() {
        return Response::from_parts(parts, body);
    }

    let content_type_hdr = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !content_type_hdr.contains("event-stream") {
        return Response::from_parts(parts, body);
    }

    let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
    let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());

    let mut out_headers = HeaderMap::new();
    for (name, value) in parts.headers.iter() {
        let n = name.as_str();
        if n.eq_ignore_ascii_case("content-length")
            || n.eq_ignore_ascii_case("transfer-encoding")
            || n.eq_ignore_ascii_case("content-type")
        {
            continue;
        }
        out_headers.insert(name.clone(), value.clone());
    }
    out_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );

    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(96);
    tokio::spawn(async move {
        let mut trace = String::new();
        let mut mode = CodexHttpSseMode::Undecided;
        let mut status_injection: Option<CodexStatusInjection> = None;
        let mut summary_injection = codex_summary::SummaryAccumulator::new_for_turn(
            state.codex_summary_config(),
            codex_client_kind,
            Some(state.clone()),
            summary_turn_id,
        );
        let mut trace_stats = StreamTraceStats::new("sse", "chat_to_responses");

        #[inline]
        async fn emit_raw_frame(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            trace_stats: &mut StreamTraceStats,
            started: &Instant,
            block: &str,
        ) -> bool {
            let mut chunk = block.to_owned();
            chunk.push_str("\n\n");
            append_codex_ws_client_trace(trace, chunk.trim_end());
            let bytes = chunk.len();
            if tx.send(Ok(Bytes::from(chunk))).await.is_ok() {
                trace_stats.record_client_chunk(started, bytes);
                true
            } else {
                trace_stats.finish("downstream_closed");
                false
            }
        }

        #[inline]
        async fn emit_c2r_frame(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            trace_stats: &mut StreamTraceStats,
            started: &Instant,
            frame_json: &str,
        ) -> bool {
            append_codex_ws_client_trace(trace, frame_json);
            // Include the SSE `event:` line so Codex parses it the same as
            // upstream Passthrough events. Without it, frames default to the
            // "message" event type and stricter SSE clients may drop them.
            let event_type = serde_json::from_str::<serde_json::Value>(frame_json)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string));
            let sse_line = match event_type {
                Some(t) => format!("event: {t}\ndata: {frame_json}\n\n"),
                None => format!("data: {frame_json}\n\n"),
            };
            let bytes = sse_line.len();
            if tx.send(Ok(Bytes::from(sse_line))).await.is_ok() {
                trace_stats.record_client_chunk(started, bytes);
                true
            } else {
                trace_stats.finish("downstream_closed");
                false
            }
        }

        #[inline]
        async fn flush_one_sse_block(
            tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
            trace: &mut String,
            trace_stats: &mut StreamTraceStats,
            started: &Instant,
            mode: &mut CodexHttpSseMode,
            event_block: &str,
            session_id: &str,
            item_id: &str,
            accumulator: &mut transforms::ChatCompletionsC2rAccumulator,
            terminal_done: &mut bool,
            status_injection: &mut Option<CodexStatusInjection>,
            summary_injection: &mut codex_summary::SummaryAccumulator,
        ) -> bool {
            loop {
                match *mode {
                    CodexHttpSseMode::Undecided => {
                        match classify_codex_upstream_sse_frame(event_block) {
                            Some(true) => {
                                *mode = CodexHttpSseMode::C2r;
                                continue;
                            }
                            Some(false) => {
                                *mode = CodexHttpSseMode::Passthrough;
                                continue;
                            }
                            None => {
                                return emit_raw_frame(
                                    tx,
                                    trace,
                                    trace_stats,
                                    started,
                                    event_block,
                                )
                                .await;
                            }
                        }
                    }
                    CodexHttpSseMode::Passthrough => {
                        let block_to_forward = summary_injection
                            .maybe_append_to_sse_block(
                                event_block,
                                started.elapsed().as_millis() as i64,
                            )
                            .unwrap_or_else(|| event_block.to_owned());
                        if !emit_raw_frame(tx, trace, trace_stats, started, &block_to_forward).await
                        {
                            return false;
                        }
                        if codex_sse_block_has_response_created(event_block) {
                            let effective_id = codex_sse_block_extract_response_id(event_block)
                                .unwrap_or_else(|| session_id.to_string());
                            if let Some(injection) = status_injection.as_mut() {
                                for frame in injection.next_frames(&effective_id) {
                                    trace_stats.mark_status_injected();
                                    if !emit_c2r_frame(tx, trace, trace_stats, started, &frame)
                                        .await
                                    {
                                        return false;
                                    }
                                }
                            }
                        }
                        return true;
                    }
                    CodexHttpSseMode::C2r => {
                        for ws_frame in summary_injection.maybe_append_to_frame_batch(
                            codex_sse_block_to_ws_frames(
                                event_block,
                                session_id,
                                item_id,
                                accumulator,
                                terminal_done,
                            ),
                            started.elapsed().as_millis() as i64,
                        ) {
                            if !emit_c2r_frame(tx, trace, trace_stats, started, &ws_frame).await {
                                return false;
                            }
                            if codex_frame_is_response_created(&ws_frame) {
                                if let Some(injection) = status_injection.as_mut() {
                                    for frame in injection.next_frames(session_id) {
                                        trace_stats.mark_status_injected();
                                        if !emit_c2r_frame(tx, trace, trace_stats, started, &frame)
                                            .await
                                        {
                                            return false;
                                        }
                                    }
                                }
                            }
                        }
                        return true;
                    }
                }
            }
        }

        let mut accumulator = transforms::ChatCompletionsC2rAccumulator::default();
        let mut terminal_done = false;

        let mut byte_stream = body.into_data_stream();
        let mut buf = String::new();
        let mut stream_broken = false;

        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    trace_stats.record_upstream_chunk(&request_started_instant, bytes.len());
                    if status_injection.is_none() {
                        status_injection = CodexStatusInjection::new(
                            visual.clone(),
                            request_started_instant.elapsed().as_millis() as i64,
                            !should_show_status || !state.codex_route_status_enabled(),
                        );
                    }
                    buf.push_str(&String::from_utf8_lossy(&bytes));
                }
                Err(_) => {
                    trace_stats.finish_error("upstream_read_error", "body stream read error");
                    stream_broken = true;
                    break;
                }
            }

            while let Some(end) = buf.find("\n\n") {
                let block = buf[..end].to_string();
                buf.drain(..end + 2);
                trace_stats.record_sse_block(&request_started_instant, &block);
                if !flush_one_sse_block(
                    &tx,
                    &mut trace,
                    &mut trace_stats,
                    &request_started_instant,
                    &mut mode,
                    &block,
                    &session_id,
                    &item_id,
                    &mut accumulator,
                    &mut terminal_done,
                    &mut status_injection,
                    &mut summary_injection,
                )
                .await
                {
                    drop(tx);
                    persist_codex_client_response_body(
                        &state,
                        log_row_id,
                        trace,
                        Some(trace_stats),
                    )
                    .await;
                    return;
                }
            }
        }

        if !buf.trim().is_empty() {
            buf.push('\n');
            buf.push('\n');
            while let Some(end) = buf.find("\n\n") {
                let block = buf[..end].to_string();
                buf.drain(..end + 2);
                trace_stats.record_sse_block(&request_started_instant, &block);
                let _ = flush_one_sse_block(
                    &tx,
                    &mut trace,
                    &mut trace_stats,
                    &request_started_instant,
                    &mut mode,
                    &block,
                    &session_id,
                    &item_id,
                    &mut accumulator,
                    &mut terminal_done,
                    &mut status_injection,
                    &mut summary_injection,
                )
                .await;
            }
        }

        if mode == CodexHttpSseMode::C2r && !terminal_done {
            let detail = if stream_broken {
                "upstream SSE read error before a terminal chunk (no finish_reason / [DONE] seen)"
            } else {
                "upstream stream ended before a terminal chunk (no finish_reason / [DONE] seen)"
            };
            let payload = transforms::codex_response_proxy_fault_event(
                &session_id,
                "upstream_stream_truncated",
                detail,
            );
            append_codex_ws_client_trace(&mut trace, &payload);
            let sse_line = format!("data: {}\n\n", payload);
            trace_stats.mark_terminal_injected();
            let bytes = sse_line.len();
            if tx.send(Ok(Bytes::from(sse_line))).await.is_ok() {
                trace_stats.record_client_chunk(&request_started_instant, bytes);
            }
        }

        if trace_stats.terminal_seen() {
            trace_stats.finish("completed");
        } else if trace_stats.end_reason().is_none() {
            trace_stats.finish("upstream_eof");
        }
        drop(tx);
        persist_codex_client_response_body(&state, log_row_id, trace, Some(trace_stats)).await;
    });

    let mut out = Response::new(Body::from_stream(ReceiverStream::new(rx)));
    *out.status_mut() = parts.status;
    *out.version_mut() = parts.version;
    *out.headers_mut() = out_headers;
    out
}

/// One SSE event block (text between `\n\n`) → Codex WebSocket text frames.
/// Accumulate the full JSON-Lines trace forwarded to Codex so `client_response_body` can be fully persisted without truncation.
fn append_codex_ws_client_trace(acc: &mut String, json_line: &str) {
    if !acc.is_empty() {
        acc.push('\n');
    }
    acc.push_str(json_line);
}

async fn persist_codex_client_response_body(
    state: &AppState,
    row_id: Option<String>,
    trace: String,
    stats: Option<StreamTraceStats>,
) {
    let Some(id) = row_id else {
        return;
    };
    if trace.is_empty() && stats.is_none() {
        return;
    }
    let db = state.db.clone();
    let id_for_warn = id.clone();
    let res = tokio::task::spawn_blocking(move || {
        if let Some(stats) = stats {
            let mut log = empty_patch_log(&id);
            log.client_response_body = (!trace.is_empty()).then_some(trace);
            stats.apply_to_log(&mut log);
            db.log_update_client_trace_and_stream_fields(&log)
        } else {
            db.log_set_client_response_body(&id, Some(&trace))
        }
    })
    .await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(
            log_id = %id_for_warn,
            ?e,
            "failed to PATCH client_response_body"
        ),
        Err(j) => {
            tracing::warn!(log_id = %id_for_warn, %j, "join error patching client_response_body")
        }
    }
}

fn empty_patch_log(id: &str) -> vibe_protocol::RequestLog {
    let mut log = vibe_protocol::RequestLog {
        id: id.to_string(),
        started_at: 0,
        app: None,
        provider_id: None,
        requested_model: None,
        upstream_model: None,
        status_code: None,
        error: None,
        latency_ms: None,
        first_token_ms: None,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        estimated_cost_usd: "0".into(),
        wire: None,
        route_prefix: None,
        credential_id: None,
        cb_key: None,
        upstream_http_status: None,
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
    };
    empty_stream_fields(&mut log);
    log
}

fn codex_sse_block_to_ws_frames(
    event_block: &str,
    session_id: &str,
    item_id: &str,
    accumulator: &mut transforms::ChatCompletionsC2rAccumulator,
    terminal_done: &mut bool,
) -> Vec<String> {
    let mut frames = Vec::new();
    for raw_line in event_block.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" {
                continue;
            }
            if transforms::upstream_sse_data_is_terminal(data) {
                *terminal_done = true;
            }
            let events = if data.contains("\"choices\"") {
                transforms::chat_event_to_responses_events(data, session_id, item_id, accumulator)
            } else {
                let skip_usage_tail = serde_json::from_str::<serde_json::Value>(data)
                    .map(|v| v.get("choices").is_none() && v.get("usage").is_some())
                    .unwrap_or(false);
                if skip_usage_tail {
                    vec![]
                } else {
                    vec![data.to_string()]
                }
            };
            frames.extend(events);
        }
    }
    frames
}

/// WebSocket bridge: receive request JSON → forward via HTTP → stream events back.
struct CodexWsActiveGuard {
    state: AppState,
}

impl Drop for CodexWsActiveGuard {
    fn drop(&mut self) {
        self.state.codex_transport.ws_closed();
    }
}

async fn codex_ws_bridge(mut socket: WebSocket, state: AppState, ws_headers: HeaderMap) {
    let _active_guard = CodexWsActiveGuard {
        state: state.clone(),
    };
    // Codex keeps the WS connection alive across multiple turns (tool execution
    // cycles).  We loop here to handle each `response.create` message that
    // arrives on the same connection.
    loop {
        // 1. Wait for the next request message from Codex.
        let body_bytes: Bytes = loop {
            match socket.recv().await {
                Some(Ok(Message::Text(t))) => break Bytes::from(t.into_bytes()),
                Some(Ok(Message::Binary(b))) => break Bytes::from(b),
                Some(Ok(Message::Close(_))) | None => return,
                Some(Ok(_)) => continue, // ping/pong — ignore
                Some(Err(_)) => return,
            }
        };
        state.codex_transport.ws_request();

        // 2. Strip the WS envelope: {"type":"response.create", ...} → {...}
        {
            let preview = String::from_utf8_lossy(&body_bytes[..body_bytes.len().min(300)]);
            tracing::debug!(preview = %preview, "codex ws body (first 300 bytes)");
        }
        let stripped = transforms::strip_ws_envelope(&body_bytes);
        let should_show_status = transforms::responses_input_ends_with_user_message(&stripped);
        let turn_id = codex_summary::turn_id_from_request(&body_bytes)
            .or_else(|| codex_summary::turn_id_from_request(&stripped));

        match crate::codex_upstream_ws::try_forward_official_codex_ws(
            &mut socket,
            state.clone(),
            ws_headers.clone(),
            body_bytes.clone(),
            StatusDecision {
                should_show_status,
                turn_id: turn_id.clone(),
            },
        )
        .await
        {
            UpstreamWsOutcome::Forwarded => continue,
            UpstreamWsOutcome::Fallback(body) => {
                tracing::debug!(
                    bytes = body.len(),
                    "codex ws selected non-official upstream; using HTTP bridge fallback"
                );
            }
        }

        // For WS mode, always request streaming from the upstream.
        let http_body: Bytes = {
            let mut val: serde_json::Value = serde_json::from_slice(&stripped)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            if let Some(obj) = val.as_object_mut() {
                obj.insert("stream".into(), serde_json::Value::Bool(true));
            }
            serde_json::to_vec(&val)
                .map(Bytes::from)
                .unwrap_or(stripped.clone())
        };
        {
            let model = serde_json::from_slice::<serde_json::Value>(&http_body)
                .ok()
                .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
                .unwrap_or_default();
            tracing::debug!(model = %model, "codex ws stripped body model");
        }

        // Per-response IDs.
        let session_id = format!("resp-{}", uuid::Uuid::new_v4().simple());
        let item_id = format!("msg-{}", uuid::Uuid::new_v4().simple());

        // 3. Build minimal headers for the forward call.
        let mut req_headers = ws_headers.clone();
        req_headers.insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        // 4. Forward to upstream; get back an axum Response.
        let request_started_instant = Instant::now();
        let response = forward::forward(
            state.clone(),
            Wire::OpenaiResponses,
            None,
            req_headers,
            http_body,
            Some("codex-v1".into()),
        )
        .await;

        let (parts, body) = response.into_parts();
        let stream_log_row_id = parts
            .extensions
            .get::<forward::VibeLogId>()
            .map(|x| x.0.clone());
        let visual = parts
            .extensions
            .get::<VibeCodexVisual>()
            .map(|x| x.0.clone());
        let codex_client_kind = parts
            .extensions
            .get::<VibeCodexClientKind>()
            .map(|x| x.0)
            .unwrap_or(codex_summary::CodexClientKind::Unknown);
        let suppress_status = !state.codex_route_status_enabled()
            || visual
                .as_ref()
                .map(|visual| {
                    !should_show_status
                        || !should_emit_codex_route_status(&state, turn_id.as_deref(), visual)
                })
                .unwrap_or(false);
        let mut client_ws_trace = String::new();
        let mut trace_stats = StreamTraceStats::new("websocket", "responses_to_ws");
        let mut summary_injection = codex_summary::SummaryAccumulator::new_for_turn(
            state.codex_summary_config(),
            codex_client_kind,
            Some(state.clone()),
            turn_id.clone(),
        );

        // 5. Non-2xx: emit a Responses-shaped `response.failed` frame so Codex CLI can
        //    surface ServerOverloaded / retry / quota — not plain text (which leaves the
        //    turn without a terminal event and triggers reconnect spam).
        if !parts.status.is_success() {
            let status_u16 = parts.status.as_u16();
            let detail = axum::body::to_bytes(body, 64 * 1024)
                .await
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let payload = transforms::codex_response_failed_event(&session_id, status_u16, &detail);
            append_codex_ws_client_trace(&mut client_ws_trace, &payload);
            trace_stats.mark_terminal_injected();
            if socket.send(Message::Text(payload.clone())).await.is_ok() {
                trace_stats.record_client_chunk(&request_started_instant, payload.len());
            } else {
                trace_stats.finish("downstream_closed");
            }
            trace_stats.finish("completed");
            persist_codex_client_response_body(
                &state,
                stream_log_row_id,
                client_ws_trace,
                Some(trace_stats),
            )
            .await;
            continue;
        }

        let content_type = parts
            .headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let is_sse = content_type.contains("event-stream");

        if is_sse {
            // 6a. Streaming SSE: parse each event and emit as Responses API WS messages.
            use futures_util::StreamExt as _;
            let mut stream = body.into_data_stream();
            let mut buf = String::new();
            let mut accumulator = transforms::ChatCompletionsC2rAccumulator::default();
            let mut terminal_done = false;
            let mut stream_broken = false;
            let mut status_injection: Option<CodexStatusInjection> = None;

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        trace_stats.record_upstream_chunk(&request_started_instant, bytes.len());
                        if status_injection.is_none() {
                            status_injection = CodexStatusInjection::new(
                                visual.clone(),
                                request_started_instant.elapsed().as_millis() as i64,
                                suppress_status,
                            );
                        }
                        // Never drop bytes on a non-UTF8 chunk boundary (reqwest can split codepoints).
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Err(_) => {
                        trace_stats.finish_error("upstream_read_error", "body stream read error");
                        stream_broken = true;
                        break;
                    }
                }

                // Consume complete SSE events (terminated by blank line).
                while let Some(event_end) = buf.find("\n\n") {
                    let event_block = buf[..event_end].to_string();
                    buf.drain(..event_end + 2);
                    trace_stats.record_sse_block(&request_started_instant, &event_block);
                    for event_str in summary_injection.maybe_append_to_frame_batch(
                        codex_sse_block_to_ws_frames(
                            &event_block,
                            &session_id,
                            &item_id,
                            &mut accumulator,
                            &mut terminal_done,
                        ),
                        request_started_instant.elapsed().as_millis() as i64,
                    ) {
                        tracing::debug!(event = %&event_str[..event_str.len().min(200)], "codex ws → client event");
                        append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                        let is_created = codex_frame_is_response_created(&event_str);
                        if socket.send(Message::Text(event_str.clone())).await.is_err() {
                            trace_stats.finish("downstream_closed");
                            persist_codex_client_response_body(
                                &state,
                                stream_log_row_id,
                                client_ws_trace,
                                Some(trace_stats),
                            )
                            .await;
                            return; // client disconnected
                        } else {
                            trace_stats
                                .record_client_chunk(&request_started_instant, event_str.len());
                        }
                        if is_created {
                            let effective_id = extract_response_created_id(&event_str)
                                .unwrap_or_else(|| session_id.clone());
                            if let Some(injection) = status_injection.as_mut() {
                                for injected in injection.next_frames(&effective_id) {
                                    tracing::debug!(event = %&injected[..injected.len().min(200)], "codex ws injected status → client event");
                                    append_codex_ws_client_trace(&mut client_ws_trace, &injected);
                                    trace_stats.mark_status_injected();
                                    if socket.send(Message::Text(injected.clone())).await.is_err() {
                                        trace_stats.finish("downstream_closed");
                                        persist_codex_client_response_body(
                                            &state,
                                            stream_log_row_id,
                                            client_ws_trace,
                                            Some(trace_stats),
                                        )
                                        .await;
                                        return;
                                    } else {
                                        trace_stats.record_client_chunk(
                                            &request_started_instant,
                                            injected.len(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Some upstreams end the body without a final blank line after the last `data:`.
            if !buf.trim().is_empty() {
                buf.push('\n');
                buf.push('\n');
                while let Some(event_end) = buf.find("\n\n") {
                    let event_block = buf[..event_end].to_string();
                    buf.drain(..event_end + 2);
                    trace_stats.record_sse_block(&request_started_instant, &event_block);
                    for event_str in summary_injection.maybe_append_to_frame_batch(
                        codex_sse_block_to_ws_frames(
                            &event_block,
                            &session_id,
                            &item_id,
                            &mut accumulator,
                            &mut terminal_done,
                        ),
                        request_started_instant.elapsed().as_millis() as i64,
                    ) {
                        tracing::debug!(event = %&event_str[..event_str.len().min(200)], "codex ws flush → client event");
                        append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                        let is_created = codex_frame_is_response_created(&event_str);
                        if socket.send(Message::Text(event_str.clone())).await.is_err() {
                            trace_stats.finish("downstream_closed");
                            persist_codex_client_response_body(
                                &state,
                                stream_log_row_id,
                                client_ws_trace,
                                Some(trace_stats),
                            )
                            .await;
                            return;
                        } else {
                            trace_stats
                                .record_client_chunk(&request_started_instant, event_str.len());
                        }
                        if is_created {
                            let effective_id = extract_response_created_id(&event_str)
                                .unwrap_or_else(|| session_id.clone());
                            if let Some(injection) = status_injection.as_mut() {
                                for injected in injection.next_frames(&effective_id) {
                                    tracing::debug!(event = %&injected[..injected.len().min(200)], "codex ws injected flush status → client event");
                                    append_codex_ws_client_trace(&mut client_ws_trace, &injected);
                                    trace_stats.mark_status_injected();
                                    if socket.send(Message::Text(injected.clone())).await.is_err() {
                                        trace_stats.finish("downstream_closed");
                                        persist_codex_client_response_body(
                                            &state,
                                            stream_log_row_id,
                                            client_ws_trace,
                                            Some(trace_stats),
                                        )
                                        .await;
                                        return;
                                    } else {
                                        trace_stats.record_client_chunk(
                                            &request_started_instant,
                                            injected.len(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !terminal_done {
                let detail = if stream_broken {
                    "upstream SSE read error before a terminal chunk (no finish_reason / [DONE] seen)"
                } else {
                    "upstream stream ended before a terminal chunk (no finish_reason / [DONE] seen)"
                };
                let payload = transforms::codex_response_proxy_fault_event(
                    &session_id,
                    "upstream_stream_truncated",
                    detail,
                );
                append_codex_ws_client_trace(&mut client_ws_trace, &payload);
                trace_stats.mark_terminal_injected();
                if socket.send(Message::Text(payload.clone())).await.is_err() {
                    trace_stats.finish("downstream_closed");
                    persist_codex_client_response_body(
                        &state,
                        stream_log_row_id,
                        client_ws_trace,
                        Some(trace_stats),
                    )
                    .await;
                    return;
                } else {
                    trace_stats.record_client_chunk(&request_started_instant, payload.len());
                }
            }
        } else {
            // 6b. Non-SSE: upstream can still return full Chat JSON. Codex WS only accepts event sequences with `type`,
            //     so do not send a raw `response` object directly (see transforms::chat_completion_non_stream_to_ws_events).
            if let Ok(bytes) = axum::body::to_bytes(body, 8 * 1024 * 1024).await {
                trace_stats.record_upstream_chunk(&request_started_instant, bytes.len());
                if bytes.windows(9).any(|w| w == b"\"choices\"") {
                    match transforms::chat_completion_non_stream_to_ws_events(
                        &bytes,
                        &session_id,
                        &item_id,
                    ) {
                        Ok(frames) => {
                            let mut status_injection = CodexStatusInjection::new(
                                visual.clone(),
                                request_started_instant.elapsed().as_millis() as i64,
                                suppress_status,
                            );
                            for event_str in summary_injection.maybe_append_to_frame_batch(
                                frames,
                                request_started_instant.elapsed().as_millis() as i64,
                            ) {
                                tracing::debug!(
                                    event = %&event_str[..event_str.len().min(200)],
                                    "codex ws non-sse → client event"
                                );
                                append_codex_ws_client_trace(&mut client_ws_trace, &event_str);
                                let is_created = codex_frame_is_response_created(&event_str);
                                trace_stats.record_ws_text(&request_started_instant, &event_str);
                                if socket.send(Message::Text(event_str.clone())).await.is_err() {
                                    trace_stats.finish("downstream_closed");
                                    persist_codex_client_response_body(
                                        &state,
                                        stream_log_row_id,
                                        client_ws_trace,
                                        Some(trace_stats),
                                    )
                                    .await;
                                    return;
                                } else {
                                    trace_stats.record_client_chunk(
                                        &request_started_instant,
                                        event_str.len(),
                                    );
                                }
                                if is_created {
                                    let effective_id = extract_response_created_id(&event_str)
                                        .unwrap_or_else(|| session_id.clone());
                                    if let Some(injection) = status_injection.as_mut() {
                                        for injected in injection.next_frames(&effective_id) {
                                            append_codex_ws_client_trace(
                                                &mut client_ws_trace,
                                                &injected,
                                            );
                                            trace_stats.mark_status_injected();
                                            if socket
                                                .send(Message::Text(injected.clone()))
                                                .await
                                                .is_err()
                                            {
                                                trace_stats.finish("downstream_closed");
                                                persist_codex_client_response_body(
                                                    &state,
                                                    stream_log_row_id,
                                                    client_ws_trace,
                                                    Some(trace_stats),
                                                )
                                                .await;
                                                return;
                                            } else {
                                                trace_stats.record_client_chunk(
                                                    &request_started_instant,
                                                    injected.len(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(()) => {
                            let payload = transforms::codex_response_proxy_fault_event(
                                &session_id,
                                "upstream_invalid_chat_completion_json",
                                "upstream returned a non-stream body that is not valid Chat Completions JSON",
                            );
                            trace_stats.mark_terminal_injected();
                            if socket.send(Message::Text(payload.clone())).await.is_err() {
                                trace_stats.finish("downstream_closed");
                                persist_codex_client_response_body(
                                    &state,
                                    stream_log_row_id,
                                    client_ws_trace,
                                    Some(trace_stats),
                                )
                                .await;
                                return;
                            } else {
                                trace_stats
                                    .record_client_chunk(&request_started_instant, payload.len());
                            }
                        }
                    }
                } else {
                    let detail = String::from_utf8_lossy(&bytes).into_owned();
                    let payload = transforms::codex_response_proxy_fault_event(
                        &session_id,
                        "upstream_body_not_chat_completion",
                        &format!("upstream body did not look like Chat Completions JSON: {detail}"),
                    );
                    append_codex_ws_client_trace(&mut client_ws_trace, &payload);
                    trace_stats.mark_terminal_injected();
                    if socket.send(Message::Text(payload.clone())).await.is_err() {
                        trace_stats.finish("downstream_closed");
                        persist_codex_client_response_body(
                            &state,
                            stream_log_row_id,
                            client_ws_trace,
                            Some(trace_stats),
                        )
                        .await;
                        return;
                    } else {
                        trace_stats.record_client_chunk(&request_started_instant, payload.len());
                    }
                }
            }
        }
        if trace_stats.terminal_seen() {
            trace_stats.finish("completed");
        } else if trace_stats.end_reason().is_none() {
            trace_stats.finish("upstream_eof");
        }
        persist_codex_client_response_body(
            &state,
            stream_log_row_id,
            client_ws_trace,
            Some(trace_stats),
        )
        .await;
        // Loop back to wait for the next response.create on this same connection.
    }
}

/// Plain-HTTP wrapper for /codex/v1/chat/completions (no WS needed).
async fn post_or_reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if is_websocket_upgrade(&headers) {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "WebSocket not supported on chat/completions",
        )
            .into_response();
    }
    forward::forward(
        state,
        Wire::OpenaiChat,
        None,
        headers,
        body,
        Some("codex-v1".into()),
    )
    .await
}

async fn post_gemini(
    State(state): State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let upstream_path = format!("/v1beta/models/{}", path);
    forward::forward(
        state,
        Wire::GeminiNative,
        Some(upstream_path),
        headers,
        body,
        Some("gemini-v1".into()),
    )
    .await
}

async fn refresh_provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Provider>, AppError> {
    let provider = run_blocking(state.clone(), move |s| s.db.provider_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    let refreshed = refresh_remote_models_for_provider(&state, &provider).await?;
    publish_providers_overview_soon(state);
    Ok(Json(refreshed))
}

pub(crate) async fn refresh_remote_models_for_provider_if_supported(
    state: &AppState,
    provider: &Provider,
) -> anyhow::Result<Provider> {
    match provider.kind {
        vibe_protocol::ProviderKind::GeminiNative => Ok(provider.clone()),
        _ => refresh_remote_models_for_provider(state, provider).await,
    }
}

async fn refresh_remote_models_for_provider(
    state: &AppState,
    provider: &Provider,
) -> anyhow::Result<Provider> {
    let secret = if let Some(auth_ref) = provider.auth_ref.as_deref() {
        Some(crate::secrets::resolve(auth_ref)?)
    } else {
        None
    };
    let base = provider.base_url.trim_end_matches('/');
    let url = match provider.kind {
        vibe_protocol::ProviderKind::OpenaiChat | vibe_protocol::ProviderKind::OpenaiResponses => {
            format!("{base}/v1/models")
        }
        vibe_protocol::ProviderKind::Anthropic => format!("{base}/v1/models"),
        vibe_protocol::ProviderKind::GeminiNative => {
            anyhow::bail!("gemini-native model list refresh not supported yet")
        }
    };
    let mut req = state.http.get(url);
    if let Some(secret) = secret.as_deref() {
        req = req.bearer_auth(secret);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("model list refresh failed: HTTP {}", resp.status());
    }
    let v: serde_json::Value = resp.json().await?;
    let mut names = Vec::<String>::new();
    if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                let s = id.trim();
                if !s.is_empty() {
                    names.push(s.to_string());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    let id = provider.id.clone();
    let fetched_at = chrono::Utc::now().timestamp();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.provider_update_remote_models(&id, names, fetched_at)
    })
    .await?;
    Ok(updated)
}

// ---------------------------------------------------------------------------
// Local import (scan installed tools → ready-to-use ProviderInput)
// ---------------------------------------------------------------------------

/// `GET /_vp/providers/import-local`
/// Scan locally installed Claude Code / Codex CLI and return importable candidates.
/// No database writes; filesystem reads only.
async fn scan_local_providers() -> Json<Vec<local_import::LocalCandidate>> {
    Json(local_import::scan())
}

fn provider_to_input(p: &Provider) -> ProviderInput {
    ProviderInput {
        name: p.name.clone(),
        group_name: p.group_name.clone(),
        avatar_url: p.avatar_url.clone(),
        kind: p.kind,
        base_url: p.base_url.clone(),
        auth_ref: p.auth_ref.clone(),
        enabled: p.enabled,
        priority: p.priority,
        supports_websocket: p.supports_websocket,
        passthrough_mode: p.passthrough_mode,
        model_aliases: p.model_aliases.clone(),
    }
}

fn model_aliases_equal(a: &[vibe_protocol::ModelAlias], b: &[vibe_protocol::ModelAlias]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(a, b)| a.alias == b.alias && a.upstream_model == b.upstream_model)
}

/// For re-imports with the same fingerprint, merge tokens / cached identity to avoid skipping fresh local OAuth material.
fn merge_codex_credential_on_reimport(
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
async fn import_local_providers(
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
            } else if c.client.starts_with("ccs:") {
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

async fn upsert_import_plan(
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

async fn import_ccs_profile_bundle(
    State(state): State<AppState>,
    Json(bundle): Json<Value>,
) -> Result<Json<Provider>, AppError> {
    let plan = local_import::ccs_bundle_to_plan(&bundle)?;
    Ok(Json(upsert_import_plan(state, plan).await?))
}

#[derive(Debug, Deserialize)]
struct CcSwitchImportRequest {
    url: String,
}

async fn import_cc_switch_deeplink(
    State(state): State<AppState>,
    Json(input): Json<CcSwitchImportRequest>,
) -> Result<Json<Provider>, AppError> {
    let plan = local_import::cc_switch_deeplink_to_plan(&input.url)?;
    Ok(Json(upsert_import_plan(state, plan).await?))
}

type ClientStatusResponse = ClientStatus;

#[derive(Debug, serde::Serialize)]
struct ClientDoctorResponse {
    client: String,
    ok: bool,
    checks: Vec<ClientDoctorCheck>,
}

#[derive(Debug, serde::Serialize)]
struct ClientDoctorCheck {
    name: String,
    ok: bool,
    detail: String,
}

type ClientTakeoverResponse = ClientTakeoverResult;

async fn client_status(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientStatusResponse>, AppError> {
    let status = client_status_inner(&client, state.port)?;
    Ok(Json(status))
}

async fn client_doctor(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientDoctorResponse>, AppError> {
    let status = client_status_inner(&client, state.port)?;
    let mut checks = Vec::new();
    checks.push(ClientDoctorCheck {
        name: "config_exists".into(),
        ok: status.config_exists,
        detail: status.config_path.clone(),
    });
    checks.push(ClientDoctorCheck {
        name: "base_url_points_to_vibe".into(),
        ok: status.taken_over,
        detail: status
            .configured_base_url
            .clone()
            .unwrap_or_else(|| "(missing)".into()),
    });
    if let Some(proxy_managed) = status.auth_proxy_managed {
        checks.push(ClientDoctorCheck {
            name: "auth_proxy_managed".into(),
            ok: proxy_managed,
            detail: if proxy_managed {
                "client token is delegated to vibe".into()
            } else {
                "client still has a direct token or no proxy marker".into()
            },
        });
    }
    checks.push(ClientDoctorCheck {
        name: "model_overrides_cleared".into(),
        ok: status.model_overrides_present.is_empty(),
        detail: if status.model_overrides_present.is_empty() {
            "no known model override env vars found".into()
        } else {
            status.model_overrides_present.join(", ")
        },
    });
    let ok = checks.iter().all(|c| c.ok);
    Ok(Json(ClientDoctorResponse { client, ok, checks }))
}

async fn client_takeover(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientTakeoverResponse>, AppError> {
    let base_url = format!("http://127.0.0.1:{}", state.port);
    let outcome = run_blocking(state.clone(), {
        let client = client.clone();
        move |_| crate::takeover::takeover(&client, &base_url)
    })
    .await?;
    let status = client_status_inner(&client, state.port)?;
    state
        .ws
        .publish(WsEvent::ClientStatusChanged(status.clone()));
    Ok(Json(ClientTakeoverResponse {
        client: outcome.client,
        config_path: outcome.config_path,
        backup_path: outcome.backup_path,
        status,
    }))
}

async fn client_restore(
    State(state): State<AppState>,
    Path(client): Path<String>,
) -> Result<Json<ClientTakeoverResponse>, AppError> {
    let outcome = run_blocking(state.clone(), {
        let client = client.clone();
        move |_| crate::takeover::restore(&client)
    })
    .await?;
    let status = client_status_inner(&client, state.port)?;
    state
        .ws
        .publish(WsEvent::ClientStatusChanged(status.clone()));
    Ok(Json(ClientTakeoverResponse {
        client: outcome.client,
        config_path: outcome.config_path,
        backup_path: outcome.backup_path,
        status,
    }))
}

fn client_status_inner(client: &str, port: u16) -> Result<ClientStatusResponse, AppError> {
    let base = format!("http://127.0.0.1:{port}");
    match client {
        "claude" => {
            let path = crate::paths::claude_settings_path()?;
            let expected = format!("{base}/claude");
            let (configured, auth_proxy, overrides, notes) = read_claude_client_config(&path)?;
            Ok(ClientStatusResponse {
                client: client.into(),
                config_path: path.display().to_string(),
                config_exists: path.exists(),
                taken_over: configured.as_deref() == Some(expected.as_str()),
                expected_base_url: expected,
                configured_base_url: configured,
                auth_proxy_managed: auth_proxy,
                model_overrides_present: overrides,
                notes,
            })
        }
        "codex" => {
            let path = crate::paths::codex_config_path()?;
            let expected = format!("{base}/codex/v1");
            let configured = read_codex_base_url(&path)?;
            Ok(ClientStatusResponse {
                client: client.into(),
                config_path: path.display().to_string(),
                config_exists: path.exists(),
                taken_over: configured.as_deref() == Some(expected.as_str()),
                expected_base_url: expected,
                configured_base_url: configured,
                auth_proxy_managed: None,
                model_overrides_present: Vec::new(),
                notes: Vec::new(),
            })
        }
        "opencode" => {
            let path = crate::paths::opencode_config_path()?;
            let expected = format!("{base}/opencode/v1");
            let configured = read_opencode_base_url(&path)?;
            Ok(ClientStatusResponse {
                client: client.into(),
                config_path: path.display().to_string(),
                config_exists: path.exists(),
                taken_over: configured.as_deref() == Some(expected.as_str()),
                expected_base_url: expected,
                configured_base_url: configured,
                auth_proxy_managed: None,
                model_overrides_present: Vec::new(),
                notes: Vec::new(),
            })
        }
        other => Err(anyhow::anyhow!(
            "unknown client: {other}. Supported: claude, codex, opencode"
        )
        .into()),
    }
}

fn read_claude_client_config(
    path: &PathBuf,
) -> anyhow::Result<(Option<String>, Option<bool>, Vec<String>, Vec<String>)> {
    if !path.exists() {
        return Ok((None, None, Vec::new(), vec!["config file missing".into()]));
    }
    let raw = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;
    let env = v.get("env").and_then(|x| x.as_object());
    let configured = env
        .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let auth_proxy = env
        .and_then(|e| {
            e.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| e.get("ANTHROPIC_API_KEY"))
        })
        .and_then(|x| x.as_str())
        .map(|s| s == "PROXY_MANAGED");
    let overrides = [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ]
    .iter()
    .filter(|k| env.map(|e| e.contains_key(**k)).unwrap_or(false))
    .map(|s| (*s).to_string())
    .collect();
    Ok((configured, auth_proxy, overrides, Vec::new()))
}

fn read_codex_base_url(path: &PathBuf) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let v: toml::Value = toml::from_str(&raw)?;
    let active_provider = v.get("model_provider").and_then(|x| x.as_str());
    let provider_key = active_provider.unwrap_or("vibeplus");
    Ok(v.get("model_providers")
        .and_then(|x| x.get(provider_key))
        .and_then(|x| x.get("base_url"))
        .and_then(|x| x.as_str())
        .map(str::to_string))
}

fn read_opencode_base_url(path: &PathBuf) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let v: Value = serde_json::from_str(&raw)?;
    Ok(v.pointer("/provider/vibe/options/baseURL")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.pointer("/provider/vibe/options/baseUrl")
                .and_then(|x| x.as_str())
        })
        .map(str::to_string))
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
    let name = input.name.clone();
    let p = run_blocking(state.clone(), move |s| s.db.provider_insert(input)).await?;
    emit_app_log(
        &state,
        AppLogLevel::Info,
        "provider",
        format!("Provider added: {name}"),
    );
    publish_providers_overview_soon(state);
    Ok(Json(p))
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderInput>,
) -> Result<Json<Provider>, AppError> {
    let name = input.name.clone();
    let enabled = input.enabled;
    let id_for_update = id.clone();
    let p = run_blocking(state.clone(), move |s| {
        s.db.provider_update(&id_for_update, input)
    })
    .await?;
    // Bind toggles to circuit state: after switching, clear circuit state for the provider and its credentials,
    // avoiding UI enabled state while requests remain blocked by historical circuit breaks.
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;
    state.cb.reset(&id);
    for cid in cred_ids {
        state.cb.reset(&cid);
    }
    let msg = if enabled {
        format!("Provider enabled: {name}")
    } else {
        format!("Provider disabled: {name}")
    };
    emit_app_log(&state, AppLogLevel::Info, "provider", msg);
    publish_providers_overview_soon(state);
    Ok(Json(p))
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let id_clone = id.clone();
    run_blocking(state.clone(), move |s| s.db.provider_delete(&id_clone)).await?;
    emit_app_log(
        &state,
        AppLogLevel::Warn,
        "provider",
        format!("Provider deleted: {id}"),
    );
    publish_providers_overview_soon(state);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ProviderTestInput {
    model: Option<String>,
    stream: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct ProviderTestResponse {
    ok: bool,
    status: u16,
    latency_ms: i64,
    log_id: Option<String>,
    body_preview: String,
}

async fn provider_test(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderTestInput>,
) -> Result<Json<ProviderTestResponse>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| {
            s.db.provider_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;
    let model = input.model.unwrap_or_else(|| {
        provider
            .model_aliases
            .first()
            .map(|a| a.alias.clone())
            .unwrap_or_else(|| match provider.kind {
                vibe_protocol::ProviderKind::Anthropic => "claude-sonnet-4-5".into(),
                vibe_protocol::ProviderKind::GeminiNative => "gemini-2.5-pro".into(),
                _ => "gpt-5.3-codex".into(),
            })
    });
    let stream = input.stream.unwrap_or(false);
    let started = Instant::now();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        HeaderName::from_static("x-vibe-provider-test"),
        HeaderValue::from_static("1"),
    );
    let (wire, route_prefix, body) = match provider.kind {
        vibe_protocol::ProviderKind::Anthropic => (
            Wire::Anthropic,
            Some("provider-test".into()),
            serde_json::json!({
                "model": model,
                "max_tokens": 16,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": stream
            }),
        ),
        vibe_protocol::ProviderKind::GeminiNative => (
            Wire::GeminiNative,
            Some("provider-test".into()),
            serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "ping"}]}]
            }),
        ),
        vibe_protocol::ProviderKind::OpenaiChat => (
            Wire::OpenaiChat,
            Some("provider-test".into()),
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": stream
            }),
        ),
        vibe_protocol::ProviderKind::OpenaiResponses => (
            Wire::OpenaiResponses,
            Some("provider-test".into()),
            serde_json::json!({
                "model": model,
                "input": "ping",
                "stream": stream
            }),
        ),
    };
    let path = if provider.kind == vibe_protocol::ProviderKind::GeminiNative {
        Some(format!("/v1beta/models/{model}:generateContent"))
    } else {
        None
    };
    let response = forward::forward(
        state,
        wire,
        path,
        headers,
        Bytes::from(serde_json::to_vec(&body)?),
        route_prefix,
    )
    .await;
    let log_id = response
        .extensions()
        .get::<forward::VibeLogId>()
        .map(|x| x.0.clone());
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let preview = String::from_utf8_lossy(&bytes).chars().take(600).collect();
    Ok(Json(ProviderTestResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        latency_ms: started.elapsed().as_millis() as i64,
        log_id,
        body_preview: preview,
    }))
}

const SPEEDTEST_DEFAULT_TIMEOUT_SECS: u64 = 8;
const SPEEDTEST_MIN_TIMEOUT_SECS: u64 = 2;
const SPEEDTEST_MAX_TIMEOUT_SECS: u64 = 30;

fn sanitize_speedtest_timeout(timeout_secs: Option<u64>) -> u64 {
    timeout_secs
        .unwrap_or(SPEEDTEST_DEFAULT_TIMEOUT_SECS)
        .clamp(SPEEDTEST_MIN_TIMEOUT_SECS, SPEEDTEST_MAX_TIMEOUT_SECS)
}

fn speedtest_error_result(
    url: String,
    checked_at: i64,
    error: impl Into<String>,
) -> ProviderSpeedtestResult {
    ProviderSpeedtestResult {
        url,
        ok: false,
        latency_ms: None,
        status: None,
        error: Some(error.into()),
        checked_at,
    }
}

async fn run_endpoint_speedtest(
    raw_url: &str,
    timeout_secs: Option<u64>,
) -> ProviderSpeedtestResult {
    let checked_at = chrono::Utc::now().timestamp();
    let trimmed = raw_url.trim().to_string();
    if trimmed.is_empty() {
        return speedtest_error_result(trimmed, checked_at, "URL cannot be empty");
    }

    let parsed_url = match url::Url::parse(&trimmed) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => parsed,
        Ok(parsed) => {
            return speedtest_error_result(
                trimmed,
                checked_at,
                format!("Unsupported URL scheme: {}", parsed.scheme()),
            );
        }
        Err(err) => {
            return speedtest_error_result(trimmed, checked_at, format!("Invalid URL: {err}"))
        }
    };

    let timeout = Duration::from_secs(sanitize_speedtest_timeout(timeout_secs));
    let client = match reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(30))
        .timeout(timeout)
        .build()
    {
        Ok(client) => client,
        Err(err) => return speedtest_error_result(trimmed, checked_at, err.to_string()),
    };

    // Same logic as CC Switch: warm up once and ignore the result, then time the second request.
    let _ = client.get(parsed_url.clone()).timeout(timeout).send().await;

    let start = Instant::now();
    match client.get(parsed_url).timeout(timeout).send().await {
        Ok(resp) => ProviderSpeedtestResult {
            url: trimmed,
            ok: true,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(resp.status().as_u16()),
            error: None,
            checked_at,
        },
        Err(err) => {
            let error_message = if err.is_timeout() {
                "Request timed out".to_string()
            } else if err.is_connect() {
                "Connection failed".to_string()
            } else {
                err.to_string()
            };
            ProviderSpeedtestResult {
                url: trimmed,
                ok: false,
                latency_ms: None,
                status: err.status().map(|s| s.as_u16()),
                error: Some(error_message),
                checked_at,
            }
        }
    }
}

async fn provider_speedtest(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderSpeedtestInput>,
) -> Result<Json<Provider>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| {
            s.db.provider_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;

    let result = run_endpoint_speedtest(&provider.base_url, input.timeout_secs).await;
    let provider_id = provider.id.clone();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.provider_update_speedtest(&provider_id, result)
    })
    .await?;
    publish_providers_overview_soon(state);
    Ok(Json(updated))
}

async fn provider_probe(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderSpeedtestInput>,
) -> Result<Json<Provider>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| {
            s.db.provider_get(&id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;

    let result = run_protocol_probe(&state, &provider, input.timeout_secs).await;
    let provider_id = provider.id.clone();
    let updated = run_blocking(state.clone(), move |s| {
        s.db.provider_update_speedtest(&provider_id, result)
    })
    .await?;
    publish_providers_overview_soon(state);
    Ok(Json(updated))
}

async fn run_protocol_probe(
    state: &AppState,
    provider: &Provider,
    timeout_secs: Option<u64>,
) -> ProviderSpeedtestResult {
    let checked_at = chrono::Utc::now().timestamp();
    let timeout = Duration::from_secs(sanitize_speedtest_timeout(timeout_secs));
    let model = probe_model(provider);
    let secret = match probe_secret(state, provider).await {
        Ok(secret) => secret,
        Err(err) => {
            return speedtest_error_result(provider.base_url.clone(), checked_at, err.to_string());
        }
    };
    let (wire, body, upstream_path, url) = probe_request(provider, &model);
    let adapter = crate::providers::select(provider);
    let req = match adapter.build(
        provider,
        secret.as_deref(),
        &state.http,
        wire,
        &body,
        upstream_path.as_deref(),
    ) {
        Ok(req) => req,
        Err(err) => return speedtest_error_result(url, checked_at, err.to_string()),
    };

    let start = Instant::now();
    let resp = match req.timeout(timeout).send().await {
        Ok(resp) => resp,
        Err(err) => {
            let error_message = if err.is_timeout() {
                "Request timed out".to_string()
            } else if err.is_connect() {
                "Connection failed".to_string()
            } else {
                err.to_string()
            };
            return ProviderSpeedtestResult {
                url,
                ok: false,
                latency_ms: None,
                status: err.status().map(|s| s.as_u16()),
                error: Some(error_message),
                checked_at,
            };
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let preview = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(220)
            .collect::<String>();
        return ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: Some(if preview.trim().is_empty() {
                format!("HTTP {}", status.as_u16())
            } else {
                format!("HTTP {}: {}", status.as_u16(), preview)
            }),
            checked_at,
        };
    }

    let mut stream = resp.bytes_stream();
    match tokio::time::timeout(timeout, stream.next()).await {
        Ok(Some(Ok(chunk))) if !chunk.is_empty() => ProviderSpeedtestResult {
            url,
            ok: true,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: None,
            checked_at,
        },
        Ok(Some(Ok(_))) => ProviderSpeedtestResult {
            url,
            ok: true,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: None,
            checked_at,
        },
        Ok(Some(Err(err))) => ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: Some(format!("stream read failed: {err}")),
            checked_at,
        },
        Ok(None) => ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: Some(start.elapsed().as_millis() as i64),
            status: Some(status.as_u16()),
            error: Some("No response data received".into()),
            checked_at,
        },
        Err(_) => ProviderSpeedtestResult {
            url,
            ok: false,
            latency_ms: None,
            status: Some(status.as_u16()),
            error: Some("Request timed out".into()),
            checked_at,
        },
    }
}

async fn provider_sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderSyncInput>,
) -> Result<Json<ProviderSyncPreview>, AppError> {
    let provider = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            s.db.provider_get(&id2)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))
        }
    })
    .await?;

    let scope = input.scope.unwrap_or_else(|| "all".into());
    let mut updated_input = provider_to_input(&provider);
    let mut branding = SyncBranding::default();
    let mut financials = SyncFinancials::default();
    let mut note_parts: Vec<String> = Vec::new();

    if matches!(scope.as_str(), "all" | "brand") {
        if let Ok(found) = sync_fetch_branding(&state.http, &provider.base_url).await {
            if let Some(name) = found.display_name.clone() {
                updated_input.name = name.clone();
            }
            if let Some(url) = found.avatar_url.clone() {
                updated_input.avatar_url = Some(url);
            }
            branding = found;
            note_parts.push("brand".into());
        }
    }

    let supported_protocols = if matches!(scope.as_str(), "all" | "protocol") {
        note_parts.push("protocol".into());
        sync_detect_supported_protocols(&state, &provider).await
    } else {
        vec![provider_kind_to_api_label(provider.kind).to_string()]
    };

    if matches!(scope.as_str(), "all" | "usage") {
        if let Ok(secret) = probe_secret(&state, &provider).await {
            if let Some(secret) = secret {
                financials = sync_fetch_financials(&state.http, &provider, &secret).await;
                note_parts.push("usage".into());
            }
        }
    }

    let mut updated = if matches!(scope.as_str(), "all" | "models") {
        note_parts.push("models".into());
        refresh_remote_models_for_provider_if_supported(&state, &provider)
            .await
            .unwrap_or(provider.clone())
    } else {
        provider.clone()
    };

    updated_input.model_aliases = updated.model_aliases.clone();
    updated = run_blocking(state.clone(), {
        let id3 = id.clone();
        let input3 = updated_input.clone();
        move |s| s.db.provider_update(&id3, input3)
    })
    .await?;

    Ok(Json(ProviderSyncPreview {
        display_name: branding
            .display_name
            .clone()
            .unwrap_or_else(|| updated.name.clone()),
        avatar_url: branding
            .avatar_url
            .clone()
            .or_else(|| updated.avatar_url.clone()),
        balance: financials.balance,
        usage: financials.usage,
        supported_protocols,
        platform_guess: sync_guess_platform(&branding, &provider.base_url),
        note: if note_parts.is_empty() {
            "synced".into()
        } else {
            note_parts.join(", ")
        },
        provider: updated,
    }))
}

async fn sync_detect_supported_protocols(state: &AppState, provider: &Provider) -> Vec<String> {
    let mut out = Vec::new();
    for kind in [
        vibe_protocol::ProviderKind::OpenaiResponses,
        vibe_protocol::ProviderKind::OpenaiChat,
        vibe_protocol::ProviderKind::Anthropic,
        vibe_protocol::ProviderKind::GeminiNative,
    ] {
        let mut candidate = provider.clone();
        candidate.kind = kind;
        let result = run_protocol_probe(state, &candidate, Some(5)).await;
        if result.ok || matches!(result.status, Some(400) | Some(401) | Some(403)) {
            out.push(provider_kind_to_api_label(kind).to_string());
        }
    }
    if out.is_empty() {
        out.push(provider_kind_to_api_label(provider.kind).to_string());
    }
    out
}

fn provider_kind_to_api_label(kind: vibe_protocol::ProviderKind) -> &'static str {
    match kind {
        vibe_protocol::ProviderKind::OpenaiResponses => "openai-responses",
        vibe_protocol::ProviderKind::OpenaiChat => "openai-chat",
        vibe_protocol::ProviderKind::Anthropic => "anthropic",
        vibe_protocol::ProviderKind::GeminiNative => "gemini-native",
    }
}

async fn sync_fetch_branding(
    http: &reqwest::Client,
    raw_url: &str,
) -> anyhow::Result<SyncBranding> {
    let url = reqwest::Url::parse(raw_url)?;
    let resp = tokio::time::timeout(
        Duration::from_secs(6),
        http.get(url.clone())
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send(),
    )
    .await??;
    if !resp.status().is_success() {
        anyhow::bail!("branding fetch status {}", resp.status());
    }
    let body = resp.text().await?;
    let doc = Html::parse_document(&body);
    let meta_selector = Selector::parse("meta").unwrap();
    let title_selector = Selector::parse("title").unwrap();
    let link_selector = Selector::parse("link").unwrap();
    let mut meta_by_key = HashMap::<String, String>::new();
    for meta in doc.select(&meta_selector) {
        let value = meta.value();
        let content = value.attr("content").unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        for key in [value.attr("property"), value.attr("name")]
            .into_iter()
            .flatten()
        {
            let key = key.trim().to_ascii_lowercase();
            if !key.is_empty() {
                meta_by_key
                    .entry(key)
                    .or_insert_with(|| content.to_string());
            }
        }
    }
    let title = doc
        .select(&title_selector)
        .next()
        .map(|n| n.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty());
    let display_name = [
        "og:site_name",
        "application-name",
        "apple-mobile-web-app-title",
        "og:title",
        "twitter:title",
    ]
    .iter()
    .find_map(|k| meta_by_key.get(*k).cloned())
    .or_else(|| {
        title.clone().map(|t| {
            t.split(['|', '-', '—', '·'])
                .next()
                .unwrap_or(&t)
                .trim()
                .to_string()
        })
    });
    let avatar_url = ["og:image", "twitter:image", "msapplication-tileimage"]
        .iter()
        .find_map(|k| {
            meta_by_key
                .get(*k)
                .and_then(|v| url.join(v).ok().map(|u| u.to_string()))
        })
        .or_else(|| {
            for link in doc.select(&link_selector) {
                let rel = link.value().attr("rel").unwrap_or("").to_ascii_lowercase();
                if !(rel.contains("icon") || rel.contains("apple-touch-icon")) {
                    continue;
                }
                if let Some(href) = link.value().attr("href") {
                    if let Ok(joined) = url.join(href) {
                        return Some(joined.to_string());
                    }
                }
            }
            url.join("/favicon.ico").ok().map(|u| u.to_string())
        });
    Ok(SyncBranding {
        display_name,
        avatar_url,
        title,
    })
}

fn sync_guess_platform(branding: &SyncBranding, base_url: &str) -> Option<String> {
    let title = branding.title.as_deref().unwrap_or("");
    let avatar = branding.avatar_url.as_deref().unwrap_or("");
    if title.contains("AI API Gateway") && avatar.contains("/logo.png") {
        return Some("sub2api-like".into());
    }
    let base = base_url.to_ascii_lowercase();
    if base.contains("newapi") || base.contains("freeapi") {
        return Some("newapi-like".into());
    }
    None
}

async fn sync_fetch_financials(
    http: &reqwest::Client,
    provider: &Provider,
    secret: &str,
) -> SyncFinancials {
    if !matches!(
        provider.kind,
        vibe_protocol::ProviderKind::OpenaiChat | vibe_protocol::ProviderKind::OpenaiResponses
    ) {
        return SyncFinancials::default();
    }
    let base = provider
        .base_url
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string();
    let mut out = SyncFinancials::default();
    let headers = |req: reqwest::RequestBuilder| req.bearer_auth(secret);
    if let Ok(Some(v)) =
        send_sync_credit(headers(http.get(format!("{base}/api/user/credit_grants")))).await
    {
        out.balance = Some(v);
    }
    if let Ok(Some(v)) =
        send_sync_usage(headers(http.get(format!("{base}/dashboard/billing/usage")))).await
    {
        out.usage = Some(v);
    }
    out
}

async fn send_sync_credit(
    req: reqwest::RequestBuilder,
) -> anyhow::Result<Option<ProviderBalanceSnapshot>> {
    let resp = tokio::time::timeout(Duration::from_secs(6), req.send()).await??;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let value = resp.json::<serde_json::Value>().await?;
    let total = value
        .pointer("/total_granted")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    let used = value
        .pointer("/total_used")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    let remaining = value
        .pointer("/total_available")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    if total.is_none() && used.is_none() && remaining.is_none() {
        return Ok(None);
    }
    Ok(Some(ProviderBalanceSnapshot {
        currency: "USD".into(),
        balance: remaining.clone(),
        remaining,
        used,
        total,
        period: None,
        note: Some("credit grants".into()),
    }))
}

async fn send_sync_usage(
    req: reqwest::RequestBuilder,
) -> anyhow::Result<Option<ProviderBalanceSnapshot>> {
    let resp = tokio::time::timeout(Duration::from_secs(6), req.send()).await??;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let value = resp.json::<serde_json::Value>().await?;
    let used = value
        .get("total_usage")
        .and_then(|v| v.as_f64())
        .map(|n| n.to_string());
    if used.is_none() {
        return Ok(None);
    }
    Ok(Some(ProviderBalanceSnapshot {
        currency: "USD".into(),
        balance: None,
        remaining: None,
        used,
        total: None,
        period: Some("current window".into()),
        note: Some("usage".into()),
    }))
}

async fn probe_secret(state: &AppState, provider: &Provider) -> anyhow::Result<Option<String>> {
    let credentials = state.db.credential_list_for_provider(&provider.id)?;
    if let Some(cred) = credentials.into_iter().find(|cred| cred.enabled) {
        if let Some(token) = cred.oauth_access_token.filter(|s| !s.trim().is_empty()) {
            return Ok(Some(token));
        }
        if let Some(auth_ref) = cred.auth_ref.filter(|s| !s.trim().is_empty()) {
            return Ok(Some(crate::secrets::resolve(&auth_ref)?));
        }
    }
    match provider
        .auth_ref
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(auth_ref) => Ok(Some(crate::secrets::resolve(auth_ref)?)),
        None => Ok(None),
    }
}

fn probe_model(provider: &Provider) -> String {
    provider
        .model_aliases
        .first()
        .map(|a| a.upstream_model.clone())
        .or_else(|| provider.remote_models.first().cloned())
        .unwrap_or_else(|| match provider.kind {
            vibe_protocol::ProviderKind::Anthropic => "claude-sonnet-4-5".into(),
            vibe_protocol::ProviderKind::GeminiNative => "gemini-2.5-pro".into(),
            vibe_protocol::ProviderKind::OpenaiChat => "gpt-5.4".into(),
            vibe_protocol::ProviderKind::OpenaiResponses => "gpt-5.3-codex".into(),
        })
}

fn probe_request(provider: &Provider, model: &str) -> (Wire, Vec<u8>, Option<String>, String) {
    let base = provider.base_url.trim_end_matches('/');
    match provider.kind {
        vibe_protocol::ProviderKind::Anthropic => {
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": true
            });
            (
                Wire::Anthropic,
                serde_json::to_vec(&body).unwrap_or_default(),
                None,
                format!("{base}/v1/messages"),
            )
        }
        vibe_protocol::ProviderKind::GeminiNative => {
            let path = format!("/v1beta/models/{model}:streamGenerateContent");
            let body = serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": "ping"}]}]
            });
            (
                Wire::GeminiNative,
                serde_json::to_vec(&body).unwrap_or_default(),
                Some(path.clone()),
                format!("{base}{path}"),
            )
        }
        vibe_protocol::ProviderKind::OpenaiChat => {
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": true,
                "max_tokens": 1
            });
            (
                Wire::OpenaiChat,
                serde_json::to_vec(&body).unwrap_or_default(),
                None,
                format!("{base}/v1/chat/completions"),
            )
        }
        vibe_protocol::ProviderKind::OpenaiResponses => {
            let body = serde_json::json!({
                "model": model,
                "input": [{"role": "user", "content": "ping"}],
                "stream": true,
                "max_output_tokens": 16,
                "store": false
            });
            let path = if provider.base_url.contains("chatgpt.com/backend-api") {
                "/responses"
            } else {
                "/v1/responses"
            };
            (
                Wire::OpenaiResponses,
                serde_json::to_vec(&body).unwrap_or_default(),
                None,
                format!("{base}{path}"),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Provider health
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RollingHoursQuery {
    #[serde(default = "default_rolling_hours")]
    hours: i64,
}

fn default_rolling_hours() -> i64 {
    24
}

fn cb_state_rank(state: CbState) -> i32 {
    match state {
        CbState::Open => 3,
        CbState::HalfOpen => 2,
        CbState::Closed => 1,
    }
}

fn effective_circuit_for_provider(
    state: &AppState,
    provider_id: &str,
    credential_ids: &[String],
) -> (String, i32, bool) {
    let mut worst = state.cb.state_of(provider_id);
    let mut max_failures = state.cb.consecutive_failures(provider_id) as i32;

    for cid in credential_ids {
        let s = state.cb.state_of(cid);
        if cb_state_rank(s) > cb_state_rank(worst) {
            worst = s;
        }
        let cf = state.cb.consecutive_failures(cid) as i32;
        if cf > max_failures {
            max_failures = cf;
        }
    }

    let is_healthy = worst != CbState::Open;
    (worst.as_str().to_string(), max_failures, is_healthy)
}

fn build_provider_health_summary(
    state: &AppState,
    provider_id: &str,
    row: vibe_db::DbHealth,
    cred_ids: &[String],
    rolling_hours: i64,
    rolling: Option<vibe_protocol::ProviderStat>,
) -> ProviderHealthSummary {
    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(state, provider_id, cred_ids);
    let success_rate = if row.total_requests > 0 {
        row.total_successes as f64 / row.total_requests as f64
    } else {
        1.0
    };
    ProviderHealthSummary {
        cumulative: ProviderHealth {
            provider_id: row.provider_id,
            is_healthy,
            circuit_state,
            consecutive_failures,
            total_requests: row.total_requests,
            total_successes: row.total_successes,
            total_failures: row.total_failures,
            success_rate,
            last_success_at: row.last_success_at,
            last_failure_at: row.last_failure_at,
            last_error: row.last_error,
            avg_latency_ms: row.avg_latency_ms,
            updated_at: row.updated_at,
        },
        rolling_hours,
        rolling,
    }
}

fn credential_is_rate_limited(c: &Credential, now_secs: i64) -> bool {
    let req_exhausted = c.rl_requests_remaining == Some(0)
        && c.rl_requests_reset_at
            .map(|t| t > now_secs)
            .unwrap_or(false);
    let tok_exhausted = c.rl_tokens_remaining == Some(0)
        && c.rl_tokens_reset_at.map(|t| t > now_secs).unwrap_or(false);
    req_exhausted || tok_exhausted
}

fn build_provider_pool_summary(
    state: &AppState,
    provider: &Provider,
    credentials: Vec<Credential>,
    rolling_stats: &[vibe_db::CredentialRollingStat],
    plan_snapshots: &std::collections::HashMap<String, CredentialPlanSnapshot>,
    rolling_hours: i64,
) -> ProviderAuthPoolSummary {
    let now = chrono::Utc::now().timestamp();
    let mut total_credentials: i64 = 0;
    let mut enabled_credentials: i64 = 0;
    let mut available_credentials: i64 = 0;
    let mut rate_limited_credentials: i64 = 0;
    let mut open_circuit_credentials: i64 = 0;
    let mut statuses: Vec<CredentialPoolStatus> = Vec::new();

    let stat_map: std::collections::HashMap<&str, &vibe_db::CredentialRollingStat> = rolling_stats
        .iter()
        .map(|s| (s.credential_id.as_str(), s))
        .collect();

    let mut cred_ids: Vec<String> = Vec::new();
    let mut provider_last_error: Option<String> = None;
    for c in credentials {
        total_credentials += 1;
        if c.enabled {
            enabled_credentials += 1;
        }
        if provider_last_error.is_none() {
            provider_last_error = c.last_error.clone();
        }
        cred_ids.push(c.id.clone());
        let circuit_state = state.cb.state_of(&c.id).as_str().to_string();
        let circuit_open = circuit_state == CbState::Open.as_str();
        let circuit_open_remaining_secs = state.cb.open_remaining_secs(&c.id).map(|v| v as i64);
        if circuit_open {
            open_circuit_credentials += 1;
        }
        let plan_exhausted = plan_snapshots.get(&c.id).is_some_and(|snap| {
            let primary = snap
                .codex_primary_used_percent
                .or(snap.codex_5h_used_percent)
                .or(snap.codex_7d_used_percent)
                .unwrap_or(0.0);
            primary >= 99.95
        });
        let is_rate_limited = credential_is_rate_limited(&c, now) || plan_exhausted;
        if is_rate_limited {
            rate_limited_credentials += 1;
        }
        let credential_available = c.enabled && !circuit_open && !is_rate_limited;
        if credential_available {
            available_credentials += 1;
        }
        let stat = stat_map.get(c.id.as_str());
        statuses.push(CredentialPoolStatus {
            credential_id: c.id.clone(),
            label: c.label,
            enabled: c.enabled,
            auth_mode: if c.oauth_access_token.as_ref().is_some_and(|v| !v.is_empty()) {
                "oauth".into()
            } else {
                "auth_ref".into()
            },
            circuit_state,
            circuit_open,
            circuit_open_remaining_secs,
            consecutive_failures: state.cb.consecutive_failures(&c.id) as i32,
            is_rate_limited,
            rl_requests_remaining: c.rl_requests_remaining,
            rl_requests_reset_at: c.rl_requests_reset_at,
            rl_tokens_remaining: c.rl_tokens_remaining,
            rl_tokens_reset_at: c.rl_tokens_reset_at,
            oauth_expires_at: c.oauth_expires_at,
            last_error: c.last_error,
            last_used_at: c.last_used_at,
            rolling_requests: stat.map(|x| x.requests).unwrap_or(0),
            rolling_successes: stat.map(|x| x.successes).unwrap_or(0),
            rolling_failures: stat.map(|x| x.failures).unwrap_or(0),
            rolling_avg_latency_ms: stat.and_then(|x| x.avg_latency_ms),
        });
    }
    statuses.sort_by(|a, b| a.credential_id.cmp(&b.credential_id));
    let (provider_circuit_state, _, _) =
        effective_circuit_for_provider(state, &provider.id, &cred_ids);
    let provider_circuit_open = provider_circuit_state == CbState::Open.as_str();
    let provider_circuit_open_remaining_secs = cred_ids
        .iter()
        .filter_map(|cid| state.cb.open_remaining_secs(cid))
        .max()
        .map(|v| v as i64);

    ProviderAuthPoolSummary {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        kind: provider.kind,
        rolling_hours,
        total_credentials,
        enabled_credentials,
        available_credentials,
        rate_limited_credentials,
        open_circuit_credentials,
        provider_circuit_open_remaining_secs,
        provider_circuit_state,
        provider_circuit_open,
        provider_last_error,
        credentials: statuses,
    }
}

async fn provider_pool_summary(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProviderAuthPoolSummary>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let (provider, creds, rolling_stats, plan_snapshots) = run_blocking(state.clone(), {
        let provider_id = id.clone();
        move |s| {
            let p =
                s.db.provider_get(&provider_id)?
                    .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
            let creds = s.db.credential_list_for_provider(&provider_id)?;
            let stat = s.db.credential_stats_for_provider(&provider_id, hours)?;
            let plan_credential_ids = creds.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
            let plan_snapshots = s.db.plan_snapshot_latest_map(&plan_credential_ids)?;
            Ok::<
                (
                    Provider,
                    Vec<Credential>,
                    Vec<vibe_db::CredentialRollingStat>,
                    std::collections::HashMap<String, CredentialPlanSnapshot>,
                ),
                anyhow::Error,
            >((p, creds, stat, plan_snapshots))
        }
    })
    .await?;
    Ok(Json(build_provider_pool_summary(
        &state,
        &provider,
        creds,
        &rolling_stats,
        &plan_snapshots,
        hours,
    )))
}

async fn provider_pool_list(
    State(state): State<AppState>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<Vec<ProviderAuthPoolSummary>>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let mut out = Vec::new();
    for p in providers {
        let provider_id = p.id.clone();
        let (creds, rolling_stats, plan_snapshots) = run_blocking(state.clone(), move |s| {
            let creds = s.db.credential_list_for_provider(&provider_id)?;
            let stat = s.db.credential_stats_for_provider(&provider_id, hours)?;
            let plan_credential_ids = creds.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
            let plan_snapshots = s.db.plan_snapshot_latest_map(&plan_credential_ids)?;
            Ok::<
                (
                    Vec<Credential>,
                    Vec<vibe_db::CredentialRollingStat>,
                    std::collections::HashMap<String, CredentialPlanSnapshot>,
                ),
                anyhow::Error,
            >((creds, stat, plan_snapshots))
        })
        .await?;
        out.push(build_provider_pool_summary(
            &state,
            &p,
            creds,
            &rolling_stats,
            &plan_snapshots,
            hours,
        ));
    }
    out.sort_by(|a, b| a.provider_name.cmp(&b.provider_name));
    Ok(Json(out))
}

async fn provider_overview(
    State(state): State<AppState>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProvidersOverview>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    Ok(Json(build_providers_overview(state, hours).await?))
}

async fn build_providers_overview(
    state: AppState,
    hours: i64,
) -> anyhow::Result<ProvidersOverview> {
    let (
        providers,
        health_rows,
        mut credentials_all,
        rolling_provider_stats,
        rolling_credential_stats,
        plan_snapshots,
    ) = run_blocking(state.clone(), move |s| {
        let providers = s.db.provider_list()?;
        let health_rows = s.db.health_list()?;
        let credentials = s.db.credential_list_all()?;
        let rolling_provider_stats = s.db.per_provider_stats(hours)?;
        let rolling_credential_stats = s.db.credential_stats_all(hours)?;
        let plan_credential_ids = credentials.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        let plan_snapshots = s.db.plan_snapshot_latest_map(&plan_credential_ids)?;
        Ok::<_, anyhow::Error>((
            providers,
            health_rows,
            credentials,
            rolling_provider_stats,
            rolling_credential_stats,
            plan_snapshots,
        ))
    })
    .await?;

    crate::oauth_identity::credentials_attach_oauth_identities(&mut credentials_all);

    let mut health_by_provider: HashMap<String, vibe_db::DbHealth> = health_rows
        .into_iter()
        .map(|r| (r.provider_id.clone(), r))
        .collect();
    let mut credentials_by_provider: HashMap<String, Vec<Credential>> = HashMap::new();
    let mut credential_ids_by_provider: HashMap<String, Vec<String>> = HashMap::new();
    for c in credentials_all {
        credential_ids_by_provider
            .entry(c.provider_id.clone())
            .or_default()
            .push(c.id.clone());
        credentials_by_provider
            .entry(c.provider_id.clone())
            .or_default()
            .push(c);
    }

    let mut rolling_by_provider: HashMap<String, vibe_protocol::ProviderStat> =
        rolling_provider_stats
            .into_iter()
            .map(|s| (s.provider_id.clone(), s))
            .collect();
    let mut rolling_by_credential: HashMap<String, vibe_db::CredentialRollingStat> =
        rolling_credential_stats
            .into_iter()
            .map(|s| (s.credential_id.clone(), s))
            .collect();
    let official_provider_ids: HashSet<String> = providers
        .iter()
        .filter(|p| crate::router::provider_is_chatgpt_codex_official(p))
        .map(|p| p.id.clone())
        .collect();
    let mut health = Vec::with_capacity(providers.len());
    let mut pools = Vec::with_capacity(providers.len());
    let mut codex_plans: HashMap<String, Vec<ProviderCodexPlanItem>> = HashMap::new();

    for p in &providers {
        let creds = credentials_by_provider
            .get(&p.id)
            .cloned()
            .unwrap_or_default();
        let cred_ids = credential_ids_by_provider
            .get(&p.id)
            .cloned()
            .unwrap_or_default();
        let row = health_by_provider
            .remove(&p.id)
            .unwrap_or_else(|| vibe_db::DbHealth {
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
        health.push(build_provider_health_summary(
            &state,
            &p.id,
            row,
            &cred_ids,
            hours,
            rolling_by_provider.remove(&p.id),
        ));

        let credential_stats = creds
            .iter()
            .filter_map(|c| rolling_by_credential.remove(&c.id))
            .collect::<Vec<_>>();
        pools.push(build_provider_pool_summary(
            &state,
            p,
            creds.clone(),
            &credential_stats,
            &plan_snapshots,
            hours,
        ));

        if official_provider_ids.contains(&p.id) {
            codex_plans.insert(
                p.id.clone(),
                creds
                    .into_iter()
                    .map(|c| ProviderCodexPlanItem {
                        credential_id: c.id.clone(),
                        label: c.label,
                        plan: plan_snapshots.get(&c.id).cloned(),
                    })
                    .collect(),
            );
        }
    }
    pools.sort_by(|a, b| a.provider_name.cmp(&b.provider_name));

    Ok(ProvidersOverview {
        rolling_hours: hours,
        providers,
        health,
        pools,
        credentials: credentials_by_provider,
        codex_plans,
    })
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn emit_app_log(state: &AppState, level: AppLogLevel, category: &str, message: String) {
    let ev = AppLogEvent {
        ts: now_secs(),
        level,
        category: category.to_string(),
        message,
        detail: None,
    };
    state.ws.publish(WsEvent::AppLog(ev.clone()));
    let state2 = state.clone();
    tokio::task::spawn_blocking(move || {
        let _ = state2.db.app_log_insert(&ev);
    });
}

fn publish_providers_overview_soon(state: AppState) {
    if state
        .providers_overview_publish_pending
        .swap(true, Ordering::Relaxed)
    {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(350)).await;
        for hours in [1, default_rolling_hours()] {
            match build_providers_overview(state.clone(), hours).await {
                Ok(overview) => state
                    .ws
                    .publish(WsEvent::ProvidersOverviewChanged(overview)),
                Err(e) => tracing::warn!(?e, hours, "build providers overview ws event failed"),
            }
        }
        state
            .providers_overview_publish_pending
            .store(false, Ordering::Relaxed);
    });
}

async fn provider_health(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<ProviderHealthSummary>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;

    let db_row = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let row = s.db.health_get(&id2)?;
            Ok(row)
        }
    })
    .await?
    .unwrap_or_else(|| vibe_db::DbHealth {
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
    });
    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(&state, &id, &cred_ids);

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    let cumulative = ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy,
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
    };

    let rolling = run_blocking(state.clone(), move |s| {
        s.db.provider_stat_single(&id, hours)
    })
    .await?;

    Ok(Json(ProviderHealthSummary {
        cumulative,
        rolling_hours: hours,
        rolling,
    }))
}

async fn provider_health_list(
    State(state): State<AppState>,
    Query(q): Query<RollingHoursQuery>,
) -> Result<Json<Vec<ProviderHealthSummary>>, AppError> {
    let hours = q.hours.clamp(1, 24 * 30);
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let rows = run_blocking(state.clone(), |s| s.db.health_list()).await?;
    let creds_all = run_blocking(state.clone(), |s| s.db.credential_list_all()).await?;
    let rolling_stats =
        run_blocking(state.clone(), move |s| s.db.per_provider_stats(hours)).await?;

    let mut row_by_provider: std::collections::HashMap<String, vibe_db::DbHealth> = rows
        .into_iter()
        .map(|r| (r.provider_id.clone(), r))
        .collect();
    let mut cred_ids_by_provider: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for c in creds_all {
        cred_ids_by_provider
            .entry(c.provider_id)
            .or_default()
            .push(c.id);
    }
    let mut rolling_by_provider = rolling_stats
        .into_iter()
        .map(|s| (s.provider_id.clone(), s))
        .collect::<std::collections::HashMap<_, _>>();

    let mut out = Vec::with_capacity(providers.len());
    for p in providers {
        let row = row_by_provider
            .remove(&p.id)
            .unwrap_or_else(|| vibe_db::DbHealth {
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
        let cred_ids = cred_ids_by_provider.get(&p.id).cloned().unwrap_or_default();
        let (circuit_state, consecutive_failures, is_healthy) =
            effective_circuit_for_provider(&state, &p.id, &cred_ids);
        let success_rate = if row.total_requests > 0 {
            row.total_successes as f64 / row.total_requests as f64
        } else {
            1.0
        };
        out.push(ProviderHealthSummary {
            cumulative: ProviderHealth {
                provider_id: row.provider_id,
                is_healthy,
                circuit_state,
                consecutive_failures,
                total_requests: row.total_requests,
                total_successes: row.total_successes,
                total_failures: row.total_failures,
                success_rate,
                last_success_at: row.last_success_at,
                last_failure_at: row.last_failure_at,
                last_error: row.last_error,
                avg_latency_ms: row.avg_latency_ms,
                updated_at: row.updated_at,
            },
            rolling_hours: hours,
            rolling: rolling_by_provider.remove(&p.id),
        });
    }

    Ok(Json(out))
}

async fn provider_circuit_reset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProviderHealth>, AppError> {
    let cred_ids = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| {
            let creds = s.db.credential_list_for_provider(&id2)?;
            Ok::<Vec<String>, anyhow::Error>(creds.into_iter().map(|c| c.id).collect())
        }
    })
    .await?;
    if state.cb.reset(&id).is_some() {
        emit_app_log(
            &state,
            AppLogLevel::Info,
            "circuit",
            format!("Circuit reset: {id}"),
        );
    }
    for cid in &cred_ids {
        state.cb.reset(cid);
    }
    let db_row = run_blocking(state.clone(), {
        let id2 = id.clone();
        move |s| s.db.health_get(&id2)
    })
    .await?
    .unwrap_or_else(|| vibe_db::DbHealth {
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
    });

    let success_rate = if db_row.total_requests > 0 {
        db_row.total_successes as f64 / db_row.total_requests as f64
    } else {
        1.0
    };

    let (circuit_state, consecutive_failures, is_healthy) =
        effective_circuit_for_provider(&state, &id, &cred_ids);
    let out = ProviderHealth {
        provider_id: db_row.provider_id,
        is_healthy,
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
    };
    publish_providers_overview_soon(state);
    Ok(Json(out))
}

async fn health_all_providers(
    State(state): State<AppState>,
) -> Result<Json<HealthSummary>, AppError> {
    let rows = run_blocking(state.clone(), |s| s.db.health_list()).await?;
    let providers = run_blocking(state.clone(), |s| s.db.provider_list()).await?;
    let creds_all = run_blocking(state.clone(), |s| s.db.credential_list_all()).await?;
    let mut cred_ids_by_provider: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for c in creds_all {
        cred_ids_by_provider
            .entry(c.provider_id)
            .or_default()
            .push(c.id);
    }

    // Build health entries for every known provider (even those never hit)
    let mut health_map: std::collections::HashMap<String, vibe_db::DbHealth> = rows
        .into_iter()
        .map(|r| (r.provider_id.clone(), r))
        .collect();

    for p in &providers {
        health_map
            .entry(p.id.clone())
            .or_insert_with(|| vibe_db::DbHealth {
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
            let cred_ids = cred_ids_by_provider
                .get(&row.provider_id)
                .cloned()
                .unwrap_or_default();
            let (cs, cf, is_healthy) =
                effective_circuit_for_provider(&state, &row.provider_id, &cred_ids);
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
// Credential CRUD
// ---------------------------------------------------------------------------

async fn list_credentials(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<Credential>>, AppError> {
    let mut v = run_blocking(state, move |s| {
        s.db.credential_list_for_provider(&provider_id)
    })
    .await?;
    crate::oauth_identity::credentials_attach_oauth_identities(&mut v);
    Ok(Json(v))
}

async fn list_credentials_all(
    State(state): State<AppState>,
) -> Result<Json<std::collections::HashMap<String, Vec<Credential>>>, AppError> {
    let mut creds = run_blocking(state, move |s| s.db.credential_list_all()).await?;
    crate::oauth_identity::credentials_attach_oauth_identities(&mut creds);
    let mut out: std::collections::HashMap<String, Vec<Credential>> =
        std::collections::HashMap::new();
    for cred in creds {
        out.entry(cred.provider_id.clone()).or_default().push(cred);
    }
    Ok(Json(out))
}

async fn create_credential(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<Credential>, AppError> {
    let label = input.label.clone();
    let fp = crate::auth_fingerprint::credential_fingerprint(
        input.auth_ref.as_deref(),
        input.oauth_access_token.as_deref(),
    );
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_insert(&provider_id, input, Some(fp))
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_log(
        &state,
        AppLogLevel::Info,
        "credential",
        format!("Credential added: {label}"),
    );
    publish_providers_overview_soon(state);
    Ok(Json(c))
}

async fn get_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    let mut c = run_blocking(state, move |s| s.db.credential_get(&id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    Ok(Json(c))
}

async fn update_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> Result<Json<Credential>, AppError> {
    let label = input.label.clone();
    let fp = crate::auth_fingerprint::credential_fingerprint(
        input.auth_ref.as_deref(),
        input.oauth_access_token.as_deref(),
    );
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_update(&id, input, Some(fp))
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_log(
        &state,
        AppLogLevel::Info,
        "credential",
        format!("Credential updated: {label}"),
    );
    publish_providers_overview_soon(state);
    Ok(Json(c))
}

async fn credential_plan_latest(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<CredentialPlanSnapshot>>, AppError> {
    let snap = run_blocking(state, move |s| s.db.plan_snapshot_latest(&id)).await?;
    Ok(Json(snap))
}

async fn refresh_codex_plan_for_credential(
    state: &AppState,
    cred: &Credential,
) -> anyhow::Result<()> {
    let Some(access) = cred.oauth_access_token.as_ref().filter(|t| !t.is_empty()) else {
        anyhow::bail!("credential has no OAuth access token");
    };
    let oauth = crate::forward::CredOAuth {
        access_token: access.clone(),
        expires_at: cred.oauth_expires_at,
    };
    let token = crate::forward::resolve_oauth_token(state, Some(cred.id.as_str()), oauth).await?;
    let acct = crate::auth_fingerprint::chatgpt_account_id_from_access_token(&token);
    let snap = crate::codex_wham_usage::fetch_wham_plan_snapshot(
        &state.http,
        &token,
        acct.as_deref(),
        &cred.id,
    )
    .await?;
    let db = state.db.clone();
    let snap_ins = snap.clone();
    tokio::task::spawn_blocking(move || db.plan_snapshot_insert(&snap_ins)).await??;
    Ok(())
}

async fn credential_plan_refresh(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CredentialPlanSnapshot>, AppError> {
    let cred_opt = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.credential_get(&id)
    })
    .await?;
    let cred = cred_opt.ok_or_else(|| anyhow::anyhow!("credential not found"))?;
    let pid = cred.provider_id.clone();
    let provider_opt = run_blocking(state.clone(), move |s| s.db.provider_get(&pid)).await?;
    let provider = provider_opt.ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    if !crate::router::provider_is_chatgpt_codex_official(&provider) {
        return Err(anyhow::anyhow!(
            "Not a ChatGPT Codex official provider (chatgpt.com … /backend-api/…/codex)."
        )
        .into());
    }
    refresh_codex_plan_for_credential(&state, &cred).await?;
    let snap_opt = run_blocking(state.clone(), {
        let id = id.clone();
        move |s| s.db.plan_snapshot_latest(&id)
    })
    .await?;
    let snap = snap_opt.ok_or_else(|| anyhow::anyhow!("plan snapshot missing after refresh"))?;
    publish_providers_overview_soon(state);
    Ok(Json(snap))
}

async fn provider_codex_plan_list(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<Vec<ProviderCodexPlanItem>>, AppError> {
    let pid = provider_id.clone();
    let items = run_blocking(state.clone(), move |s| {
        let p =
            s.db.provider_get(&pid)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if !crate::router::provider_is_chatgpt_codex_official(&p) {
            return Ok(Vec::new());
        }
        let creds = s.db.credential_list_for_provider(&pid)?;
        let mut out = Vec::new();
        for c in creds {
            let plan = s.db.plan_snapshot_latest(&c.id)?;
            out.push(ProviderCodexPlanItem {
                credential_id: c.id,
                label: c.label,
                plan,
            });
        }
        Ok(out)
    })
    .await?;
    Ok(Json(items))
}

async fn provider_codex_plan_list_all(
    State(state): State<AppState>,
) -> Result<Json<std::collections::HashMap<String, Vec<ProviderCodexPlanItem>>>, AppError> {
    let items = run_blocking(state.clone(), move |s| {
        let providers = s.db.provider_list()?;
        let official_provider_ids = providers
            .into_iter()
            .filter(crate::router::provider_is_chatgpt_codex_official)
            .map(|p| p.id)
            .collect::<std::collections::HashSet<_>>();
        if official_provider_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let creds = s.db.credential_list_all()?;
        let credential_ids = creds
            .iter()
            .filter(|c| official_provider_ids.contains(&c.provider_id))
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let plans = s.db.plan_snapshot_latest_map(&credential_ids)?;
        let mut out: std::collections::HashMap<String, Vec<ProviderCodexPlanItem>> =
            std::collections::HashMap::new();

        for c in creds {
            if !official_provider_ids.contains(&c.provider_id) {
                continue;
            }
            out.entry(c.provider_id.clone())
                .or_default()
                .push(ProviderCodexPlanItem {
                    credential_id: c.id.clone(),
                    label: c.label,
                    plan: plans.get(&c.id).cloned(),
                });
        }

        Ok(out)
    })
    .await?;
    Ok(Json(items))
}

async fn provider_codex_plan_refresh_all(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> Result<Json<CodexPlanRefreshResult>, AppError> {
    let creds = run_blocking(state.clone(), move |s| {
        let p =
            s.db.provider_get(&provider_id)?
                .ok_or_else(|| anyhow::anyhow!("provider not found"))?;
        if !crate::router::provider_is_chatgpt_codex_official(&p) {
            anyhow::bail!("not a ChatGPT Codex official provider");
        }
        s.db.credential_list_for_provider(&provider_id)
    })
    .await?;

    let mut attempted = 0usize;
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for c in creds {
        if c.oauth_access_token
            .as_ref()
            .map_or(true, |t: &String| t.is_empty())
        {
            continue;
        }
        attempted += 1;
        match refresh_codex_plan_for_credential(&state, &c).await {
            Err(e) => errors.push(format!("{}: {e}", c.label)),
            Ok(()) => ok += 1,
        }
    }

    publish_providers_overview_soon(state);
    Ok(Json(CodexPlanRefreshResult {
        attempted,
        ok,
        errors,
    }))
}

async fn delete_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let id_clone = id.clone();
    run_blocking(state.clone(), move |s| s.db.credential_delete(&id_clone)).await?;
    emit_app_log(
        &state,
        AppLogLevel::Warn,
        "credential",
        format!("Credential deleted: {id}"),
    );
    publish_providers_overview_soon(state);
    Ok(StatusCode::NO_CONTENT)
}

async fn enable_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    state.cb.reset(&id);
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_set_enabled(&id, true)
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_log(
        &state,
        AppLogLevel::Info,
        "credential",
        format!("Credential enabled: {}", c.label),
    );
    publish_providers_overview_soon(state);
    Ok(Json(c))
}

async fn disable_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Credential>, AppError> {
    state.cb.reset(&id);
    let mut c = run_blocking(state.clone(), move |s| {
        s.db.credential_set_enabled(&id, false)
    })
    .await?;
    crate::oauth_identity::credential_attach_oauth_identity(&mut c);
    emit_app_log(
        &state,
        AppLogLevel::Warn,
        "credential",
        format!("Credential disabled: {}", c.label),
    );
    publish_providers_overview_soon(state);
    Ok(Json(c))
}

async fn credential_circuit_reset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if state.cb.reset(&id).is_some() {
        emit_app_log(
            &state,
            AppLogLevel::Info,
            "circuit",
            format!("Circuit reset: {id}"),
        );
    }
    publish_providers_overview_soon(state);
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// App logs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AppLogQuery {
    limit: Option<i64>,
    since: Option<i64>,
}

async fn list_app_logs(
    State(state): State<AppState>,
    Query(q): Query<AppLogQuery>,
) -> Result<Json<Vec<AppLogEvent>>, AppError> {
    let limit = q.limit.unwrap_or(200).clamp(1, 500);
    let since = q.since;
    let logs = run_blocking(state, move |s| s.db.app_log_list(limit, since)).await?;
    Ok(Json(logs))
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

#[derive(Debug, Deserialize)]
struct AttemptQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn get_request_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let log = run_blocking(state, move |s| s.db.log_get(&id)).await?;
    Ok(match log {
        Some(log) => Json(log).into_response(),
        None => (StatusCode::NOT_FOUND, "log not found").into_response(),
    })
}

async fn get_upstream_attempt(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let attempt = run_blocking(state, move |s| s.db.upstream_attempt_get(&id)).await?;
    Ok(match attempt {
        Some(attempt) => Json(attempt).into_response(),
        None => (StatusCode::NOT_FOUND, "attempt not found").into_response(),
    })
}

async fn list_request_attempts(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<UpstreamAttemptLog>>, AppError> {
    let attempts = run_blocking(state, move |s| s.db.upstream_attempts_for_request(&id)).await?;
    Ok(Json(attempts))
}

async fn list_upstream_attempts(
    State(state): State<AppState>,
    Query(q): Query<AttemptQuery>,
) -> Result<Json<Vec<UpstreamAttemptLog>>, AppError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    let attempts = run_blocking(state, move |s| s.db.upstream_attempt_list(limit, offset)).await?;
    Ok(Json(attempts))
}

#[derive(Debug, serde::Serialize)]
struct LogStreamTraceResponse {
    id: String,
    stream_kind: Option<String>,
    stream_terminal_seen: Option<bool>,
    stream_end_reason: Option<String>,
    stream_error_detail: Option<String>,
    upstream_first_byte_ms: Option<i64>,
    client_first_write_ms: Option<i64>,
    last_upstream_event_ms: Option<i64>,
    last_client_write_ms: Option<i64>,
    upstream_chunk_count: i64,
    upstream_bytes: i64,
    client_chunk_count: i64,
    client_bytes: i64,
    sse_event_count: i64,
    sse_data_count: i64,
    sse_comment_count: i64,
    sse_keepalive_count: i64,
    sse_done_count: i64,
    parse_error_count: i64,
    first_keepalive_ms: Option<i64>,
    last_keepalive_ms: Option<i64>,
    max_gap_between_upstream_events_ms: Option<i64>,
    max_gap_between_data_events_ms: Option<i64>,
    keepalive_after_last_data_count: i64,
    last_data_event_ms: Option<i64>,
    bridge_mode: Option<String>,
    status_injected: bool,
    terminal_injected: bool,
    upstream_terminal_type: Option<String>,
    verdict: String,
}

async fn get_log_stream_trace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let log = run_blocking(state, move |s| s.db.log_get(&id)).await?;
    let Some(log) = log else {
        return Ok((StatusCode::NOT_FOUND, "log not found").into_response());
    };
    let verdict = if log.stream_kind.as_deref() == Some("none") || log.stream_kind.is_none() {
        "not_streaming"
    } else if log.stream_terminal_seen == Some(true) {
        "completed"
    } else if matches!(
        log.stream_end_reason.as_deref(),
        Some("downstream_closed") | Some("downstream_write_error")
    ) {
        "client_or_downstream_closed"
    } else if matches!(
        log.stream_end_reason.as_deref(),
        Some("upstream_read_error") | Some("upstream_eof") | Some("truncated")
    ) {
        "upstream_or_proxy_truncated"
    } else if log.sse_keepalive_count > 0 && log.sse_data_count == 0 {
        "keepalive_only"
    } else {
        "unknown"
    };
    Ok(Json(LogStreamTraceResponse {
        id: log.id,
        stream_kind: log.stream_kind,
        stream_terminal_seen: log.stream_terminal_seen,
        stream_end_reason: log.stream_end_reason,
        stream_error_detail: log.stream_error_detail,
        upstream_first_byte_ms: log.upstream_first_byte_ms,
        client_first_write_ms: log.client_first_write_ms,
        last_upstream_event_ms: log.last_upstream_event_ms,
        last_client_write_ms: log.last_client_write_ms,
        upstream_chunk_count: log.upstream_chunk_count,
        upstream_bytes: log.upstream_bytes,
        client_chunk_count: log.client_chunk_count,
        client_bytes: log.client_bytes,
        sse_event_count: log.sse_event_count,
        sse_data_count: log.sse_data_count,
        sse_comment_count: log.sse_comment_count,
        sse_keepalive_count: log.sse_keepalive_count,
        sse_done_count: log.sse_done_count,
        parse_error_count: log.parse_error_count,
        first_keepalive_ms: log.first_keepalive_ms,
        last_keepalive_ms: log.last_keepalive_ms,
        max_gap_between_upstream_events_ms: log.max_gap_between_upstream_events_ms,
        max_gap_between_data_events_ms: log.max_gap_between_data_events_ms,
        keepalive_after_last_data_count: log.keepalive_after_last_data_count,
        last_data_event_ms: log.last_data_event_ms,
        bridge_mode: log.bridge_mode,
        status_injected: log.status_injected,
        terminal_injected: log.terminal_injected,
        upstream_terminal_type: log.upstream_terminal_type,
        verdict: verdict.into(),
    })
    .into_response())
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
    Ok(Json(build_dashboard_stats(state, hours).await?))
}

async fn build_dashboard_stats(state: AppState, hours: i64) -> anyhow::Result<DashboardStats> {
    let stats = run_blocking(state, move |s| {
        let now = chrono::Utc::now().timestamp();
        let since_window = now - hours * 3600;
        let since_1h = now - 3600;

        let requests_last_hour = s.db.count_logs_since(since_1h)?;
        let requests_last_24h = s.db.count_logs_since(now - 86400)?;

        let (ok_window, total_window) = s.db.ok_total_since(since_window)?;
        let (ok_1h, total_1h) = s.db.ok_total_since(since_1h)?;
        let success_rate_in_window = if total_window == 0 {
            1.0
        } else {
            ok_window as f64 / total_window as f64
        };
        let success_rate_last_hour = if total_1h == 0 {
            1.0
        } else {
            ok_1h as f64 / total_1h as f64
        };

        let (p50, p95) = s.db.latency_percentiles(hours)?;
        let top_models = s.db.top_models(hours, 10)?;
        let per_provider = s.db.per_provider_stats(hours)?;
        let output_tokens_per_sec_in_window = s.db.output_tokens_per_sec(hours)?;
        let decode_output_tokens_per_sec_in_window = s.db.decode_output_tokens_per_sec(hours)?;
        let summary_window = s.db.usage_summary_last_hours(hours)?;
        let summary_24h = s.db.usage_summary_last_hours(24)?;

        let window_label = match hours {
            1 => "Last 1 hour".to_string(),
            5 => "Last 5 hours".to_string(),
            24 => "Last 24 hours".to_string(),
            168 => "Last 7 days".to_string(),
            720 => "Last 30 days".to_string(),
            h if h % 24 == 0 && h > 24 => format!("Last {} days", h / 24),
            h => format!("Last {h} hours"),
        };

        Ok(vibe_protocol::DashboardStats {
            window_hours: hours,
            window_label,
            requests_in_window: summary_window.requests,
            success_rate_in_window,
            input_tokens_in_window: summary_window.input_tokens,
            output_tokens_in_window: summary_window.output_tokens,
            output_tokens_per_sec_in_window,
            decode_output_tokens_per_sec_in_window,
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
    Ok(stats)
}

pub(crate) fn publish_dashboard_stats_soon(state: AppState) {
    if state
        .dashboard_stats_publish_pending
        .swap(true, Ordering::Relaxed)
    {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        for hours in [1, 24] {
            match build_dashboard_stats(state.clone(), hours).await {
                Ok(stats) => state.ws.publish(WsEvent::DashboardStatsChanged(stats)),
                Err(e) => tracing::warn!(?e, hours, "build dashboard stats ws event failed"),
            }
        }
        state
            .dashboard_stats_publish_pending
            .store(false, Ordering::Relaxed);
    });
}

#[derive(Debug, serde::Serialize)]
struct ToolConfigRawResponse {
    tool: String,
    path: String,
    exists: bool,
    mtime_ms: Option<i64>,
    raw_text: String,
}

#[derive(Debug, Deserialize)]
struct ToolConfigRawUpdateInput {
    raw_text: String,
}

fn resolve_tool_config_path(tool: &str) -> anyhow::Result<PathBuf> {
    match tool {
        "codex" => Ok(crate::codex_config::codex_config_path()),
        "claude" => crate::paths::claude_settings_path(),
        _ => anyhow::bail!("unsupported tool: {tool}"),
    }
}

fn file_mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

async fn get_tool_config_raw(Path(tool): Path<String>) -> Response {
    let path = match resolve_tool_config_path(&tool) {
        Ok(p) => p,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    let read_result = tokio::task::spawn_blocking({
        let path = path.clone();
        move || -> anyhow::Result<(bool, Option<i64>, String)> {
            if !path.exists() {
                return Ok((false, None, String::new()));
            }
            let meta = std::fs::metadata(&path)?;
            let raw = std::fs::read_to_string(&path)?;
            Ok((true, file_mtime_ms(&meta), raw))
        }
    })
    .await;

    let (exists, mtime_ms, raw_text) = match read_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(ToolConfigRawResponse {
        tool,
        path: path.to_string_lossy().to_string(),
        exists,
        mtime_ms,
        raw_text,
    })
    .into_response()
}

async fn put_tool_config_raw(
    Path(tool): Path<String>,
    Json(input): Json<ToolConfigRawUpdateInput>,
) -> Response {
    let path = match resolve_tool_config_path(&tool) {
        Ok(p) => p,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    if tool == "codex" && toml::from_str::<toml::Value>(&input.raw_text).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid TOML in codex config").into_response();
    }
    if tool == "claude" && serde_json::from_str::<serde_json::Value>(&input.raw_text).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid JSON in claude settings").into_response();
    }

    let write_result = tokio::task::spawn_blocking({
        let path = path.clone();
        let raw = input.raw_text.clone();
        move || -> anyhow::Result<(bool, Option<i64>, String)> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            let meta = std::fs::metadata(&path)?;
            let saved = std::fs::read_to_string(&path)?;
            Ok((true, file_mtime_ms(&meta), saved))
        }
    })
    .await;

    let (exists, mtime_ms, raw_text) = match write_result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    Json(ToolConfigRawResponse {
        tool,
        path: path.to_string_lossy().to_string(),
        exists,
        mtime_ms,
        raw_text,
    })
    .into_response()
}

async fn get_codex_config_settings() -> Result<Json<CodexConfigSettings>, AppError> {
    let path = crate::codex_config::codex_config_path();
    let settings =
        tokio::task::spawn_blocking(move || crate::codex_config::read_settings(&path)).await??;
    Ok(Json(settings))
}

async fn put_codex_config_settings(
    Json(input): Json<CodexConfigSettingsInput>,
) -> Result<Json<CodexConfigSettings>, AppError> {
    let path = crate::codex_config::codex_config_path();
    let settings =
        tokio::task::spawn_blocking(move || crate::codex_config::write_settings(&path, input))
            .await??;
    Ok(Json(settings))
}

#[derive(Debug, Deserialize)]
struct CodexHistoryPreviewQuery {
    provider: Option<String>,
}

async fn get_codex_history_preview(
    Query(query): Query<CodexHistoryPreviewQuery>,
) -> Result<Json<vibe_protocol::CodexHistorySummary>, AppError> {
    let provider = query
        .provider
        .unwrap_or_else(|| crate::codex_history::DEFAULT_PROVIDER_ID.to_string());
    let summary = tokio::task::spawn_blocking(move || {
        crate::codex_history::unify(vibe_protocol::CodexHistoryUnifyInput {
            provider,
            from_providers: Vec::new(),
            apply: false,
            no_backup: false,
            codex_home: None,
        })
    })
    .await??;
    Ok(Json(summary))
}

async fn post_codex_history_unify(
    Json(mut input): Json<vibe_protocol::CodexHistoryUnifyInput>,
) -> Result<Json<vibe_protocol::CodexHistorySummary>, AppError> {
    input.apply = true;
    let summary = tokio::task::spawn_blocking(move || crate::codex_history::unify(input)).await??;
    Ok(Json(summary))
}

type CodexAppStatusResponse = CodexAppStatus;
type CodexAppActionResponse = CodexAppActionResult;

async fn get_codex_app_status() -> Result<Json<CodexAppStatusResponse>, AppError> {
    let status = tokio::task::spawn_blocking(codex_app_status).await??;
    Ok(Json(status))
}

async fn post_codex_app_open(
    State(state): State<AppState>,
) -> Result<Json<CodexAppActionResponse>, AppError> {
    let response = tokio::task::spawn_blocking(|| -> anyhow::Result<CodexAppActionResponse> {
        open_codex_app()?;
        std::thread::sleep(Duration::from_millis(450));
        Ok(CodexAppActionResponse {
            action: "open".into(),
            status: codex_app_status()?,
        })
    })
    .await??;
    state
        .ws
        .publish(WsEvent::CodexAppStatusChanged(response.status.clone()));
    Ok(Json(response))
}

async fn post_codex_app_quit(
    State(state): State<AppState>,
) -> Result<Json<CodexAppActionResponse>, AppError> {
    let response = tokio::task::spawn_blocking(|| -> anyhow::Result<CodexAppActionResponse> {
        quit_codex_app()?;
        std::thread::sleep(Duration::from_millis(450));
        Ok(CodexAppActionResponse {
            action: "quit".into(),
            status: codex_app_status()?,
        })
    })
    .await??;
    state
        .ws
        .publish(WsEvent::CodexAppStatusChanged(response.status.clone()));
    Ok(Json(response))
}

async fn post_codex_app_restart(
    State(state): State<AppState>,
) -> Result<Json<CodexAppActionResponse>, AppError> {
    let response = tokio::task::spawn_blocking(|| -> anyhow::Result<CodexAppActionResponse> {
        quit_codex_app()?;
        std::thread::sleep(Duration::from_millis(900));
        open_codex_app()?;
        std::thread::sleep(Duration::from_millis(450));
        Ok(CodexAppActionResponse {
            action: "restart".into(),
            status: codex_app_status()?,
        })
    })
    .await??;
    state
        .ws
        .publish(WsEvent::CodexAppStatusChanged(response.status.clone()));
    Ok(Json(response))
}

fn codex_app_path() -> PathBuf {
    PathBuf::from("/Applications/Codex.app")
}

#[cfg(target_os = "macos")]
fn codex_app_status() -> anyhow::Result<CodexAppStatusResponse> {
    let app_path = codex_app_path();
    let output = std::process::Command::new("ps")
        .args(["ax", "-o", "pid=,args="])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut processes = Vec::new();
    let mut main_pid = None;
    for line in stdout.lines() {
        let Some((pid_raw, command)) = line.trim().split_once(' ') else {
            continue;
        };
        if !command.contains("/Applications/Codex.app") {
            continue;
        }
        let Ok(pid) = pid_raw.parse::<u32>() else {
            continue;
        };
        let role = classify_codex_app_process(command);
        if role == "main" {
            main_pid = Some(pid);
        }
        processes.push(CodexAppProcess {
            pid,
            role: role.into(),
            command: command.into(),
        });
    }
    processes.sort_by_key(|p| (p.role != "main", p.pid));
    Ok(CodexAppStatusResponse {
        app_path: app_path.display().to_string(),
        installed: app_path.exists(),
        running: main_pid.is_some(),
        main_pid,
        process_count: processes.len(),
        processes,
    })
}

#[cfg(target_os = "macos")]
fn classify_codex_app_process(command: &str) -> &'static str {
    if command.starts_with("/Applications/Codex.app/Contents/MacOS/Codex") {
        "main"
    } else if command.contains("chrome_crashpad_handler") {
        "crashpad"
    } else if command.contains(" app-server") {
        "app-server"
    } else if command.contains("Helper (Renderer)") || command.contains("--type=renderer") {
        "renderer"
    } else if command.contains("Helper") {
        "helper"
    } else {
        "support"
    }
}

#[cfg(not(target_os = "macos"))]
fn codex_app_status() -> anyhow::Result<CodexAppStatusResponse> {
    let app_path = codex_app_path();
    Ok(CodexAppStatusResponse {
        app_path: app_path.display().to_string(),
        installed: app_path.exists(),
        running: false,
        main_pid: None,
        process_count: 0,
        processes: Vec::new(),
    })
}

#[cfg(target_os = "macos")]
fn open_codex_app() -> anyhow::Result<()> {
    let status = std::process::Command::new("open")
        .args(["-a", "Codex"])
        .status()?;
    anyhow::ensure!(status.success(), "failed to open Codex.app");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open_codex_app() -> anyhow::Result<()> {
    anyhow::bail!("Codex App control is currently implemented for macOS")
}

#[cfg(target_os = "macos")]
fn quit_codex_app() -> anyhow::Result<()> {
    let status = std::process::Command::new("osascript")
        .args(["-e", r#"tell application "Codex" to quit"#])
        .status()?;
    anyhow::ensure!(status.success(), "failed to quit Codex.app");
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn quit_codex_app() -> anyhow::Result<()> {
    anyhow::bail!("Codex App control is currently implemented for macOS")
}

#[derive(Debug, Deserialize)]
struct CodexFilePathQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexFileWriteInput {
    path: String,
    raw_text: String,
}

#[derive(Debug, Deserialize)]
struct CodexDirCreateInput {
    path: String,
}

#[derive(Debug, Deserialize)]
struct CodexFileMoveInput {
    from: String,
    to: String,
    overwrite: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct CodexFileEntry {
    name: String,
    path: String,
    kind: String,
    size: Option<u64>,
    mtime_ms: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
struct CodexFileListResponse {
    root: String,
    path: String,
    abs_path: String,
    entries: Vec<CodexFileEntry>,
}

#[derive(Debug, serde::Serialize)]
struct CodexFileResponse {
    root: String,
    path: String,
    abs_path: String,
    exists: bool,
    mtime_ms: Option<i64>,
    raw_text: String,
}

async fn list_codex_files(
    Query(q): Query<CodexFilePathQuery>,
) -> Result<Json<CodexFileListResponse>, AppError> {
    let rel = q.path.unwrap_or_default();
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &rel)?;
    let (mtime_path, entries) = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        move || -> anyhow::Result<(String, Vec<CodexFileEntry>)> {
            if !path.exists() {
                return Ok((relative_codex_path(&root, &path), Vec::new()));
            }
            if !path.is_dir() {
                anyhow::bail!("path is not a directory");
            }
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                let p = entry.path();
                let meta = entry.metadata()?;
                entries.push(CodexFileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: relative_codex_path(&root, &p),
                    kind: if meta.is_dir() { "dir" } else { "file" }.into(),
                    size: meta.is_file().then_some(meta.len()),
                    mtime_ms: file_mtime_ms(&meta),
                });
            }
            entries.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
            Ok((relative_codex_path(&root, &path), entries))
        }
    })
    .await??;
    Ok(Json(CodexFileListResponse {
        root: root.display().to_string(),
        path: mtime_path,
        abs_path: path.display().to_string(),
        entries,
    }))
}

async fn read_codex_file(
    Query(q): Query<CodexFilePathQuery>,
) -> Result<Json<CodexFileResponse>, AppError> {
    let rel = q.path.unwrap_or_else(|| "config.toml".into());
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &rel)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        move || -> anyhow::Result<CodexFileResponse> {
            if !path.exists() {
                return Ok(CodexFileResponse {
                    root: root.display().to_string(),
                    path: relative_codex_path(&root, &path),
                    abs_path: path.display().to_string(),
                    exists: false,
                    mtime_ms: None,
                    raw_text: String::new(),
                });
            }
            if !path.is_file() {
                anyhow::bail!("path is not a file");
            }
            let meta = std::fs::metadata(&path)?;
            let raw_text = std::fs::read_to_string(&path)?;
            Ok(CodexFileResponse {
                root: root.display().to_string(),
                path: relative_codex_path(&root, &path),
                abs_path: path.display().to_string(),
                exists: true,
                mtime_ms: file_mtime_ms(&meta),
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

async fn write_codex_file(
    Json(input): Json<CodexFileWriteInput>,
) -> Result<Json<CodexFileResponse>, AppError> {
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &input.path)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let path = path.clone();
        let raw = input.raw_text;
        move || -> anyhow::Result<CodexFileResponse> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            let meta = std::fs::metadata(&path)?;
            let raw_text = std::fs::read_to_string(&path)?;
            Ok(CodexFileResponse {
                root: root.display().to_string(),
                path: relative_codex_path(&root, &path),
                abs_path: path.display().to_string(),
                exists: true,
                mtime_ms: file_mtime_ms(&meta),
                raw_text,
            })
        }
    })
    .await??;
    Ok(Json(out))
}

async fn delete_codex_file(Query(q): Query<CodexFilePathQuery>) -> Result<StatusCode, AppError> {
    let Some(rel) = q.path else {
        return Err(anyhow::anyhow!("missing path").into());
    };
    if rel.trim().is_empty() || rel.trim() == "." {
        return Err(anyhow::anyhow!("refusing to delete codex root").into());
    }
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &rel)?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    })
    .await??;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_codex_dir(
    Json(input): Json<CodexDirCreateInput>,
) -> Result<Json<CodexFileListResponse>, AppError> {
    let root = codex_home_dir();
    let path = resolve_codex_path(&root, &input.path)?;
    tokio::task::spawn_blocking({
        let path = path.clone();
        move || std::fs::create_dir_all(path)
    })
    .await??;
    list_codex_files(Query(CodexFilePathQuery {
        path: Some(input.path),
    }))
    .await
}

async fn move_codex_file(
    Json(input): Json<CodexFileMoveInput>,
) -> Result<Json<CodexFileResponse>, AppError> {
    if input.from.trim().is_empty() || input.from.trim() == "." {
        return Err(anyhow::anyhow!("refusing to move codex root").into());
    }
    if input.to.trim().is_empty() || input.to.trim() == "." {
        return Err(anyhow::anyhow!("destination path is required").into());
    }
    let root = codex_home_dir();
    let from = resolve_codex_path(&root, &input.from)?;
    let to = resolve_codex_path(&root, &input.to)?;
    let out = tokio::task::spawn_blocking({
        let root = root.clone();
        let overwrite = input.overwrite.unwrap_or(false);
        move || -> anyhow::Result<CodexFileResponse> {
            if !from.exists() {
                anyhow::bail!("source path does not exist");
            }
            if to.exists() {
                if !overwrite {
                    anyhow::bail!("destination already exists");
                }
                if to.is_dir() {
                    std::fs::remove_dir_all(&to)?;
                } else {
                    std::fs::remove_file(&to)?;
                }
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from, &to)?;
            if to.is_file() {
                let meta = std::fs::metadata(&to)?;
                let raw_text = std::fs::read_to_string(&to)?;
                return Ok(CodexFileResponse {
                    root: root.display().to_string(),
                    path: relative_codex_path(&root, &to),
                    abs_path: to.display().to_string(),
                    exists: true,
                    mtime_ms: file_mtime_ms(&meta),
                    raw_text,
                });
            }
            Ok(CodexFileResponse {
                root: root.display().to_string(),
                path: relative_codex_path(&root, &to),
                abs_path: to.display().to_string(),
                exists: true,
                mtime_ms: std::fs::metadata(&to).ok().as_ref().and_then(file_mtime_ms),
                raw_text: String::new(),
            })
        }
    })
    .await??;
    Ok(Json(out))
}

fn codex_home_dir() -> PathBuf {
    crate::codex_config::codex_config_path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_codex_path(root: &std::path::Path, rel: &str) -> anyhow::Result<PathBuf> {
    if rel.contains('\0') {
        anyhow::bail!("invalid path");
    }
    let rel_path = std::path::Path::new(rel);
    if rel_path.is_absolute() {
        anyhow::bail!("absolute paths are not allowed");
    }
    let mut out = root.to_path_buf();
    for component in rel_path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            _ => anyhow::bail!("path traversal is not allowed"),
        }
    }
    ensure_codex_path_within_root(root, &out)?;
    Ok(out)
}

fn ensure_codex_path_within_root(
    root: &std::path::Path,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let canonical = if path.exists() {
        path.canonicalize()?
    } else {
        let mut ancestor = path.parent();
        let mut found = None;
        while let Some(candidate) = ancestor {
            if candidate.exists() {
                found = Some(candidate.canonicalize()?);
                break;
            }
            ancestor = candidate.parent();
        }
        found.unwrap_or_else(|| root.clone())
    };
    if !canonical.starts_with(&root) {
        anyhow::bail!("path resolves outside codex home");
    }
    Ok(())
}

fn relative_codex_path(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".into())
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
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<WsEvent>(32);
    let hello = WsEvent::Hello {
        version: VERSION.into(),
    };
    if let Ok(j) = serde_json::to_string(&hello) {
        let _ = tx.send(Message::Text(j)).await;
    }
    if let Ok(snapshot) = compute_status(state.clone()).await {
        if let Ok(j) = serde_json::to_string(&WsEvent::StatusChanged(snapshot)) {
            let _ = tx.send(Message::Text(j)).await;
        }
    }
    loop {
        tokio::select! {
            ev = sub.recv() => {
                let Ok(ev) = ev else { break };
                let Ok(j) = serde_json::to_string(&ev) else { continue };
                if tx.send(Message::Text(j)).await.is_err() { break; }
            }
            ev = outbound_rx.recv() => {
                let Some(ev) = ev else { continue };
                let Ok(j) = serde_json::to_string(&ev) else { continue };
                if tx.send(Message::Text(j)).await.is_err() { break; }
            }
            incoming = rx.next() => {
                match incoming {
                    None => break,
                    Some(Err(_)) => break,
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Text(text))) => {
                        handle_ws_client_text(state.clone(), outbound_tx.clone(), text.to_string()).await;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_ws_client_text(state: AppState, outbound: mpsc::Sender<WsEvent>, text: String) {
    let Ok(message) = serde_json::from_str::<WsClientMessage>(&text) else {
        return;
    };
    match message {
        WsClientMessage::Snapshot {
            request_id,
            topic,
            hours,
            client,
        } => {
            let request_id = request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            tokio::spawn(async move {
                match topic.as_str() {
                    "status" => {
                        if let Ok(snapshot) = compute_status(state.clone()).await {
                            let _ = outbound.send(WsEvent::StatusChanged(snapshot)).await;
                        }
                    }
                    "dashboard-stats" => {
                        let hours = hours.unwrap_or(24).clamp(1, 24 * 30);
                        if let Ok(stats) = build_dashboard_stats(state.clone(), hours).await {
                            let _ = outbound.send(WsEvent::DashboardStatsChanged(stats)).await;
                        }
                    }
                    "providers-overview" => {
                        let hours = hours.unwrap_or(default_rolling_hours()).clamp(1, 24 * 30);
                        stream_providers_overview(state.clone(), outbound, request_id, hours).await;
                    }
                    "routes" => {
                        if let Ok(routes) = run_blocking(state.clone(), |s| s.db.route_list()).await
                        {
                            let _ = outbound.send(WsEvent::RoutesChanged { routes }).await;
                        }
                    }
                    "codex-app-status" => {
                        if let Ok(Ok(status)) = tokio::task::spawn_blocking(codex_app_status).await
                        {
                            let _ = outbound.send(WsEvent::CodexAppStatusChanged(status)).await;
                        }
                    }
                    "client-status" => {
                        if let Some(client) = client {
                            if let Ok(status) = client_status_inner(&client, state.port) {
                                let _ = outbound.send(WsEvent::ClientStatusChanged(status)).await;
                            }
                        }
                    }
                    _ => {}
                }
            });
        }
    }
}

async fn stream_providers_overview(
    state: AppState,
    outbound: mpsc::Sender<WsEvent>,
    request_id: String,
    hours: i64,
) {
    let Ok(overview) = build_providers_overview(state, hours).await else {
        return;
    };
    let _ = outbound
        .send(WsEvent::ProvidersOverviewStreamStarted(
            ProvidersOverviewStreamStarted {
                request_id: request_id.clone(),
                rolling_hours: hours,
            },
        ))
        .await;
    let _ = outbound
        .send(WsEvent::ProvidersOverviewProvidersChunk(
            ProvidersOverviewProvidersChunk {
                request_id: request_id.clone(),
                rolling_hours: hours,
                providers: overview.providers,
            },
        ))
        .await;
    let _ = outbound
        .send(WsEvent::ProvidersOverviewHealthChunk(
            ProvidersOverviewHealthChunk {
                request_id: request_id.clone(),
                rolling_hours: hours,
                health: overview.health,
            },
        ))
        .await;
    let _ = outbound
        .send(WsEvent::ProvidersOverviewPoolsChunk(
            ProvidersOverviewPoolsChunk {
                request_id: request_id.clone(),
                rolling_hours: hours,
                pools: overview.pools,
            },
        ))
        .await;
    for (provider_id, credentials) in overview.credentials {
        let _ = outbound
            .send(WsEvent::ProvidersOverviewCredentialsChunk(
                ProvidersOverviewCredentialsChunk {
                    request_id: request_id.clone(),
                    rolling_hours: hours,
                    provider_id,
                    credentials,
                },
            ))
            .await;
    }
    for (provider_id, codex_plans) in overview.codex_plans {
        let _ = outbound
            .send(WsEvent::ProvidersOverviewCodexPlansChunk(
                ProvidersOverviewCodexPlansChunk {
                    request_id: request_id.clone(),
                    rolling_hours: hours,
                    provider_id,
                    codex_plans,
                },
            ))
            .await;
    }
    let _ = outbound
        .send(WsEvent::ProvidersOverviewStreamEnded(
            ProvidersOverviewStreamEnded {
                request_id,
                rolling_hours: hours,
            },
        ))
        .await;
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

#[cfg(test)]
mod request_body_limit_tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;
    use vibe_protocol::{ModelAlias, ProviderInput, ProviderKind};

    #[tokio::test]
    async fn codex_responses_allows_payloads_above_axum_default_body_limit() {
        let db = vibe_db::Db::memory().expect("db");
        let provider = db
            .provider_insert(ProviderInput {
                name: "dummy responses".into(),
                group_name: None,
                avatar_url: None,
                kind: ProviderKind::OpenaiResponses,
                base_url: "http://127.0.0.1:9".into(),
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
        db.route_insert(vibe_protocol::RouteInput {
            name: "default".into(),
            match_model: "gpt-test".into(),
            target_provider_id: Some(provider.id),
            target_model: Some("gpt-test".into()),
            tier: vibe_protocol::RouteTier::Default,
            priority: 100,
        })
        .expect("route");

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
    #[cfg(target_os = "macos")]
    fn codex_app_process_roles_are_specific() {
        assert_eq!(
            classify_codex_app_process("/Applications/Codex.app/Contents/MacOS/Codex"),
            "main"
        );
        assert_eq!(
            classify_codex_app_process(
                "/Applications/Codex.app/Contents/Frameworks/Codex Helper.app/Contents/MacOS/Codex Helper --type=gpu-process"
            ),
            "helper"
        );
        assert_eq!(
            classify_codex_app_process(
                "/Applications/Codex.app/Contents/Frameworks/Codex Helper (Renderer).app/Contents/MacOS/Codex Helper (Renderer) --type=renderer"
            ),
            "renderer"
        );
        assert_eq!(
            classify_codex_app_process(
                "/Applications/Codex.app/Contents/Resources/codex app-server --analytics-default-enabled"
            ),
            "app-server"
        );
        assert_eq!(
            classify_codex_app_process(
                "/Applications/Codex.app/Contents/Frameworks/Electron Framework.framework/Helpers/chrome_crashpad_handler"
            ),
            "crashpad"
        );
    }
}
