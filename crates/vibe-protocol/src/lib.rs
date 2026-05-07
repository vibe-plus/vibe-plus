//! Shared protocol types between vibe core and clients (CLI, Web, future Tauri).
//!
//! Every type in this crate must derive Serialize, Deserialize, and TS so it
//! round-trips between Rust HTTP handlers, the SQLite layer, and the Vue UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const TS_OUT_DIR: &str = "../../packages/protocol/types";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/ProviderKind.ts")]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Anthropic,
    OpenaiCompat,
    OpenaiResponses,
    GeminiNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/RouteTier.ts")]
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
#[ts(export, export_to = "../../packages/protocol/types/Provider.ts")]
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
#[ts(export, export_to = "../../packages/protocol/types/ModelAlias.ts")]
pub struct ModelAlias {
    pub alias: String,
    pub upstream_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/Route.ts")]
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
#[ts(export, export_to = "../../packages/protocol/types/RequestLog.ts")]
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
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/ModelPricing.ts")]
pub struct ModelPricing {
    pub model: String,
    pub input_per_million_usd: String,
    pub output_per_million_usd: String,
    pub cache_read_per_million_usd: String,
    pub cache_creation_per_million_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/Status.ts")]
pub struct Status {
    pub version: String,
    pub uptime_secs: u64,
    pub port: u16,
    pub providers_total: usize,
    pub providers_enabled: usize,
    pub requests_last_hour: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/UsageSummary.ts")]
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
#[ts(export, export_to = "../../packages/protocol/types/WsEvent.ts")]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WsEvent {
    Hello { version: String },
    StatusChanged(Status),
    LogAppended(RequestLog),
}

/// Paginated request log envelope returned by `GET /_vp/logs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/LogPage.ts")]
pub struct LogPage {
    pub items: Vec<RequestLog>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/Health.ts")]
pub struct Health {
    pub ok: bool,
}

/// Body for `POST /_vp/providers` and the patch shape for `PUT`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/ProviderInput.ts")]
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
#[ts(export, export_to = "../../packages/protocol/types/ProviderHealth.ts")]
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
#[ts(export, export_to = "../../packages/protocol/types/HealthSummary.ts")]
pub struct HealthSummary {
    pub providers: Vec<ProviderHealth>,
    pub total_providers: usize,
    pub healthy_providers: usize,
}

/// Enhanced stats for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/DashboardStats.ts")]
pub struct DashboardStats {
    pub requests_last_hour: i64,
    pub requests_last_24h: i64,
    pub success_rate_last_hour: f64,
    pub avg_latency_ms: i64,
    pub p95_latency_ms: i64,
    pub input_tokens_last_24h: i64,
    pub output_tokens_last_24h: i64,
    pub top_models: Vec<ModelStat>,
    pub per_provider: Vec<ProviderStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/ModelStat.ts")]
pub struct ModelStat {
    pub model: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../packages/protocol/types/ProviderStat.ts")]
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
}

pub fn ts_out_dir() -> &'static str {
    TS_OUT_DIR
}
