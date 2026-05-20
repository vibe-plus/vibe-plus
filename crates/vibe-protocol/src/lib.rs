//! Shared protocol types between vibe core and clients (CLI, Web, future Tauri).
//!
//! Every type in this crate must derive Serialize, Deserialize, and TS so it
//! round-trips between Rust HTTP handlers, the SQLite layer, and the Vue UI.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
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

/// One upstream wire endpoint for a logical vendor (e.g. DeepSeek Chat + Messages).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderProtocol.ts")]
pub struct ProviderProtocol {
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub model_aliases: Vec<ModelAlias>,
}

/// Human-facing wire protocol label (not the internal slug).
pub fn protocol_display_label(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "Messages",
        ProviderKind::OpenaiChat => "Chat",
        ProviderKind::OpenaiResponses => "Responses",
        ProviderKind::GeminiNative => "Generate",
    }
}

pub fn provider_kind_slug(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenaiChat => "openai-chat",
        ProviderKind::OpenaiResponses => "openai-responses",
        ProviderKind::GeminiNative => "gemini-native",
    }
}

/// Which upstream management platform this credential belongs to.
/// Controls login flow, balance API, group listing, and UI hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/CredentialVendor.ts")]
#[serde(rename_all = "kebab-case")]
pub enum CredentialVendor {
    /// Generic relay — bearer-token key, standard OpenAI billing endpoints.
    Generic,
    /// NewAPI / one-api fork.  Supports password login + LinuxDO OAuth.
    NewApi,
    /// Sub2API.  Supports password login, group selection, window-based usage.
    Sub2Api,
    /// Official Anthropic API key (pay-as-you-go).
    AnthropicPayg,
    /// Official Anthropic subscription plan (Pro / Max / custom).
    AnthropicPlan,
}

/// Runtime upstream candidate synthesized from a UI provider, one endpoint/protocol,
/// and optionally one credential. This is the gateway algorithm's smallest
/// schedulable unit; UI-facing provider records are only templates/pools.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Upstream.ts")]
pub struct Upstream {
    /// Stable synthetic id: provider + protocol endpoint + credential/provider auth.
    pub id: String,
    /// UI/provider profile that owns this runtime candidate.
    pub provider_id: String,
    /// Wire protocol selected for this candidate.
    pub kind: ProviderKind,
    /// Concrete upstream endpoint used by the gateway.
    pub base_url: String,
    /// Credential/account chosen for this runtime candidate, if any.
    #[serde(default)]
    pub credential_id: Option<String>,
    /// Circuit breaker / rate-limit key. Usually credential_id, otherwise provider_id.
    pub cb_key: String,
    /// Fully resolved availability gate for the algorithm.
    pub enabled: bool,
    /// Optional deterministic tie-breaker only; UI priority lives on Provider/Credential.
    pub priority: i32,
}

/// Aggregated view of runtime upstream units derived from a provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/ProviderUpstreamSummary.ts"
)]
pub struct ProviderUpstreamSummary {
    pub provider_id: String,
    pub total_upstreams: i64,
    pub enabled_upstreams: i64,
    pub endpoint_count: i64,
    pub credential_count: i64,
    #[serde(default)]
    pub sample_upstreams: Vec<Upstream>,
}

/// One rolling-window usage snapshot (5 h / 1 d / 7 d).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/UsageWindow.ts")]
pub struct UsageWindow {
    /// Human-readable label shown in the UI, e.g. "5h", "1d", "7d".
    pub label: String,
    pub used_usd: f64,
    pub limit_usd: Option<f64>,
    /// 0.0–100.0 or None when limit unknown.
    pub used_pct: Option<f64>,
    /// Unix timestamp when the window resets.
    pub reset_at: Option<i64>,
}

/// Group/channel info returned by `GET /_vp/credentials/:id/groups`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/UpstreamGroupInfo.ts")]
pub struct UpstreamGroupInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub platform: Option<String>,
    pub rate_multiplier: f64,
}

/// Body for `POST /_vp/credentials/:id/login`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialLoginRequest.ts"
)]
pub struct CredentialLoginRequest {
    pub username: String,
    pub password: String,
}

/// Response from `POST /_vp/credentials/:id/login`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialLoginResponse.ts"
)]
pub struct CredentialLoginResponse {
    pub ok: bool,
    pub note: Option<String>,
}

/// Map API hostnames to a short brand label for display and icon matching.
pub fn host_to_brand_label(host: &str) -> Option<&'static str> {
    let h = host.trim().trim_start_matches("www.").to_ascii_lowercase();
    match h.as_str() {
        "api.deepseek.com" | "deepseek.com" => Some("DeepSeek"),
        "api.moonshot.cn" | "api.moonshot.ai" | "moonshot.cn" => Some("Moonshot"),
        "dashscope.aliyuncs.com" | "dashscope-intl.aliyuncs.com" => Some("Qwen"),
        "open.bigmodel.cn" => Some("ChatGLM"),
        "api.anthropic.com" => Some("Anthropic"),
        "api.openai.com" => Some("OpenAI"),
        "generativelanguage.googleapis.com" => Some("Google"),
        "api.groq.com" => Some("Groq"),
        "openrouter.ai" | "api.openrouter.ai" => Some("OpenRouter"),
        "api.mistral.ai" => Some("Mistral"),
        "api.x.ai" => Some("xAI"),
        "api.together.xyz" => Some("Together"),
        "api.fireworks.ai" => Some("Fireworks"),
        "api.minimax.chat" => Some("MiniMax"),
        "api.z.ai" | "open.z.ai" => Some("Zhipu"),
        "api.siliconflow.cn" => Some("SiliconFlow"),
        _ => None,
    }
}

pub fn host_from_base_url(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim();
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))?;
    let host = rest.split('/').next()?.split(':').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Normalized hostname for provider deduplication (lowercase, no `www.`).
pub fn canonical_provider_host(input: &str) -> Option<String> {
    let host = if input.contains("://") {
        host_from_base_url(input)?
    } else {
        input.trim().to_string()
    };
    let key = host.trim().trim_start_matches("www.").to_ascii_lowercase();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// Fallback label from hostname segments, e.g. `api.deepseek.com` → `Api Deepseek`.
pub fn host_label_camel_fallback(host: &str) -> String {
    const SKIP: &[&str] = &[
        "www", "api", "com", "cn", "net", "org", "io", "ai", "dev", "co", "uk",
    ];
    let parts: Vec<String> = host
        .trim()
        .trim_start_matches("www.")
        .split('.')
        .filter(|p| !p.is_empty() && !SKIP.contains(&p.to_ascii_lowercase().as_str()))
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut out = first.to_uppercase().to_string();
                    out.push_str(chars.as_str());
                    out
                }
            }
        })
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        host.trim().to_string()
    } else {
        parts.join(" ")
    }
}

pub fn display_name_for_remote(
    branding_name: Option<&str>,
    base_url: &str,
    _kind: ProviderKind,
) -> String {
    if let Some(name) = branding_name.map(str::trim).filter(|s| !s.is_empty()) {
        return name.to_string();
    }
    if let Some(host) = host_from_base_url(base_url) {
        if let Some(brand) = host_to_brand_label(&host) {
            return brand.to_string();
        }
        return host_label_camel_fallback(&host);
    }
    base_url.trim().to_string()
}

impl ProviderProtocol {
    pub fn from_kind_base(kind: ProviderKind, base_url: impl Into<String>) -> Self {
        Self {
            kind,
            base_url: base_url.into(),
            model_aliases: Vec::new(),
        }
    }
}

impl Provider {
    /// Effective protocol list: stored `protocols` or a single entry from legacy `kind` + `base_url`.
    pub fn effective_protocols(&self) -> Vec<ProviderProtocol> {
        if !self.protocols.is_empty() {
            return self.protocols.clone();
        }
        vec![ProviderProtocol {
            kind: self.kind,
            base_url: self.base_url.clone(),
            model_aliases: self.model_aliases.clone(),
        }]
    }

    /// Primary protocol entry used for legacy `kind` / `base_url` compatibility.
    pub fn primary_protocol(&self) -> ProviderProtocol {
        self.effective_protocols()
            .into_iter()
            .next()
            .unwrap_or_else(|| ProviderProtocol::from_kind_base(self.kind, self.base_url.clone()))
    }

    /// Pick the protocol entry that matches `wire_kind`, or the primary (`kind` / first).
    pub fn protocol_for_kind(&self, wire_kind: ProviderKind) -> ProviderProtocol {
        self.effective_protocols()
            .into_iter()
            .find(|p| p.kind == wire_kind)
            .unwrap_or_else(|| ProviderProtocol::from_kind_base(self.kind, self.base_url.clone()))
    }

    /// Provider clone with `kind` / `base_url` / aliases aligned to the chosen protocol.
    pub fn with_protocol(&self, proto: &ProviderProtocol) -> Self {
        let mut out = self.clone();
        out.kind = proto.kind;
        out.base_url = proto.base_url.clone();
        if !proto.model_aliases.is_empty() {
            out.model_aliases = proto.model_aliases.clone();
        }
        out
    }
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
    /// Human-facing provider group, e.g. "Provider A" with HK/SG endpoints below it.
    #[serde(default)]
    pub group_name: Option<String>,
    /// Optional custom avatar/logo URL for the provider, typically discovered from the upstream site.
    #[serde(default)]
    pub avatar_url: Option<String>,
    /// Provider is a UI-facing profile/pool. Runtime routing expands it into
    /// [`Upstream`] units by crossing endpoints/protocols with credentials.
    #[serde(default)]
    pub upstreams: Vec<Upstream>,
    #[serde(default)]
    pub upstream_summary: Option<ProviderUpstreamSummary>,
    pub kind: ProviderKind,
    pub base_url: String,
    /// All wire endpoints for this vendor (Chat + Messages on the same host, etc.).
    #[serde(default)]
    pub protocols: Vec<ProviderProtocol>,
    /// Parsed hostname for dedupe and branding (`api.deepseek.com`, …).
    #[serde(default)]
    pub host: Option<String>,
    pub auth_ref: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    /// Whether this upstream endpoint itself supports WebSocket transport.
    /// `None` means unknown/not measured; the gateway may still accept client WS and bridge to HTTP/SSE.
    #[serde(default)]
    pub supports_websocket: Option<bool>,
    /// When true, request model names are passed upstream unchanged unless an explicit alias matches.
    pub passthrough_mode: bool,
    /// Latest fetched remote model ids from upstream `/models` (or equivalent), if available.
    pub remote_models: Vec<String>,
    pub remote_models_fetched_at: Option<i64>,
    /// Latest endpoint speed/liveness probe copied from CC Switch's warm-up + measured GET flow.
    #[serde(default)]
    pub last_speedtest: Option<ProviderSpeedtestResult>,
    /// Maps from a model alias (e.g. "high", "low", or "claude-sonnet") to an
    /// upstream model id. Routes are looked up here when forwarding.
    pub model_aliases: Vec<ModelAlias>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/ProviderBalanceSnapshot.ts"
)]
pub struct ProviderBalanceSnapshot {
    pub currency: String,
    pub balance: Option<String>,
    pub remaining: Option<String>,
    pub used: Option<String>,
    pub total: Option<String>,
    pub period: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/ProviderSpeedtestResult.ts"
)]
pub struct ProviderSpeedtestResult {
    pub url: String,
    pub ok: bool,
    pub latency_ms: Option<i64>,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ModelAlias.ts")]
pub struct ModelAlias {
    pub alias: String,
    pub upstream_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RequestLog.ts")]
pub struct RequestLog {
    pub id: String,
    pub started_at: i64,
    pub app: Option<String>,
    pub provider_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
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
    #[serde(default)]
    pub reasoning_tokens: i64,
    #[serde(default)]
    pub cache_creation_5m_tokens: i64,
    #[serde(default)]
    pub cache_creation_1h_tokens: i64,
    #[serde(default)]
    pub audio_input_tokens: i64,
    #[serde(default)]
    pub audio_output_tokens: i64,
    #[serde(default)]
    pub accepted_prediction_tokens: i64,
    #[serde(default)]
    pub rejected_prediction_tokens: i64,
    #[serde(default)]
    pub cost_items: Option<String>,
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
    /// Upstream JSON/text error body (lossy UTF-8, stored as fully as possible for 4xx and similar cases)。
    #[serde(default)]
    pub upstream_error_preview: Option<String>,
    /// Optional dedupe key (`x-request-id` + route).
    #[serde(default)]
    pub dedupe_key: Option<String>,
    /// Client-facing transport used by Vibe: `ws`, `http-sse`, `http`, etc.
    #[serde(default)]
    pub client_transport: Option<String>,
    /// Sanitized inbound request headers from the client.
    #[serde(default)]
    pub request_headers: Option<String>,
    /// Inbound HTTP body (gateway perspective, full lossy UTF-8)。
    #[serde(default)]
    pub request_body: Option<String>,
    /// Upstream response body (non-streaming or locally buffered streaming raw bytes converted to string, not fully truncated)。
    #[serde(default)]
    pub response_body: Option<String>,
    /// Chat→Responses side: frames sent to the client, such as Codex WS, for comparison with upstream raw Chat SSE in `response_body`.
    #[serde(default)]
    pub client_response_body: Option<String>,
    #[serde(default)]
    pub stream_kind: Option<String>,
    #[serde(default)]
    pub stream_terminal_seen: Option<bool>,
    #[serde(default)]
    pub stream_end_reason: Option<String>,
    #[serde(default)]
    pub stream_error_detail: Option<String>,
    #[serde(default)]
    pub upstream_first_byte_ms: Option<i64>,
    #[serde(default)]
    pub client_first_write_ms: Option<i64>,
    #[serde(default)]
    pub last_upstream_event_ms: Option<i64>,
    #[serde(default)]
    pub last_client_write_ms: Option<i64>,
    #[serde(default)]
    pub upstream_chunk_count: i64,
    #[serde(default)]
    pub upstream_bytes: i64,
    #[serde(default)]
    pub client_chunk_count: i64,
    #[serde(default)]
    pub client_bytes: i64,
    #[serde(default)]
    pub sse_event_count: i64,
    #[serde(default)]
    pub sse_data_count: i64,
    #[serde(default)]
    pub sse_comment_count: i64,
    #[serde(default)]
    pub sse_keepalive_count: i64,
    #[serde(default)]
    pub sse_done_count: i64,
    #[serde(default)]
    pub parse_error_count: i64,
    #[serde(default)]
    pub first_keepalive_ms: Option<i64>,
    #[serde(default)]
    pub last_keepalive_ms: Option<i64>,
    #[serde(default)]
    pub max_gap_between_upstream_events_ms: Option<i64>,
    #[serde(default)]
    pub max_gap_between_data_events_ms: Option<i64>,
    #[serde(default)]
    pub keepalive_after_last_data_count: i64,
    #[serde(default)]
    pub last_data_event_ms: Option<i64>,
    #[serde(default)]
    pub bridge_mode: Option<String>,
    #[serde(default)]
    pub status_injected: bool,
    #[serde(default)]
    pub terminal_injected: bool,
    #[serde(default)]
    pub upstream_terminal_type: Option<String>,
}

fn default_wave_size() -> i32 {
    1
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
#[ts(export, export_to = "../packages/protocol/types/Meta.ts")]
pub struct Meta {
    /// Semver of the running CLI binary.
    pub cli_version: String,
    /// Gateway protocol epoch (bump when new `/_vp/` endpoints are added).
    pub protocol_version: u32,
    /// Oldest Web UI protocol epoch this gateway can serve.
    pub min_web_protocol: u32,
    /// Canonical URL of the hosted Web UI (GitHub Pages).
    pub ui_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/WebCompatibility.ts")]
pub struct WebCompatibility {
    pub api: u32,
    pub min_web_api: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Status.ts")]
pub struct Status {
    pub version: String,
    pub web_compatibility: WebCompatibility,
    pub uptime_secs: u64,
    pub port: u16,
    pub providers_total: usize,
    pub providers_enabled: usize,
    pub requests_last_hour: i64,
    #[serde(default)]
    pub codex_ws_active: usize,
    #[serde(default)]
    pub codex_ws_total: usize,
    #[serde(default)]
    pub codex_ws_requests_total: usize,
    #[serde(default)]
    pub codex_http_responses_total: usize,
    #[serde(default)]
    pub codex_last_transport: Option<String>,
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
    #[serde(default)]
    pub reasoning_tokens: i64,
    #[serde(default)]
    pub cache_creation_5m_tokens: i64,
    #[serde(default)]
    pub cache_creation_1h_tokens: i64,
    #[serde(default)]
    pub audio_input_tokens: i64,
    #[serde(default)]
    pub audio_output_tokens: i64,
    #[serde(default)]
    pub accepted_prediction_tokens: i64,
    #[serde(default)]
    pub rejected_prediction_tokens: i64,
    #[serde(default)]
    pub cost_items: Option<String>,
    pub estimated_cost_usd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/AppLogLevel.ts")]
#[serde(rename_all = "kebab-case")]
pub enum AppLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/AppLogEvent.ts")]
pub struct AppLogEvent {
    /// Unix timestamp (seconds).
    pub ts: i64,
    pub level: AppLogLevel,
    /// Stable event type slug, e.g. "credential.circuit.opened".
    #[serde(default = "default_app_log_event_type")]
    pub event_type: String,
    /// Versioned event payload. Renderers must prefer this snapshot over live DB rows.
    #[serde(default)]
    pub payload: JsonValue,
    /// Short category slug: "provider", "credential", "circuit", "system", …
    pub category: String,
    /// Legacy fallback message for old rows or unknown event renderers.
    pub message: String,
    #[serde(default)]
    pub detail: Option<String>,
}

fn default_app_log_event_type() -> String {
    "legacy.message".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ClientStatus.ts")]
pub struct ClientStatus {
    pub client: String,
    pub config_path: String,
    pub config_exists: bool,
    pub taken_over: bool,
    pub expected_base_url: String,
    pub configured_base_url: Option<String>,
    pub auth_proxy_managed: Option<bool>,
    pub model_overrides_present: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/ClientTakeoverResult.ts"
)]
pub struct ClientTakeoverResult {
    pub client: String,
    pub config_path: String,
    pub backup_path: Option<String>,
    pub status: ClientStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/UpstreamAttemptPhase.ts"
)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamAttemptPhase {
    Connecting,
    Streaming,
    Completed,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/UpstreamAttemptOutcome.ts"
)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamAttemptOutcome {
    Success,
    RetryableError,
    ClientError,
    RateLimit,
    TransportError,
    FallbackAbandon,
    CircuitSkip,
    /// A loser in a race fanout — request was sent but cancelled because
    /// another credential won. Body may or may not have started streaming.
    RaceAborted,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/UpstreamAttemptLog.ts")]
pub struct UpstreamAttemptLog {
    pub attempt_id: String,
    pub request_id: String,
    pub attempt_index: i32,
    /// Zero-based scheduler wave for this user request. Attempts in the same wave are concurrent.
    #[serde(default)]
    pub wave_index: i32,
    /// Number of upstreams dispatched in this wave.
    #[serde(default = "default_wave_size")]
    pub wave_size: i32,
    /// Minimal runtime upstream id used by gateway scheduling.
    #[serde(default)]
    pub upstream_id: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub provider_id: Option<String>,
    pub credential_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    pub wire: Option<String>,
    pub route_prefix: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub phase: UpstreamAttemptPhase,
    pub outcome: UpstreamAttemptOutcome,
    pub status_code: Option<i32>,
    pub upstream_http_status: Option<i32>,
    pub error_summary: Option<String>,
    pub latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    #[serde(default)]
    pub reasoning_tokens: i64,
    #[serde(default)]
    pub cache_creation_5m_tokens: i64,
    #[serde(default)]
    pub cache_creation_1h_tokens: i64,
    #[serde(default)]
    pub audio_input_tokens: i64,
    #[serde(default)]
    pub audio_output_tokens: i64,
    #[serde(default)]
    pub accepted_prediction_tokens: i64,
    #[serde(default)]
    pub rejected_prediction_tokens: i64,
    #[serde(default)]
    pub cost_items: Option<String>,
    /// Stored as decimal USD string for money stability.
    #[serde(default)]
    pub estimated_cost_usd: String,
    pub upstream_first_byte_ms: Option<i64>,
    pub client_first_write_ms: Option<i64>,
    pub last_upstream_event_ms: Option<i64>,
    pub last_client_write_ms: Option<i64>,
    pub upstream_chunk_count: i64,
    pub upstream_bytes: i64,
    pub client_chunk_count: i64,
    pub client_bytes: i64,
    pub sse_event_count: i64,
    pub sse_data_count: i64,
    pub sse_comment_count: i64,
    pub sse_keepalive_count: i64,
    pub sse_done_count: i64,
    pub parse_error_count: i64,
    pub first_keepalive_ms: Option<i64>,
    pub last_keepalive_ms: Option<i64>,
    pub max_gap_between_upstream_events_ms: Option<i64>,
    pub max_gap_between_data_events_ms: Option<i64>,
    pub keepalive_after_last_data_count: i64,
    pub last_data_event_ms: Option<i64>,
    pub bridge_mode: Option<String>,
    pub status_injected: bool,
    pub terminal_injected: bool,
    pub upstream_terminal_type: Option<String>,
    pub active_upstream_decode_tps_peak: Option<f64>,
    pub active_downstream_emit_tps_peak: Option<f64>,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_headers: Option<String>,
    pub response_body: Option<String>,
    /// Gateway→upstream network URL scheme (http/https/ws/wss), separated from model semantics.
    #[serde(default)]
    pub network_scheme: Option<String>,
    /// Gateway→upstream network host, separated from provider/model semantics.
    #[serde(default)]
    pub network_host: Option<String>,
    /// Gateway→upstream request path (query intentionally omitted).
    #[serde(default)]
    pub network_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RealtimeAttempt.ts")]
pub struct RealtimeAttempt {
    pub attempt_id: String,
    pub request_id: String,
    pub attempt_index: i32,
    pub wave_index: i32,
    pub wave_size: i32,
    pub upstream_id: Option<String>,
    pub started_at: i64,
    pub updated_at: i64,
    pub provider_id: Option<String>,
    pub credential_id: Option<String>,
    pub wire: Option<String>,
    pub route_prefix: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub phase: String,
    pub status_code: Option<i32>,
    pub upstream_http_status: Option<i32>,
    pub error: Option<String>,
    pub active_output_tokens_per_sec: Option<f64>,
    pub active_cost_usd_per_hour: Option<f64>,
    pub active_upstream_bytes_per_sec: f64,
    pub active_downstream_bytes_per_sec: f64,
    pub output_tokens_so_far: i64,
    pub upstream_bytes_so_far: i64,
    pub client_bytes_so_far: i64,
    pub upstream_first_byte_ms: Option<i64>,
    pub client_first_write_ms: Option<i64>,
    pub last_upstream_event_ms: Option<i64>,
    pub last_client_write_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RealtimeRequest.ts")]
pub struct RealtimeRequest {
    pub id: String,
    pub started_at: i64,
    pub updated_at: i64,
    pub app: Option<String>,
    pub provider_id: Option<String>,
    pub credential_id: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub wire: Option<String>,
    pub route_prefix: Option<String>,
    pub client_transport: Option<String>,
    pub phase: String,
    pub status_code: Option<i32>,
    pub error: Option<String>,
    pub active_output_tokens_per_sec: Option<f64>,
    pub active_cost_usd_per_hour: Option<f64>,
    pub active_upstream_bytes_per_sec: f64,
    pub active_downstream_bytes_per_sec: f64,
    pub output_tokens_so_far: i64,
    pub upstream_bytes_so_far: i64,
    pub client_bytes_so_far: i64,
    pub upstream_first_byte_ms: Option<i64>,
    pub client_first_write_ms: Option<i64>,
    #[serde(default)]
    pub attempts: Vec<RealtimeAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RealtimeProvider.ts")]
pub struct RealtimeProvider {
    pub provider_id: String,
    pub provider_name: String,
    pub active_requests: usize,
    pub active_output_tokens_per_sec: f64,
    pub active_cost_usd_per_hour: Option<f64>,
    pub active_upstream_bytes_per_sec: f64,
    pub active_downstream_bytes_per_sec: f64,
    pub output_tokens_so_far: i64,
    pub upstream_bytes_so_far: i64,
    pub client_bytes_so_far: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/RealtimeSnapshot.ts")]
pub struct RealtimeSnapshot {
    pub now: i64,
    pub active_requests: Vec<RealtimeRequest>,
    pub recent_requests: Vec<RealtimeRequest>,
    pub providers: Vec<RealtimeProvider>,
    pub active_count: usize,
    pub active_output_tokens_per_sec: f64,
    pub active_cost_usd_per_hour: Option<f64>,
    pub active_upstream_bytes_per_sec: f64,
    pub active_downstream_bytes_per_sec: f64,
    pub codex_ws_active: usize,
    pub codex_last_transport: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialQuotaStatus.ts"
)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialQuotaState {
    Available,
    Warning,
    Exhausted,
    RateLimited,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialQuotaStatus.ts"
)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialQuotaSource {
    ResponseHeaders,
    CodexPlan,
    CredentialState,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialQuotaStatus.ts"
)]
pub struct CredentialQuotaStatus {
    pub credential_id: String,
    pub provider_id: String,
    pub status: CredentialQuotaState,
    pub ready: bool,
    pub source: CredentialQuotaSource,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub quota_data_json: Option<String>,
    #[serde(default)]
    pub next_reset_at: Option<i64>,
    #[serde(default)]
    pub last_checked_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/UsageDailyRollup.ts")]
pub struct UsageDailyRollup {
    pub day: String,
    pub scope: String,
    pub provider_id: String,
    pub credential_id: String,
    pub upstream_id: String,
    pub wire: String,
    pub route_prefix: String,
    pub upstream_model: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub turn_id: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub session_id: String,
    pub requests: i64,
    pub successes: i64,
    pub failures: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub estimated_cost_usd: String,
    pub latency_avg_ms: Option<f64>,
    pub first_token_avg_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/UsageRollupPage.ts")]
pub struct UsageRollupPage {
    pub items: Vec<UsageDailyRollup>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub has_more: bool,
}

/// Paginated request log envelope returned by `GET /_vp/logs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/LogPage.ts")]
pub struct LogPage {
    pub items: Vec<RequestLog>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/Health.ts")]
pub struct Health {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ExtraCredential.ts")]
pub struct ExtraCredential {
    pub label: String,
    pub source_path: String,
    pub token_ok: bool,
}

/// One importable local provider candidate from CC Switch or tool configs.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/LocalCandidate.ts")]
pub struct LocalCandidate {
    pub client: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub auth_ref: Option<String>,
    pub token_ok: bool,
    #[serde(default)]
    pub proxy_managed: bool,
    pub source_path: String,
    #[serde(default)]
    pub default_aliases: Vec<ModelAlias>,
    #[serde(default)]
    pub extra_credentials: Vec<ExtraCredential>,
    /// When non-empty, supersedes single `kind` + `base_url` on import.
    #[serde(default)]
    pub protocols: Vec<ProviderProtocol>,
}

/// Body for `POST /_vp/providers` and the patch shape for `PUT`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProviderInput.ts")]
pub struct ProviderInput {
    pub name: String,
    #[serde(default)]
    pub group_name: Option<String>,
    /// Optional custom avatar/logo URL for the provider, typically discovered from the upstream site.
    #[serde(default)]
    pub avatar_url: Option<String>,
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub protocols: Vec<ProviderProtocol>,
    #[serde(default)]
    pub host: Option<String>,
    pub auth_ref: Option<String>,
    pub enabled: bool,
    pub priority: i32,
    #[serde(default)]
    pub supports_websocket: Option<bool>,
    pub passthrough_mode: bool,
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
#[ts(
    export,
    export_to = "../packages/protocol/types/ProviderHealthSummary.ts"
)]
pub struct ProviderHealthSummary {
    pub cumulative: ProviderHealth,
    pub rolling_hours: i64,
    pub rolling: Option<ProviderStat>,
}

/// Runtime auth/key-pool status for one credential in a provider pool.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialPoolStatus.ts"
)]
pub struct CredentialPoolStatus {
    pub credential_id: String,
    pub label: String,
    pub enabled: bool,
    /// "oauth" or "auth_ref"
    pub auth_mode: String,
    pub circuit_state: String,
    pub circuit_open: bool,
    pub circuit_open_remaining_secs: Option<i64>,
    pub consecutive_failures: i32,
    pub is_rate_limited: bool,
    pub rl_requests_remaining: Option<i64>,
    pub rl_requests_reset_at: Option<i64>,
    pub rl_tokens_remaining: Option<i64>,
    pub rl_tokens_reset_at: Option<i64>,
    pub oauth_expires_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_used_at: Option<i64>,
    /// Rolling-window usage on this credential (hours from ProviderAuthPoolSummary.rolling_hours).
    pub rolling_requests: i64,
    pub rolling_successes: i64,
    pub rolling_failures: i64,
    pub rolling_avg_latency_ms: Option<i64>,
}

/// Unified auth/key-pool observability view for one provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/ProviderAuthPoolSummary.ts"
)]
pub struct ProviderAuthPoolSummary {
    pub provider_id: String,
    pub provider_name: String,
    pub kind: ProviderKind,
    pub rolling_hours: i64,
    pub total_credentials: i64,
    pub enabled_credentials: i64,
    pub available_credentials: i64,
    pub rate_limited_credentials: i64,
    pub open_circuit_credentials: i64,
    pub provider_circuit_open_remaining_secs: Option<i64>,
    pub provider_circuit_state: String,
    pub provider_circuit_open: bool,
    pub provider_last_error: Option<String>,
    pub credentials: Vec<CredentialPoolStatus>,
}

/// One-shot overview used by the Providers page and websocket delta refresh.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/ProvidersOverview.ts")]
pub struct ProvidersOverview {
    pub rolling_hours: i64,
    pub providers: Vec<Provider>,
    pub health: Vec<ProviderHealthSummary>,
    pub pools: Vec<ProviderAuthPoolSummary>,
    pub credentials: HashMap<String, Vec<Credential>>,
    /// Runtime upstream units grouped by UI provider.
    #[serde(default)]
    pub upstreams: HashMap<String, Vec<Upstream>>,
    pub codex_plans: HashMap<String, Vec<ProviderCodexPlanItem>>,
}

/// Latest Codex ChatGPT Plan snapshot parsed from `x-codex-*` response headers.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CredentialPlanSnapshot.ts"
)]
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
#[ts(
    export,
    export_to = "../packages/protocol/types/ProviderCodexPlanItem.ts"
)]
pub struct ProviderCodexPlanItem {
    pub credential_id: String,
    pub label: String,
    pub plan: Option<CredentialPlanSnapshot>,
}

/// Result of `POST /_vp/providers/:id/codex-plan/refresh` or single-credential refresh.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CodexPlanRefreshResult.ts"
)]
pub struct CodexPlanRefreshResult {
    pub attempted: usize,
    pub ok: usize,
    pub errors: Vec<String>,
}

/// Input for previewing or applying Codex history provider unification.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CodexHistoryUnifyInput.ts"
)]
pub struct CodexHistoryUnifyInput {
    pub provider: String,
    #[serde(default)]
    pub from_providers: Vec<String>,
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub no_backup: bool,
    #[serde(default)]
    pub codex_home: Option<String>,
}

/// Summary returned by Codex history preview/apply endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../packages/protocol/types/CodexHistorySummary.ts"
)]
pub struct CodexHistorySummary {
    pub codex_home: String,
    pub provider: String,
    pub from_providers: Vec<String>,
    pub applied: bool,
    pub sqlite_files_seen: usize,
    pub sqlite_files_changed: usize,
    pub sqlite_rows_changed: usize,
    pub rollout_files_seen: usize,
    pub rollout_files_changed: usize,
    pub rollout_fields_changed: usize,
    pub backups_created: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../packages/protocol/types/CodexThreadMeta.ts")]
pub struct CodexThreadMeta {
    pub thread_id: String,
    pub title: String,
    pub cwd: String,
    pub project: String,
    pub source: String,
    pub model: Option<String>,
    pub updated_at: i64,
    pub preview: String,
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
    /// Estimated USD cost summed across the rolling window.
    pub estimated_cost_usd_in_window: String,
    pub success_rate_in_window: f64,
    pub input_tokens_in_window: i64,
    pub output_tokens_in_window: i64,
    /// End-to-end output speed: sum(output_tokens) / sum(latency_ms) for 2xx with latency_ms > 0.
    pub output_tokens_per_sec_in_window: f64,
    /// Decode-phase speed: sum(output_tokens) / sum(latency_ms − first_token_ms) for 2xx rows with valid decode window.
    pub decode_output_tokens_per_sec_in_window: f64,

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
    /// End-to-end: sum(output_tokens) / sum(latency_ms) for 2xx with latency_ms > 0 in this window.
    pub output_tokens_per_sec: f64,
    /// Decode: sum(output_tokens) / sum(latency_ms − first_token_ms) for 2xx with first_token_ms set and latency_ms > first_token_ms.
    pub decode_output_tokens_per_sec: f64,
    /// HTTP status breakdown within the same window as other fields.
    pub err_429: i64,
    pub err_503: i64,
    pub err_4xx_other: i64,
    pub err_5xx_other: i64,
}

/// A single API key / OAuth account attached to a provider.
///
/// Supports two mutually exclusive auth modes:
/// - `auth_ref` — resolves via secrets module (keyring, env, literal…)
/// - `oauth_access_token` — token stored directly in SQLite, auto-refreshed via
///   `oauth_refresh_token` against auth.openai.com.
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
    /// User-facing provider group/pool selected on an upstream management platform.
    /// Distinct from runtime [`Upstream`]: the gateway combines this credential
    /// with each provider endpoint to create upstream units.
    #[serde(default)]
    pub upstream_group: Option<String>,
    /// Cost multiplier relative to official pricing (default 1.0).
    #[serde(default = "default_price_multiplier")]
    pub price_multiplier: f64,
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
    /// From OAuth access JWT when decodable (OpenAI ChatGPT shape, same claims as Codex `parse_chatgpt_jwt_claims`).
    #[serde(default)]
    pub oauth_account_email: Option<String>,
    /// JWT `sub` or ChatGPT user id when present.
    #[serde(default)]
    pub oauth_account_subject: Option<String>,
    /// Raw `chatgpt_plan_type` from `https://api.openai.com/auth` in the JWT (e.g. plus, pro); optional UI hint only.
    #[serde(default)]
    pub oauth_chatgpt_plan_slug: Option<String>,
    /// Model ids fetched with this credential's key (may differ per key).
    #[serde(default)]
    pub remote_models: Vec<String>,
    #[serde(default)]
    pub remote_models_fetched_at: Option<i64>,
    #[serde(default)]
    pub balance: Option<ProviderBalanceSnapshot>,
    #[serde(default)]
    pub usage: Option<ProviderBalanceSnapshot>,
    #[serde(default)]
    pub balance_fetched_at: Option<i64>,
    // ── Upstream vendor / login / group ──────────────────────────────────────
    /// Which management platform this credential belongs to.
    #[serde(default)]
    pub upstream_vendor: Option<CredentialVendor>,
    /// Username for password-based login (NewAPI / Sub2API).
    #[serde(default)]
    pub upstream_username: Option<String>,
    /// Whether a session token is cached (token itself not returned by API).
    #[serde(default)]
    pub upstream_has_session: bool,
    /// Unix timestamp when the cached session expires.
    #[serde(default)]
    pub upstream_session_expires_at: Option<i64>,
    /// Rolling-window usage snapshots fetched from the upstream platform.
    #[serde(default)]
    pub windows: Vec<UsageWindow>,
    /// Why this credential was auto-disabled (e.g. "HTTP 401 from <provider>").
    /// Cleared when an operator re-enables the credential.
    #[serde(default)]
    pub disabled_reason: Option<String>,
    /// Unix seconds when this credential was auto-disabled.
    #[serde(default)]
    pub disabled_at: Option<i64>,
}

fn default_price_multiplier() -> f64 {
    1.0
}

/// Body for `POST /_vp/providers/:id/credentials` and `PUT /_vp/credentials/:id`.
///
/// Set either `auth_ref` (points to a secret) **or** `oauth_access_token` +
/// `oauth_refresh_token` (stored directly in SQLite).  Do not set both.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
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
    /// From Codex `id_token` at import; persisted and merged into Credential `oauth_account_*` for UI.
    #[serde(default)]
    pub oauth_cached_email: Option<String>,
    #[serde(default)]
    pub oauth_cached_subject: Option<String>,
    #[serde(default)]
    pub oauth_cached_plan_slug: Option<String>,
    // ── Upstream vendor / login / group ──────────────────────────────────────
    #[serde(default)]
    pub upstream_vendor: Option<CredentialVendor>,
    #[serde(default)]
    pub upstream_username: Option<String>,
    /// Set to Some to store a new session token; leave None to preserve existing.
    #[serde(default)]
    pub upstream_session: Option<String>,
    #[serde(default)]
    pub upstream_session_expires_at: Option<i64>,
    #[serde(default)]
    pub upstream_group: Option<String>,
    /// Cost multiplier relative to official pricing (default 1.0 = 1:1).
    #[serde(default = "default_price_multiplier")]
    pub price_multiplier: f64,
}

pub fn ts_out_dir() -> &'static str {
    TS_OUT_DIR
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(alias: &str, upstream_model: &str) -> ModelAlias {
        ModelAlias {
            alias: alias.to_string(),
            upstream_model: upstream_model.to_string(),
        }
    }

    fn provider_with(protocols: Vec<ProviderProtocol>, model_aliases: Vec<ModelAlias>) -> Provider {
        Provider {
            id: "provider-1".to_string(),
            name: "Provider 1".to_string(),
            group_name: None,
            avatar_url: None,
            upstreams: vec![],
            upstream_summary: None,
            kind: ProviderKind::OpenaiChat,
            base_url: "https://legacy.example.com/v1".to_string(),
            protocols,
            host: None,
            auth_ref: Some("env:TEST_KEY".to_string()),
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: vec!["remote-model".to_string()],
            remote_models_fetched_at: Some(123),
            last_speedtest: None,
            model_aliases,
            created_at: 1,
            updated_at: 2,
        }
    }

    #[test]
    fn host_from_base_url_extracts_http_hosts_only() {
        assert_eq!(
            host_from_base_url("https://api.deepseek.com/v1"),
            Some("api.deepseek.com".to_string())
        );
        assert_eq!(
            host_from_base_url("  http://api.openai.com:8443/v1/chat/completions  "),
            Some("api.openai.com".to_string())
        );
        assert_eq!(host_from_base_url("api.deepseek.com/v1"), None);
        assert_eq!(host_from_base_url("https:///v1"), None);
        assert_eq!(host_from_base_url(""), None);
    }

    #[test]
    fn canonical_provider_host_normalizes_urls_and_hostnames() {
        assert_eq!(
            canonical_provider_host(" HTTPS://API.DeepSeek.COM/v1 "),
            None,
            "scheme matching is currently case-sensitive"
        );
        assert_eq!(
            canonical_provider_host(" https://www.API.DeepSeek.COM:443/v1 "),
            Some("api.deepseek.com".to_string())
        );
        assert_eq!(
            canonical_provider_host(" www.OpenRouter.AI "),
            Some("openrouter.ai".to_string())
        );
        assert_eq!(canonical_provider_host("   "), None);
    }

    #[test]
    fn host_to_brand_label_covers_known_hosts_and_normalizes_www() {
        assert_eq!(host_to_brand_label(" api.deepseek.com "), Some("DeepSeek"));
        assert_eq!(
            host_to_brand_label("www.api.openrouter.ai"),
            Some("OpenRouter")
        );
        assert_eq!(host_to_brand_label("API.OPENAI.COM"), Some("OpenAI"));
        assert_eq!(host_to_brand_label("unknown.example.com"), None);
    }

    #[test]
    fn host_label_camel_fallback_uses_non_generic_domain_segments() {
        assert_eq!(host_label_camel_fallback("api.deepseek.com"), "Deepseek");
        assert_eq!(host_label_camel_fallback("www.foo-bar.dev"), "Foo-bar");
        assert_eq!(
            host_label_camel_fallback("gateway.eu.example.co.uk"),
            "Gateway Eu Example"
        );
        assert_eq!(host_label_camel_fallback(" api.com "), "api.com");
    }

    #[test]
    fn display_name_for_remote_prefers_branding_then_known_brand_then_fallback() {
        assert_eq!(
            display_name_for_remote(
                Some("  Custom Name  "),
                "https://api.deepseek.com/v1",
                ProviderKind::OpenaiChat
            ),
            "Custom Name"
        );
        assert_eq!(
            display_name_for_remote(
                Some("   "),
                "https://api.deepseek.com/v1",
                ProviderKind::OpenaiChat
            ),
            "DeepSeek"
        );
        assert_eq!(
            display_name_for_remote(
                None,
                "https://api.unknown-provider.example.com/v1",
                ProviderKind::OpenaiChat
            ),
            "Unknown-provider Example"
        );
        assert_eq!(
            display_name_for_remote(None, "not a url", ProviderKind::OpenaiChat),
            "not a url"
        );
    }

    #[test]
    fn provider_kind_slug_and_protocol_display_label_are_stable() {
        let cases = [
            (ProviderKind::Anthropic, "anthropic", "Messages"),
            (ProviderKind::OpenaiChat, "openai-chat", "Chat"),
            (
                ProviderKind::OpenaiResponses,
                "openai-responses",
                "Responses",
            ),
            (ProviderKind::GeminiNative, "gemini-native", "Generate"),
        ];

        for (kind, slug, label) in cases {
            assert_eq!(provider_kind_slug(kind), slug);
            assert_eq!(protocol_display_label(kind), label);
        }
    }

    #[test]
    fn provider_protocol_from_kind_base_sets_kind_url_and_empty_aliases() {
        let proto = ProviderProtocol::from_kind_base(
            ProviderKind::OpenaiResponses,
            "https://api.openai.com/v1",
        );

        assert_eq!(proto.kind, ProviderKind::OpenaiResponses);
        assert_eq!(proto.base_url, "https://api.openai.com/v1");
        assert!(proto.model_aliases.is_empty());
    }

    #[test]
    fn provider_effective_protocols_falls_back_to_legacy_fields_with_aliases() {
        let aliases = vec![alias("high", "claude-sonnet-4")];
        let provider = provider_with(Vec::new(), aliases.clone());

        assert_eq!(
            provider.effective_protocols(),
            vec![ProviderProtocol {
                kind: ProviderKind::OpenaiChat,
                base_url: "https://legacy.example.com/v1".to_string(),
                model_aliases: aliases,
            }]
        );
    }

    #[test]
    fn provider_effective_protocols_uses_stored_protocols_as_is() {
        let protocols = vec![
            ProviderProtocol {
                kind: ProviderKind::Anthropic,
                base_url: "https://messages.example.com".to_string(),
                model_aliases: vec![alias("sonnet", "claude-sonnet-4")],
            },
            ProviderProtocol::from_kind_base(
                ProviderKind::OpenaiResponses,
                "https://responses.example.com/v1",
            ),
        ];
        let provider = provider_with(protocols.clone(), vec![alias("legacy", "legacy-model")]);

        assert_eq!(provider.effective_protocols(), protocols);
    }

    #[test]
    fn provider_primary_protocol_returns_first_effective_protocol() {
        let provider = provider_with(
            vec![
                ProviderProtocol::from_kind_base(
                    ProviderKind::Anthropic,
                    "https://messages.example.com",
                ),
                ProviderProtocol::from_kind_base(
                    ProviderKind::OpenaiChat,
                    "https://chat.example.com/v1",
                ),
            ],
            vec![alias("legacy", "legacy-model")],
        );

        let primary = provider.primary_protocol();
        assert_eq!(primary.kind, ProviderKind::Anthropic);
        assert_eq!(primary.base_url, "https://messages.example.com");
        assert!(primary.model_aliases.is_empty());
    }

    #[test]
    fn provider_protocol_for_kind_matches_requested_kind_or_legacy_fallback() {
        let responses = ProviderProtocol {
            kind: ProviderKind::OpenaiResponses,
            base_url: "https://responses.example.com/v1".to_string(),
            model_aliases: vec![alias("codex", "gpt-5.1-codex")],
        };
        let provider = provider_with(
            vec![
                ProviderProtocol::from_kind_base(
                    ProviderKind::Anthropic,
                    "https://messages.example.com",
                ),
                responses.clone(),
            ],
            vec![alias("legacy", "legacy-model")],
        );

        assert_eq!(
            provider.protocol_for_kind(ProviderKind::OpenaiResponses),
            responses
        );
        assert_eq!(
            provider.protocol_for_kind(ProviderKind::GeminiNative),
            ProviderProtocol::from_kind_base(
                ProviderKind::OpenaiChat,
                "https://legacy.example.com/v1"
            )
        );
    }

    #[test]
    fn provider_with_protocol_updates_wire_fields_and_preserves_aliases_when_proto_aliases_empty() {
        let provider = provider_with(Vec::new(), vec![alias("legacy", "legacy-model")]);
        let proto = ProviderProtocol::from_kind_base(
            ProviderKind::OpenaiResponses,
            "https://responses.example.com/v1",
        );

        let updated = provider.with_protocol(&proto);
        assert_eq!(updated.kind, ProviderKind::OpenaiResponses);
        assert_eq!(updated.base_url, "https://responses.example.com/v1");
        assert_eq!(updated.model_aliases, vec![alias("legacy", "legacy-model")]);
    }

    #[test]
    fn provider_with_protocol_replaces_aliases_when_proto_aliases_are_present() {
        let provider = provider_with(Vec::new(), vec![alias("legacy", "legacy-model")]);
        let proto = ProviderProtocol {
            kind: ProviderKind::Anthropic,
            base_url: "https://messages.example.com".to_string(),
            model_aliases: vec![alias("sonnet", "claude-sonnet-4")],
        };

        let updated = provider.with_protocol(&proto);
        assert_eq!(updated.kind, ProviderKind::Anthropic);
        assert_eq!(updated.base_url, "https://messages.example.com");
        assert_eq!(
            updated.model_aliases,
            vec![alias("sonnet", "claude-sonnet-4")]
        );
    }
}
