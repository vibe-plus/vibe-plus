//! Shared protocol types between vibe core and clients (CLI, Web, future Tauri).
//!
//! Every type in this crate must derive Serialize, Deserialize, and TS so it
//! round-trips between Rust HTTP handlers, the SQLite layer, and the Vue UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const TS_OUT_DIR: &str = "../packages/protocol/types";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderKind.ts")]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Anthropic,
    /// Chat Completions wire (/v1/chat/completions). Formerly "openai-compat".
    #[serde(alias = "openai-compat")]
    OpenaiChat,
    OpenaiResponses,
    GeminiNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RouteTier.ts")]
#[serde(rename_all = "kebab-case")]
pub enum RouteTier {
    High,
    Low,
    Default,
}

/// A configured upstream provider.
///
/// `auth_ref` is a string that resolves to a real secret via the `secrets`
/// module of `vibe-core`. Examples: `keyring:anthropic-prod`, `env:OPENAI_API_KEY`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Provider.ts")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub auth_ref: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    /// Maps from a model alias (e.g. "high", "low", or "claude-sonnet") to an
    /// upstream model id. Routes are looked up here when forwarding.
    pub model_aliases: Vec<ModelAlias>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ModelAlias.ts")]
pub struct ModelAlias {
    pub alias: String,
    pub upstream_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Route.ts")]
pub struct Route {
    pub id: String,
    pub name: String,
    pub match_model: String,
    pub target_provider_id: Option<String>,
    pub target_model: Option<String>,
    pub tier: RouteTier,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RequestLog.ts")]
pub struct RequestLog {
    pub id: String,
    pub started_at: i64,
    pub app: Option<String>,
    pub provider_id: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub status_code: Option<i32>,
    pub error: Option<String>,
    pub latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// Stored as a decimal string because we don't want float drift in money.
    pub estimated_cost_usd: String,
    /// Wire protocol: `anthropic` | `openai-chat` | `openai-responses` | `gemini-native`.
    #[serde(default)]
    pub wire: Option<String>,
    /// Entry route hint: `codex-v1`, `plain-v1`, `opencode-v1`, …
    #[serde(default)]
    pub route_prefix: Option<String>,
    #[serde(default)]
    pub credential_id: Option<String>,
    /// Circuit-breaker key tried for this attempt (`provider_id` or credential uuid).
    #[serde(default)]
    pub cb_key: Option<String>,
    /// HTTP status from upstream (when distinct from what the client ultimately saw).
    #[serde(default)]
    pub upstream_http_status: Option<i32>,
    /// Upstream JSON/text error body（4xx 等场景尽量全量 UTF-8 有损存储）。
    #[serde(default)]
    pub upstream_error_preview: Option<String>,
    /// Optional dedupe key (`x-request-id` + route).
    #[serde(default)]
    pub dedupe_key: Option<String>,
    /// Inbound HTTP body（网关视角，UTF-8 有损全量）。
    #[serde(default)]
    pub request_body: Option<String>,
    /// Upstream response body（非流式或本地缓冲的流式原始字节转为字符串，不全量截断）。
    #[serde(default)]
    pub response_body: Option<String>,
    /// Chat→Responses 侧：发给客户端的帧（如 Codex WS），与 `response_body` 里上游原始 Chat SSE 对照。
    #[serde(default)]
    pub client_response_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ModelPricing.ts")]
pub struct ModelPricing {
    pub model: String,
    pub input_per_million_usd: String,
    pub output_per_million_usd: String,
    pub cache_read_per_million_usd: String,
    pub cache_creation_per_million_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Status.ts")]
pub struct Status {
    pub version: String,
    pub uptime_secs: u64,
    pub port: u16,
    pub providers_total: usize,
    pub providers_enabled: usize,
    pub requests_last_hour: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/UsageSummary.ts")]
pub struct UsageSummary {
    pub range: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub estimated_cost_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/WsEvent.ts")]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WsEvent {
    Hello { version: String },
    StatusChanged(Status),
    LogAppended(RequestLog),
}

/// Paginated request log envelope returned by `GET /_vp/logs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/LogPage.ts")]
pub struct LogPage {
    pub items: Vec<RequestLog>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Health.ts")]
pub struct Health {
    pub ok: bool,
}

/// Body for `POST /_vp/providers` and the patch shape for `PUT`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderInput.ts")]
pub struct ProviderInput {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub auth_ref: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    pub model_aliases: Vec<ModelAlias>,
}

/// Live health record for a provider — returned by `GET /_vp/providers/:id/health`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderHealth.ts")]
pub struct ProviderHealth {
    pub provider_id: String,
    pub is_healthy: bool,
    pub circuit_state: String,
    pub consecutive_failures: i32,
    pub total_requests: i64,
    pub total_successes: i64,
    pub total_failures: i64,
    pub success_rate: f64,
    pub last_success_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_error: Option<String>,
    pub avg_latency_ms: Option<i64>,
    pub updated_at: i64,
}

/// Summary of health for all providers — returned by `GET /_vp/health/providers`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/HealthSummary.ts")]
pub struct HealthSummary {
    pub providers: Vec<ProviderHealth>,
    pub total_providers: usize,
    pub healthy_providers: usize,
}

/// Rolling-window gateway stats + cumulative circuit/health for one provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderHealthSummary.ts")]
pub struct ProviderHealthSummary {
    pub cumulative: ProviderHealth,
    pub rolling_hours: i64,
    pub rolling: Option<ProviderStat>,
}

/// Latest Codex ChatGPT Plan snapshot parsed from `x-codex-*` response headers.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/CredentialPlanSnapshot.ts")]
pub struct CredentialPlanSnapshot {
    pub id: String,
    pub credential_id: String,
    pub captured_at: i64,
    pub codex_5h_used_percent: Option<f64>,
    pub codex_7d_used_percent: Option<f64>,
    pub codex_5h_reset_after_seconds: Option<i64>,
    pub codex_7d_reset_after_seconds: Option<i64>,
    pub codex_primary_used_percent: Option<f64>,
    pub codex_secondary_used_percent: Option<f64>,
    pub summary: Option<String>,
    pub source: String,
}

/// Latest plan snapshot per credential on a ChatGPT Codex provider (`GET /_vp/providers/:id/codex-plan`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderCodexPlanItem.ts")]
pub struct ProviderCodexPlanItem {
    pub credential_id: String,
    pub label: String,
    pub plan: Option<CredentialPlanSnapshot>,
}

/// Result of `POST /_vp/providers/:id/codex-plan/refresh` or single-credential refresh.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/CodexPlanRefreshResult.ts")]
pub struct CodexPlanRefreshResult {
    pub attempted: usize,
    pub ok: usize,
    pub errors: Vec<String>,
}

/// Enhanced stats for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/DashboardStats.ts")]
pub struct DashboardStats {
    /// `hours` query parameter mirrored back for the UI.
    pub window_hours: i64,
    /// Human-readable range, e.g. `last 24h`.
    pub window_label: String,
    /// Aggregates below use this rolling window (same as `hours` query).
    pub requests_in_window: i64,
    pub success_rate_in_window: f64,
    pub input_tokens_in_window: i64,
    pub output_tokens_in_window: i64,

    pub requests_last_hour: i64,
    pub requests_last_24h: i64,
    pub success_rate_last_hour: f64,
    pub avg_latency_ms: i64,
    pub p95_latency_ms: i64,
    /// Always last rolling 24h (fixed snapshot card).
    pub input_tokens_last_24h: i64,
    pub output_tokens_last_24h: i64,
    pub top_models: Vec<ModelStat>,
    pub per_provider: Vec<ProviderStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ModelStat.ts")]
pub struct ModelStat {
    pub model: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderStat.ts")]
pub struct ProviderStat {
    pub provider_id: String,
    pub provider_name: String,
    pub requests: i64,
    pub successes: i64,
    pub failures: i64,
    pub success_rate: f64,
    pub avg_latency_ms: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// HTTP status breakdown within the same window as other fields.
    pub err_429: i64,
    pub err_503: i64,
    pub err_4xx_other: i64,
    pub err_5xx_other: i64,
}

/// A single API key / OAuth account attached to a provider.
///
/// Supports two mutually exclusive auth modes:
///   1. `auth_ref`          — resolves via secrets module (keyring, env, literal…)
///   2. `oauth_access_token` — token stored directly in SQLite, auto-refreshed via
///                            `oauth_refresh_token` against auth.openai.com.
///
/// Multiple credentials per provider enable key-pool rotation: each credential
/// gets its own circuit state and rate-limit tracking so a saturated key
/// doesn't block the whole provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Credential.ts")]
pub struct Credential {
    pub id: String,
    pub provider_id: String,
    pub label: String,
    /// auth_ref mode (keyring:, env:, literal:, …) — no `codex-auth` file paths.
    pub auth_ref: Option<String>,
    /// "claude-pro" | "codex-plus" | "codex-pro" | "token" | "payg" | …
    pub plan_type: Option<String>,
    pub notes: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    // ── OAuth direct-storage fields ─────────────────────────────────────────
    /// Current OAuth access token (may be near-expiry; auto-refreshed by proxy).
    pub oauth_access_token: Option<String>,
    /// Whether a refresh_token is stored (token itself is never returned by API).
    pub oauth_has_refresh: bool,
    /// Unix timestamp when the access token expires (null = unknown).
    pub oauth_expires_at: Option<i64>,
    // ── Rate-limit state ─────────────────────────────────────────────────────
    /// Upstream rate-limit headers — updated after every response.
    pub rl_requests_limit: Option<i64>,
    pub rl_requests_remaining: Option<i64>,
    pub rl_requests_reset_at: Option<i64>,
    pub rl_tokens_limit: Option<i64>,
    pub rl_tokens_remaining: Option<i64>,
    pub rl_tokens_reset_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub last_error: Option<String>,
    pub consecutive_failures: i32,
    pub created_at: i64,
    pub updated_at: i64,
    /// Stable hash for duplicate-import detection (`fp:…`).
    #[serde(default)]
    pub auth_fingerprint: Option<String>,
}

/// Body for `POST /_vp/providers/:id/credentials` and `PUT /_vp/credentials/:id`.
///
/// Set either `auth_ref` (points to a secret) **or** `oauth_access_token` +
/// `oauth_refresh_token` (stored directly in SQLite).  Do not set both.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/CredentialInput.ts")]
pub struct CredentialInput {
    pub label: String,
    pub auth_ref: Option<String>,
    pub plan_type: Option<String>,
    pub notes: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    // ── OAuth direct-storage (write-only; refresh_token never returned) ──────
    pub oauth_access_token: Option<String>,
    /// Write-only: stored in DB but never returned in Credential responses.
    pub oauth_refresh_token: Option<String>,
    pub oauth_expires_at: Option<i64>,
}

pub fn ts_out_dir() -> &'static str {
    TS_OUT_DIR
}
