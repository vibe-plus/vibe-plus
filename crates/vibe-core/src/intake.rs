//! Smart intake: concurrently probes clipboard/paste/drop detected credential candidates against all providers as needed,
//! letting the frontend confirm which key should be added to which providers in one clear dialog.
//!
//! Design notes:
//! - No persistence, no logs, no circuit-breaker impact: pure probing with an 8s timeout.
//! - Smart-mix probing strategy:
//!   - `openai-chat` / `openai-responses` → `GET {base}/v1/models`（Bearer），
//!     consumes no model tokens;
//!   - `anthropic` -> `POST /v1/messages` with a minimal 1-token inference;
//!   - `gemini-native` → `POST /v1beta/models/{model}:generateContent` 1-token；
//!   - official ChatGPT Codex endpoints (base_url contains `chatgpt.com/backend-api`) support only OAuth,
//!     have no `/v1/models`, and are marked `skipped`.
//! - Credential forms:
//!   - `ApiKey { value }`: user-pasted raw `sk-...`, automatically wrapped with `literal:` on persistence;
//!   - `AuthRef { value }`: already prefixed with `literal:` / `env:` / `keyring:`;
//!   - `Oauth { access, refresh, expires_at }`: stored directly into `credentials.oauth_*`.

use crate::model_defaults;
use crate::secrets;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use axum::{extract::State, Json};
use reqwest::{RequestBuilder, Url};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use vibe_protocol::{
    canonical_provider_host, display_name_for_remote, host_from_base_url,
    host_label_camel_fallback, host_to_brand_label, protocol_display_label, provider_kind_slug,
    Credential, CredentialInput, ModelAlias, Provider, ProviderBalanceSnapshot, ProviderInput,
    ProviderKind, ProviderProtocol, RemoteDetectedProtocol, RemoteProviderCapabilities,
    RemoteProviderPreview,
};

const DEFAULT_PROBE_TIMEOUT_MS: u64 = 8_000;
const ERROR_PREVIEW_CHARS: usize = 280;
const FAKE_PROBE_KEY: &str = "vp-probe-00000000000000000000000000000000";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CandidateAuth {
    /// Raw API key without prefix. Wrapped with `literal:` on persistence.
    ApiKey { value: String },
    /// auth_ref already prefixed with `literal:` / `env:` / `keyring:`.
    AuthRef { value: String },
    /// OAuth triple from auth.json / Codex / Claude login.
    Oauth {
        access: String,
        #[serde(default)]
        refresh: Option<String>,
        #[serde(default)]
        expires_at: Option<i64>,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CandidateHints {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub plan_slug: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IntakeCandidate {
    /// Frontend temporary id used to map results back to UI rows.
    pub id: String,
    /// Display name such as "OPENAI_API_KEY" or "ChatGPT OAuth".
    #[serde(default)]
    pub label: Option<String>,
    pub auth: CandidateAuth,
    #[serde(default)]
    pub hints: Option<CandidateHints>,
}

#[derive(Debug, Deserialize)]
pub struct ProbeInput {
    pub candidates: Vec<IntakeCandidate>,
    #[serde(default)]
    pub provider_ids: Option<Vec<String>>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ProbeResult {
    pub candidate_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_kind: String,
    pub ok: bool,
    pub skipped: bool,
    pub status: Option<u16>,
    pub latency_ms: i64,
    pub error: Option<String>,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProbeResponse {
    pub results: Vec<ProbeResult>,
}

#[derive(Debug, Deserialize)]
pub struct ImportAssignment {
    pub candidate: IntakeCandidate,
    pub provider_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ImportInput {
    pub assignments: Vec<ImportAssignment>,
}

#[derive(Debug, Serialize)]
pub struct ImportError {
    pub candidate_id: String,
    pub provider_id: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub credentials: Vec<Credential>,
    pub errors: Vec<ImportError>,
}

#[derive(Debug, Deserialize)]
pub struct RemoteImportInput {
    pub text: String,
}

pub type RemoteImportResponse = RemoteProviderPreviewWithProvider;

// ---------------------------------------------------------------------------
// Vendor probe (fake-key fingerprinting — no real key required)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct VendorProbeInput {
    pub url: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct VendorDiscovery {
    /// Detected wire protocol: "anthropic", "openai-chat", "gemini-native", "openai-responses".
    pub kind: String,
    pub base_url: String,
    /// True when the server returned 200 on /v1/models with NO auth at all.
    pub no_auth: bool,
    /// Short note explaining how we detected the kind.
    pub note: String,
    /// "high" / "medium" / "low".
    pub confidence: String,
    /// Specific vendor we recognized from headers (e.g. "groq", "openrouter", "lm-studio").
    pub vendor_hint: Option<String>,
    /// Model IDs returned by /v1/models when no_auth is true.
    pub model_ids: Vec<String>,
    /// Management-platform vendor detected by probing admin endpoints.
    /// "new-api" | "sub2-api" | null when unrecognized.
    pub upstream_vendor: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RemoteBranding {
    display_name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RemoteFinancialSnapshot {
    pub balance: Option<ProviderBalanceSnapshot>,
    pub usage: Option<ProviderBalanceSnapshot>,
}

pub type RemotePreviewResponse = RemoteProviderPreview;

#[derive(Debug, Serialize)]
pub struct RemoteProviderPreviewWithProvider {
    pub provider: Provider,
    pub credential: Option<Credential>,
    #[serde(flatten)]
    pub preview: RemoteProviderPreview,
}

#[derive(Debug, Clone)]
struct RemoteSnippet {
    url: String,
    secret: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveryProbe {
    kind: ProviderKind,
    base_url: String,
    note: String,
}

/// Fetch balance/usage snapshot for a credential refresh (OpenAI-compat paths only).
pub(crate) async fn fetch_financials_for_base(
    http: &reqwest::Client,
    kind: ProviderKind,
    base_url: &str,
    secret: &str,
) -> RemoteFinancialSnapshot {
    let discovery = DiscoveryProbe {
        kind,
        base_url: base_url.to_string(),
        note: String::new(),
    };
    fetch_remote_financials(http, &discovery, secret).await
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn probe_handler(
    State(state): State<AppState>,
    Json(input): Json<ProbeInput>,
) -> Result<Json<ProbeResponse>, crate::server::AppError> {
    if input.candidates.is_empty() {
        return Ok(Json(ProbeResponse { results: vec![] }));
    }

    let providers = {
        let state_cl = state.clone();
        tokio::task::spawn_blocking(move || state_cl.db.provider_list()).await??
    };
    let filtered: Vec<Provider> = match input.provider_ids {
        Some(ids) => providers
            .into_iter()
            .filter(|p| ids.iter().any(|x| x == &p.id))
            .collect(),
        None => providers,
    };

    let timeout = Duration::from_millis(input.timeout_ms.unwrap_or(DEFAULT_PROBE_TIMEOUT_MS));

    let mut joins = Vec::new();
    for candidate in &input.candidates {
        for provider in &filtered {
            let http = state.http.clone();
            let candidate = candidate.clone();
            let provider = provider.clone();
            joins.push(tokio::spawn(async move {
                probe_one(&http, &candidate, &provider, timeout).await
            }));
        }
    }

    let mut results = Vec::with_capacity(joins.len());
    for handle in joins {
        match handle.await {
            Ok(r) => results.push(r),
            Err(e) => {
                tracing::warn!(error = %e, "intake.probe task join failed");
            }
        }
    }
    Ok(Json(ProbeResponse { results }))
}

pub async fn import_handler(
    State(state): State<AppState>,
    Json(input): Json<ImportInput>,
) -> Result<Json<ImportResponse>, crate::server::AppError> {
    let mut credentials = Vec::new();
    let mut errors = Vec::new();

    for assignment in input.assignments {
        let candidate_id = assignment.candidate.id.clone();
        let provider_id = assignment.provider_id.clone();
        match build_credential_input(&assignment) {
            Ok(cred_input) => {
                let state_cl = state.clone();
                let pid = provider_id.clone();
                let auth_ref = cred_input.auth_ref.clone();
                let oauth_access = cred_input.oauth_access_token.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let fp = crate::auth_fingerprint::credential_fingerprint(
                        auth_ref.as_deref(),
                        oauth_access.as_deref(),
                    );
                    state_cl.db.credential_insert(&pid, cred_input, Some(fp))
                })
                .await;
                match result {
                    Ok(Ok(mut c)) => {
                        crate::oauth_identity::credential_attach_oauth_identity(&mut c);
                        credentials.push(c);
                    }
                    Ok(Err(e)) => errors.push(ImportError {
                        candidate_id,
                        provider_id,
                        error: e.to_string(),
                    }),
                    Err(e) => errors.push(ImportError {
                        candidate_id,
                        provider_id,
                        error: format!("join: {e}"),
                    }),
                }
            }
            Err(e) => errors.push(ImportError {
                candidate_id,
                provider_id,
                error: e.to_string(),
            }),
        }
    }

    Ok(Json(ImportResponse {
        credentials,
        errors,
    }))
}

pub async fn remote_import_handler(
    State(state): State<AppState>,
    Json(input): Json<RemoteImportInput>,
) -> Result<Json<RemoteImportResponse>, crate::server::AppError> {
    let snippet = parse_remote_snippet(&input.text)?;
    let discoveries = discover_remote_provider(&state.http, &snippet).await?;
    let primary = pick_primary_discovery(&discoveries);
    let discovery = primary.clone();

    let branding = fetch_remote_branding(&state.http, &snippet.url)
        .await
        .unwrap_or_default();
    let host = host_from_base_url(&discovery.base_url)
        .as_deref()
        .and_then(canonical_provider_host);
    let display_name = resolve_remote_display_name(
        &state.http,
        host.as_deref(),
        &discovery.base_url,
        branding.display_name.as_deref(),
    )
    .await;
    let protocols: Vec<ProviderProtocol> = discoveries
        .iter()
        .map(|d| ProviderProtocol::from_kind_base(d.kind, d.base_url.clone()))
        .collect();
    let financials =
        fetch_remote_financials_for_discoveries(&state.http, &discoveries, &snippet.secret).await;
    let provider_input = ProviderInput {
        name: display_name.clone(),
        group_name: None,
        avatar_url: branding.avatar_url.clone(),
        kind: discovery.kind,
        base_url: discovery.base_url.clone(),
        protocols: protocols.clone(),
        host: host.clone(),
        auth_ref: None,
        enabled: true,
        priority: 100,
        supports_websocket: None,
        passthrough_mode: true,
        model_aliases: vec![],
    };
    let credential_input = CredentialInput {
        label: credential_label_from_secret(&snippet.secret),
        auth_ref: Some(format!("literal:{}", snippet.secret)),
        notes: Some("remote-intake".into()),
        enabled: true,
        priority: 100,
        ..CredentialInput::default()
    };

    let provider = upsert_remote_provider(state.clone(), provider_input).await?;
    let credential =
        upsert_remote_credential(state.clone(), &provider.id, credential_input).await?;

    let mut merged_models = Vec::new();
    for probe in &discoveries {
        let batch =
            fetch_upstream_model_ids(&state.http, probe.kind, &probe.base_url, &snippet.secret)
                .await;
        for id in batch {
            if !merged_models.contains(&id) {
                merged_models.push(id);
            }
        }
    }
    merged_models.sort();
    merged_models.dedup();

    let provider = if merged_models.is_empty() {
        provider
    } else {
        let fetched_at = chrono::Utc::now().timestamp();
        let id = provider.id.clone();
        let models = merged_models.clone();
        let state_cl = state.clone();
        tokio::task::spawn_blocking(move || {
            state_cl
                .db
                .provider_update_remote_models(&id, models, fetched_at)
        })
        .await??
    };

    let cred_models = merged_models.clone();
    let cred_id = credential.id.clone();
    let cred_fetched_at = chrono::Utc::now().timestamp();
    let state_cl = state.clone();
    let credential = tokio::task::spawn_blocking(move || {
        state_cl
            .db
            .credential_update_remote_models(&cred_id, cred_models, cred_fetched_at)
    })
    .await??;

    if financials.balance.is_some() || financials.usage.is_some() {
        let cred_id = credential.id.clone();
        let balance = financials.balance.clone();
        let usage = financials.usage.clone();
        let fetched_at = chrono::Utc::now().timestamp();
        let state_cl = state.clone();
        tokio::task::spawn_blocking(move || {
            state_cl
                .db
                .credential_update_financials(&cred_id, balance, usage, fetched_at)
        })
        .await??;
    }

    let preview = build_remote_preview(&discoveries, &provider, &branding, &financials);

    Ok(Json(RemoteImportResponse {
        provider: provider.clone(),
        credential: Some(credential),
        preview,
    }))
}

pub async fn remote_preview_handler(
    State(state): State<AppState>,
    Json(input): Json<RemoteImportInput>,
) -> Result<Json<RemotePreviewResponse>, crate::server::AppError> {
    let snippet = parse_remote_snippet(&input.text)?;
    let discoveries = discover_remote_provider(&state.http, &snippet).await?;
    let primary = pick_primary_discovery(&discoveries);
    let discovery = primary.clone();
    let branding = fetch_remote_branding(&state.http, &snippet.url)
        .await
        .unwrap_or_default();
    let host = host_from_base_url(&discovery.base_url)
        .as_deref()
        .and_then(canonical_provider_host);
    let display_name = resolve_remote_display_name(
        &state.http,
        host.as_deref(),
        &discovery.base_url,
        branding.display_name.as_deref(),
    )
    .await;
    let protocols: Vec<ProviderProtocol> = discoveries
        .iter()
        .map(|d| ProviderProtocol::from_kind_base(d.kind, d.base_url.clone()))
        .collect();
    let mut provider = Provider {
        id: "preview".into(),
        name: display_name,
        group_name: None,
        avatar_url: branding.avatar_url.clone(),
        kind: discovery.kind,
        base_url: discovery.base_url.clone(),
        protocols: protocols.clone(),
        host: host_from_base_url(&discovery.base_url),
        auth_ref: None,
        enabled: true,
        priority: 100,
        supports_websocket: None,
        passthrough_mode: true,
        remote_models: Vec::new(),
        remote_models_fetched_at: None,
        last_speedtest: None,
        model_aliases: vec![],
        created_at: 0,
        updated_at: 0,
    };

    let mut merged_models = Vec::new();
    for probe in &discoveries {
        let batch =
            fetch_upstream_model_ids(&state.http, probe.kind, &probe.base_url, &snippet.secret)
                .await;
        for id in batch {
            if !merged_models.contains(&id) {
                merged_models.push(id);
            }
        }
    }
    merged_models.sort();
    merged_models.dedup();
    provider.remote_models = merged_models;

    let financials =
        fetch_remote_financials_for_discoveries(&state.http, &discoveries, &snippet.secret).await;
    let preview = build_remote_preview(&discoveries, &provider, &branding, &financials);

    Ok(Json(preview))
}

pub async fn vendor_probe_handler(
    State(state): State<AppState>,
    Json(input): Json<VendorProbeInput>,
) -> Result<Json<VendorDiscovery>, crate::server::AppError> {
    let timeout = Duration::from_millis(input.timeout_ms.unwrap_or(DEFAULT_PROBE_TIMEOUT_MS));
    let result = discover_vendor_by_url(&state.http, &input.url, timeout).await?;
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// Probe internals
// ---------------------------------------------------------------------------

async fn probe_one(
    http: &reqwest::Client,
    candidate: &IntakeCandidate,
    provider: &Provider,
    timeout: Duration,
) -> ProbeResult {
    let started = Instant::now();
    let base = ProbeResultBuilder {
        candidate_id: candidate.id.clone(),
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        provider_kind: format!("{:?}", provider.kind).to_ascii_lowercase(),
    };

    let secret = match resolve_candidate_secret(&candidate.auth) {
        Ok(s) => s,
        Err(e) => return base.error(0, format!("auth resolve: {e}")),
    };

    let plan = match plan_probe_request(provider, &secret) {
        Ok(p) => p,
        Err(SkipReason(reason)) => return base.skipped(0, reason),
    };
    let req = plan.build(http, &secret);

    let send = tokio::time::timeout(timeout, req.send()).await;
    let latency_ms = started.elapsed().as_millis() as i64;
    match send {
        Err(_) => base.error(
            latency_ms,
            format!("timeout after {}ms", timeout.as_millis()),
        ),
        Ok(Err(e)) => base.error(latency_ms, e.to_string()),
        Ok(Ok(resp)) => {
            let status = resp.status();
            let ok = status.is_success();
            let error = if ok {
                None
            } else {
                resp.text()
                    .await
                    .ok()
                    .map(|t| t.chars().take(ERROR_PREVIEW_CHARS).collect::<String>())
            };
            ProbeResult {
                candidate_id: base.candidate_id,
                provider_id: base.provider_id,
                provider_name: base.provider_name,
                provider_kind: base.provider_kind,
                ok,
                skipped: false,
                status: Some(status.as_u16()),
                latency_ms,
                error,
                skip_reason: None,
            }
        }
    }
}

struct ProbeResultBuilder {
    candidate_id: String,
    provider_id: String,
    provider_name: String,
    provider_kind: String,
}

impl ProbeResultBuilder {
    fn error(self, latency_ms: i64, msg: String) -> ProbeResult {
        ProbeResult {
            candidate_id: self.candidate_id,
            provider_id: self.provider_id,
            provider_name: self.provider_name,
            provider_kind: self.provider_kind,
            ok: false,
            skipped: false,
            status: None,
            latency_ms,
            error: Some(msg),
            skip_reason: None,
        }
    }

    fn skipped(self, latency_ms: i64, reason: String) -> ProbeResult {
        ProbeResult {
            candidate_id: self.candidate_id,
            provider_id: self.provider_id,
            provider_name: self.provider_name,
            provider_kind: self.provider_kind,
            ok: false,
            skipped: true,
            status: None,
            latency_ms,
            error: None,
            skip_reason: Some(reason),
        }
    }
}

#[derive(Debug)]
struct SkipReason(String);

enum ProbePlan {
    Get {
        url: String,
        bearer: bool,
    },
    Post {
        url: String,
        headers: Vec<(String, String)>,
        body: serde_json::Value,
        bearer: bool,
    },
}

impl ProbePlan {
    fn build(self, http: &reqwest::Client, secret: &str) -> RequestBuilder {
        match self {
            ProbePlan::Get { url, bearer } => {
                let mut req = http.get(url).header("content-type", "application/json");
                if bearer {
                    req = req.bearer_auth(secret);
                }
                req
            }
            ProbePlan::Post {
                url,
                headers,
                body,
                bearer,
            } => {
                let mut req = http.post(url).header("content-type", "application/json");
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                if bearer {
                    req = req.bearer_auth(secret);
                }
                req.json(&body)
            }
        }
    }
}

fn plan_probe_request(provider: &Provider, secret: &str) -> Result<ProbePlan, SkipReason> {
    let base = provider.base_url.trim_end_matches('/').to_string();
    match provider.kind {
        ProviderKind::OpenaiChat | ProviderKind::OpenaiResponses => {
            // ChatGPT Codex endpoints are OAuth-only and have no /v1/models, so skip probing.
            if provider.base_url.contains("chatgpt.com/backend-api") {
                return Err(SkipReason(
                    "ChatGPT Codex endpoints support only OAuth upstream and cannot be probed with an API key.".into(),
                ));
            }
            Ok(ProbePlan::Get {
                url: format!("{base}/v1/models"),
                bearer: true,
            })
        }
        ProviderKind::Anthropic => {
            let model = pick_default_upstream_model(provider, "claude-haiku-4-5-20251001");
            Ok(ProbePlan::Post {
                url: format!("{base}/v1/messages"),
                headers: vec![
                    ("x-api-key".into(), secret.to_string()),
                    ("anthropic-version".into(), "2023-06-01".into()),
                ],
                body: serde_json::json!({
                    "model": model,
                    "max_tokens": 1,
                    "messages": [{ "role": "user", "content": "ping" }],
                }),
                bearer: false,
            })
        }
        ProviderKind::GeminiNative => {
            let model = pick_default_upstream_model(provider, "gemini-2.5-flash-preview-04-17");
            // Gemini passes keys through query parameters, not Bearer or headers.
            let url = format!("{base}/v1beta/models/{model}:generateContent?key={secret}");
            Ok(ProbePlan::Post {
                url,
                headers: vec![],
                body: serde_json::json!({
                    "contents": [{ "role": "user", "parts": [{ "text": "ping" }] }],
                    "generationConfig": { "maxOutputTokens": 1 },
                }),
                bearer: false,
            })
        }
    }
}

fn pick_default_upstream_model(provider: &Provider, fallback: &str) -> String {
    if let Some(alias) = provider.model_aliases.first() {
        return alias.upstream_model.clone();
    }
    if let Some(alias) = model_defaults::default_aliases(provider.kind).first() {
        return alias.upstream_model.clone();
    }
    fallback.to_string()
}

fn resolve_candidate_secret(auth: &CandidateAuth) -> Result<String> {
    match auth {
        CandidateAuth::ApiKey { value } => Ok(value.trim().to_string()),
        CandidateAuth::AuthRef { value } => secrets::resolve(value),
        CandidateAuth::Oauth { access, .. } => Ok(access.trim().to_string()),
    }
}

// ---------------------------------------------------------------------------
// Import internals
// ---------------------------------------------------------------------------

fn build_credential_input(a: &ImportAssignment) -> Result<CredentialInput> {
    let (auth_ref, oauth_access, oauth_refresh, oauth_expires) = match &a.candidate.auth {
        CandidateAuth::ApiKey { value } => {
            let v = value.trim();
            if v.is_empty() {
                return Err(anyhow!("empty api key"));
            }
            (Some(format!("literal:{v}")), None, None, None)
        }
        CandidateAuth::AuthRef { value } => {
            let v = value.trim();
            if v.is_empty() {
                return Err(anyhow!("empty auth_ref"));
            }
            (Some(v.to_string()), None, None, None)
        }
        CandidateAuth::Oauth {
            access,
            refresh,
            expires_at,
        } => (
            None,
            Some(access.trim().to_string()),
            refresh.as_ref().map(|s| s.trim().to_string()),
            *expires_at,
        ),
    };

    let hints = a.candidate.hints.clone().unwrap_or_default();
    let label = a
        .label
        .clone()
        .or_else(|| a.candidate.label.clone())
        .or_else(|| hints.email.clone())
        .unwrap_or_else(|| match &a.candidate.auth {
            CandidateAuth::Oauth { .. } => "OAuth".to_string(),
            _ => "API Key".to_string(),
        });

    Ok(CredentialInput {
        label,
        auth_ref,
        plan_type: a.plan_type.clone().or_else(|| hints.plan_slug.clone()),
        notes: a.notes.clone().or_else(|| Some("intake".into())),
        enabled: a.enabled.unwrap_or(true),
        priority: a.priority.unwrap_or(100),
        oauth_access_token: oauth_access,
        oauth_refresh_token: oauth_refresh,
        oauth_expires_at: oauth_expires,
        oauth_cached_email: hints.email,
        oauth_cached_subject: hints.subject,
        oauth_cached_plan_slug: hints.plan_slug,
        ..CredentialInput::default()
    })
}

fn parse_remote_snippet(raw: &str) -> Result<RemoteSnippet> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty input");
    }

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if v.get("_type").and_then(|x| x.as_str()) == Some("newapi_channel_conn") {
            let url = v
                .get("url")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("newapi_channel_conn missing url"))?;
            let secret = v
                .get("key")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("newapi_channel_conn missing key"))?;
            return Ok(RemoteSnippet {
                url: url.to_string(),
                secret: secret.to_string(),
            });
        }
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 2 {
        let url = parts
            .iter()
            .find(|p| p.starts_with("http://") || p.starts_with("https://"))
            .copied();
        let secret = parts
            .iter()
            .find(|p| !p.starts_with("http://") && !p.starts_with("https://"))
            .copied();
        if let (Some(url), Some(secret)) = (url, secret) {
            return Ok(RemoteSnippet {
                url: url.trim().to_string(),
                secret: secret.trim().to_string(),
            });
        }
    }

    anyhow::bail!("unsupported remote provider snippet")
}

async fn discover_remote_provider(
    http: &reqwest::Client,
    snippet: &RemoteSnippet,
) -> Result<Vec<DiscoveryProbe>> {
    let base = normalize_remote_base(&snippet.url)?;
    let api_root = remote_api_root(&base);

    let mut probes = Vec::new();
    let mut seen = HashSet::new();

    for candidate in [
        (
            ProviderKind::OpenaiResponses,
            normalize_openai_base(&api_root),
            "OpenAI Responses via /v1/models",
        ),
        (
            ProviderKind::OpenaiChat,
            normalize_openai_base(&api_root),
            "OpenAI Chat via /v1/models",
        ),
        (
            ProviderKind::Anthropic,
            normalize_anthropic_base(&api_root),
            "Messages API via /v1/messages",
        ),
        (
            ProviderKind::GeminiNative,
            normalize_gemini_base(&api_root),
            "Gemini Generate via /v1beta/models",
        ),
    ] {
        let key = format!("{}::{}", provider_kind_slug(candidate.0), candidate.1);
        if seen.insert(key) {
            probes.push(DiscoveryProbe {
                kind: candidate.0,
                base_url: candidate.1,
                note: candidate.2.into(),
            });
        }
    }

    let timeout = Duration::from_millis(DEFAULT_PROBE_TIMEOUT_MS);
    let mut matched = Vec::new();
    let mut best_err = String::new();
    for probe in probes {
        if remote_probe_ok(http, &probe, &snippet.secret, timeout).await {
            matched.push(probe);
        } else if best_err.is_empty() {
            best_err = format!("{} {}", provider_kind_slug(probe.kind), probe.base_url);
        }
    }

    if matched.is_empty() {
        anyhow::bail!("could not verify remote provider protocol for {best_err}");
    }

    const ORDER: [ProviderKind; 4] = [
        ProviderKind::OpenaiChat,
        ProviderKind::OpenaiResponses,
        ProviderKind::Anthropic,
        ProviderKind::GeminiNative,
    ];
    matched.sort_by_key(|p| {
        ORDER
            .iter()
            .position(|k| *k == p.kind)
            .unwrap_or(ORDER.len())
    });
    Ok(matched)
}

async fn remote_probe_ok(
    http: &reqwest::Client,
    probe: &DiscoveryProbe,
    secret: &str,
    timeout: Duration,
) -> bool {
    let provider = Provider {
        id: "probe".into(),
        name: "probe".into(),
        group_name: None,
        avatar_url: None,
        kind: probe.kind,
        base_url: probe.base_url.clone(),
        protocols: vec![ProviderProtocol::from_kind_base(
            probe.kind,
            probe.base_url.clone(),
        )],
        host: host_from_base_url(&probe.base_url),
        auth_ref: None,
        enabled: true,
        priority: 100,
        supports_websocket: None,
        passthrough_mode: true,
        remote_models: Vec::new(),
        remote_models_fetched_at: None,
        last_speedtest: None,
        model_aliases: vec![],
        created_at: 0,
        updated_at: 0,
    };
    let plan = match plan_probe_request(&provider, secret) {
        Ok(plan) => plan,
        Err(_) => return false,
    };
    match tokio::time::timeout(timeout, plan.build(http, secret).send()).await {
        Ok(Ok(resp)) => {
            let status = resp.status().as_u16();
            let headers = resp.headers().clone();
            let body = resp.bytes().await.unwrap_or_default();
            response_confirms_probe_kind(probe.kind, status, &headers, &body)
        }
        _ => false,
    }
}

fn is_openai_models_success_body(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    v.get("data").and_then(|x| x.as_array()).is_some()
}

fn is_anthropic_messages_success_body(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    v.get("type").and_then(|x| x.as_str()) == Some("message")
        || v.get("content").is_some()
        || v.get("role").and_then(|x| x.as_str()) == Some("assistant")
}

fn is_gemini_generate_success_body(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    v.get("candidates").is_some() || v.get("usageMetadata").is_some()
}

/// True when the HTTP response body/headers match the wire we probed (not a generic 401 from wrong route).
fn response_confirms_probe_kind(
    probe_kind: ProviderKind,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> bool {
    if (200..300).contains(&status) {
        return match probe_kind {
            ProviderKind::OpenaiChat | ProviderKind::OpenaiResponses => {
                is_openai_models_success_body(body)
            }
            ProviderKind::Anthropic => is_anthropic_messages_success_body(body),
            ProviderKind::GeminiNative => is_gemini_generate_success_body(body),
        };
    }

    if matches!(status, 400 | 401 | 403 | 429) {
        let header_hint = header_kind_hint(headers);
        if let Some((detected, _, _)) = fingerprint_from_error_body(body, header_hint) {
            return wire_kinds_equivalent(detected, probe_kind);
        }
        return false;
    }

    false
}

fn wire_kinds_equivalent(detected: ProviderKind, probe: ProviderKind) -> bool {
    match (detected, probe) {
        (ProviderKind::OpenaiChat, ProviderKind::OpenaiResponses)
        | (ProviderKind::OpenaiResponses, ProviderKind::OpenaiChat) => true,
        (a, b) => a == b,
    }
}

/// List upstream model IDs using the OpenAI-style `/v1/models` route on the API host root.
pub(crate) async fn fetch_upstream_model_ids(
    http: &reqwest::Client,
    kind: ProviderKind,
    base_url: &str,
    secret: &str,
) -> Vec<String> {
    if kind == ProviderKind::GeminiNative {
        return Vec::new();
    }
    let root = normalize_openai_base(base_url);
    let url = format!("{root}/v1/models");
    for use_bearer in [true, false] {
        let req = http.get(&url);
        let req = if use_bearer {
            req.bearer_auth(secret)
        } else {
            req.header("x-api-key", secret)
        };
        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<serde_json::Value>().await {
                    let names = parse_models_json_value(&v);
                    if !names.is_empty() {
                        return names;
                    }
                }
            }
        }
    }
    Vec::new()
}

fn parse_models_json_value(v: &serde_json::Value) -> Vec<String> {
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
    names
}

#[derive(Debug, Deserialize)]
struct ModelsDevProviderEntry {
    name: Option<String>,
    api: Option<String>,
}

async fn models_dev_display_name(http: &reqwest::Client, host: &str) -> Option<String> {
    let want = canonical_provider_host(host)?;
    let data: std::collections::HashMap<String, ModelsDevProviderEntry> = http
        .get("https://models.dev/api.json")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    for prov in data.values() {
        if let Some(api) = prov.api.as_deref() {
            if canonical_provider_host(api).as_deref() == Some(want.as_str()) {
                return prov.name.clone().filter(|n| !n.trim().is_empty());
            }
        }
    }
    for (id, prov) in &data {
        let id_lower = id.to_ascii_lowercase();
        if id_lower.len() >= 4 && want.contains(&id_lower) {
            return prov.name.clone().filter(|n| !n.trim().is_empty());
        }
    }
    None
}

fn marketing_site_url(host: &str) -> Option<String> {
    let h = host.trim().trim_start_matches("www.");
    if let Some(rest) = h.strip_prefix("api.") {
        let site = rest.trim();
        if !site.is_empty() {
            return Some(format!("https://{site}"));
        }
    }
    None
}

async fn resolve_remote_display_name(
    http: &reqwest::Client,
    host: Option<&str>,
    base_url: &str,
    api_branding_name: Option<&str>,
) -> String {
    if let Some(host) = host {
        if let Some(name) = models_dev_display_name(http, host).await {
            return name;
        }
        if let Some(site) = marketing_site_url(host) {
            if let Ok(branding) = fetch_remote_branding(http, &site).await {
                if let Some(name) = branding.display_name.as_deref().filter(|s| !s.is_empty()) {
                    return name.to_string();
                }
            }
        }
        if let Some(brand) = host_to_brand_label(host) {
            return brand.to_string();
        }
        return host_label_camel_fallback(host);
    }
    display_name_for_remote(api_branding_name, base_url, ProviderKind::OpenaiChat)
}

async fn fetch_remote_financials_for_discoveries(
    http: &reqwest::Client,
    discoveries: &[DiscoveryProbe],
    secret: &str,
) -> RemoteFinancialSnapshot {
    let mut out = RemoteFinancialSnapshot::default();
    for probe in discoveries {
        let snap = fetch_remote_financials(http, probe, secret).await;
        if out.balance.is_none() {
            out.balance = snap.balance;
        }
        if out.usage.is_none() {
            out.usage = snap.usage;
        }
        if out.balance.is_some() && out.usage.is_some() {
            break;
        }
    }
    out
}

fn normalize_remote_base(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw.trim())?;
    let mut out = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    let path = url.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    Ok(out)
}

/// API origin only (`scheme://host[:port]`), ignoring path prefixes like `/anthropic`.
fn remote_api_root(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if let Ok(url) = Url::parse(trimmed) {
        if let Some(host) = url.host_str() {
            let mut out = format!("{}://{}", url.scheme(), host);
            if let Some(port) = url.port() {
                out.push(':');
                out.push_str(&port.to_string());
            }
            return out;
        }
    }
    trimmed.to_string()
}

fn normalize_openai_base(base: &str) -> String {
    let root = remote_api_root(base);
    root.trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string()
}

fn normalize_anthropic_base(base: &str) -> String {
    let trimmed = remote_api_root(base).trim_end_matches('/').to_string();
    if let Some(stripped) = trimmed.strip_suffix("/v1") {
        return stripped.to_string();
    }
    if let Some(stripped) = trimmed.strip_suffix("/anthropic") {
        return stripped.to_string();
    }
    trimmed
}

fn normalize_gemini_base(base: &str) -> String {
    remote_api_root(base)
        .trim_end_matches('/')
        .trim_end_matches("/v1beta")
        .to_string()
}

fn pick_primary_discovery<'a>(discoveries: &'a [DiscoveryProbe]) -> &'a DiscoveryProbe {
    const ORDER: [ProviderKind; 4] = [
        ProviderKind::OpenaiChat,
        ProviderKind::OpenaiResponses,
        ProviderKind::Anthropic,
        ProviderKind::GeminiNative,
    ];
    for kind in ORDER {
        if let Some(d) = discoveries.iter().find(|p| p.kind == kind) {
            return d;
        }
    }
    &discoveries[0]
}

async fn fetch_remote_branding(http: &reqwest::Client, raw_url: &str) -> Result<RemoteBranding> {
    let url = Url::parse(raw_url)?;
    let resp = tokio::time::timeout(
        Duration::from_secs(6),
        http.get(url.clone())
            .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml")
            .send(),
    )
    .await
    .map_err(|_| anyhow!("branding fetch timeout"))??;

    if !resp.status().is_success() {
        anyhow::bail!("branding fetch status {}", resp.status());
    }

    let body = resp.text().await?;
    Ok(parse_remote_branding(&url, &body))
}

fn parse_remote_branding(base_url: &Url, html: &str) -> RemoteBranding {
    let doc = Html::parse_document(html);
    let mut out = RemoteBranding::default();
    let meta_selector = Selector::parse("meta").expect("valid meta selector");
    let title_selector = Selector::parse("title").expect("valid title selector");
    let link_selector = Selector::parse("link").expect("valid link selector");
    let mut meta_by_key = std::collections::HashMap::<String, String>::new();

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

    let display_name = [
        "og:site_name",
        "application-name",
        "apple-mobile-web-app-title",
        "og:title",
        "twitter:title",
    ]
    .iter()
    .find_map(|key| meta_by_key.get(*key).cloned())
    .or_else(|| {
        doc.select(&title_selector)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
    })
    .map(|name| sanitize_site_title(&name));

    let avatar_url = ["og:image", "twitter:image", "msapplication-tileimage"]
        .iter()
        .find_map(|key| {
            meta_by_key
                .get(*key)
                .and_then(|value| absolutize_url(base_url, value))
        })
        .or_else(|| {
            let preferred_rels = [
                "apple-touch-icon",
                "apple-touch-icon-precomposed",
                "shortcut icon",
                "icon",
                "mask-icon",
            ];
            for rel in preferred_rels {
                for link in doc.select(&link_selector) {
                    let rel_attr = link.value().attr("rel").unwrap_or("").to_ascii_lowercase();
                    if !rel_attr.contains(rel) {
                        continue;
                    }
                    if let Some(href) = link.value().attr("href") {
                        if let Some(url) = absolutize_url(base_url, href) {
                            return Some(url);
                        }
                    }
                }
            }
            absolutize_url(base_url, "/favicon.ico")
        });

    out.display_name = display_name;
    out.avatar_url = avatar_url;
    out
}

fn sanitize_site_title(raw: &str) -> String {
    let trimmed = raw.trim();
    for sep in ['|', '-', '—', '·'] {
        if let Some((head, _)) = trimmed.split_once(sep) {
            let head = head.trim();
            if !head.is_empty() {
                return head.to_string();
            }
        }
    }
    trimmed.to_string()
}

fn absolutize_url(base_url: &Url, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match base_url.join(raw) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => Some(url.to_string()),
        _ => None,
    }
}

async fn fetch_remote_financials(
    http: &reqwest::Client,
    discovery: &DiscoveryProbe,
    secret: &str,
) -> RemoteFinancialSnapshot {
    let root = normalize_openai_base(&discovery.base_url);
    let mut out = fetch_openai_compatible_financials(http, &root, secret).await;
    if out.balance.is_none() {
        if let Some(balance) = fetch_deepseek_user_balance(http, &root, secret).await {
            out.balance = Some(balance);
        }
    }
    out
}

async fn fetch_deepseek_user_balance(
    http: &reqwest::Client,
    base_url: &str,
    secret: &str,
) -> Option<ProviderBalanceSnapshot> {
    if !base_url.to_ascii_lowercase().contains("deepseek.com") {
        return None;
    }
    let url = format!("{}/user/balance", base_url.trim_end_matches('/'));
    let resp = http.get(url).bearer_auth(secret).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    let infos = v.get("balance_infos")?.as_array()?;
    let first = infos.first()?;
    let currency = first
        .get("currency")
        .and_then(|x| x.as_str())
        .unwrap_or("USD")
        .to_string();
    let total = first
        .get("total_balance")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Some(ProviderBalanceSnapshot {
        currency,
        balance: total.clone(),
        remaining: total,
        used: None,
        total: None,
        period: None,
        note: Some("deepseek user balance".into()),
    })
}

async fn fetch_openai_compatible_financials(
    http: &reqwest::Client,
    base_url: &str,
    secret: &str,
) -> RemoteFinancialSnapshot {
    let mut out = RemoteFinancialSnapshot::default();
    let base = normalize_openai_base(base_url);
    let headers = |req: reqwest::RequestBuilder| req.bearer_auth(secret);

    let credit_url = format!("{base}/api/user/credit_grants");
    if let Ok(Some(snapshot)) = fetch_credit_grants(http, headers(http.get(&credit_url))).await {
        out.balance = Some(snapshot);
    }

    let dashboard_credit_url = format!("{base}/dashboard/billing/credit_grants");
    if out.balance.is_none() {
        if let Ok(Some(snapshot)) =
            fetch_credit_grants(http, headers(http.get(&dashboard_credit_url))).await
        {
            out.balance = Some(snapshot);
        }
    }

    let subscription_url = format!("{base}/dashboard/billing/subscription");
    if out.balance.is_none() {
        if let Ok(Some(snapshot)) =
            fetch_subscription(http, headers(http.get(&subscription_url))).await
        {
            out.balance = Some(snapshot);
        }
    }

    let usage_url = format!("{base}/dashboard/billing/usage");
    if let Ok(Some(snapshot)) = fetch_usage(http, headers(http.get(&usage_url))).await {
        out.usage = Some(snapshot);
    }

    out
}

async fn fetch_credit_grants(
    http: &reqwest::Client,
    req: reqwest::RequestBuilder,
) -> Result<Option<ProviderBalanceSnapshot>> {
    let value = send_json(http, req).await?;
    let total = json_num_string(
        value
            .pointer("/total_granted")
            .or_else(|| value.pointer("/grant_amount")),
    );
    let used = json_num_string(
        value
            .pointer("/total_used")
            .or_else(|| value.pointer("/used_amount")),
    );
    let remaining = json_num_string(
        value
            .pointer("/total_available")
            .or_else(|| value.pointer("/available_amount"))
            .or_else(|| value.pointer("/grants/total_available")),
    );
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

async fn fetch_subscription(
    http: &reqwest::Client,
    req: reqwest::RequestBuilder,
) -> Result<Option<ProviderBalanceSnapshot>> {
    let value = send_json(http, req).await?;
    let total = json_num_string(
        value
            .get("hard_limit_usd")
            .or_else(|| value.get("system_hard_limit_usd")),
    );
    if total.is_none() {
        return Ok(None);
    }
    Ok(Some(ProviderBalanceSnapshot {
        currency: "USD".into(),
        balance: None,
        remaining: None,
        used: None,
        total,
        period: value
            .get("billing_period")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        note: Some("subscription limit".into()),
    }))
}

async fn fetch_usage(
    http: &reqwest::Client,
    req: reqwest::RequestBuilder,
) -> Result<Option<ProviderBalanceSnapshot>> {
    let value = send_json(http, req).await?;
    let used = json_num_string(
        value
            .get("total_usage")
            .or_else(|| value.pointer("/daily_costs/0/line_items/0/cost")),
    );
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

async fn send_json(
    _http: &reqwest::Client,
    req: reqwest::RequestBuilder,
) -> Result<serde_json::Value> {
    let resp = tokio::time::timeout(Duration::from_secs(6), req.send())
        .await
        .map_err(|_| anyhow!("financial fetch timeout"))??;
    if !resp.status().is_success() {
        anyhow::bail!("financial fetch status {}", resp.status());
    }
    Ok(resp.json::<serde_json::Value>().await?)
}

fn json_num_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

fn build_remote_preview(
    discoveries: &[DiscoveryProbe],
    provider: &Provider,
    branding: &RemoteBranding,
    financials: &RemoteFinancialSnapshot,
) -> RemoteProviderPreview {
    let primary = pick_primary_discovery(discoveries);
    let detected_protocols: Vec<RemoteDetectedProtocol> = discoveries
        .iter()
        .map(|d| RemoteDetectedProtocol {
            kind: provider_kind_slug(d.kind).into(),
            label: protocol_display_label(d.kind).into(),
            base_url: d.base_url.clone(),
        })
        .collect();
    let note = if discoveries.len() > 1 {
        discoveries
            .iter()
            .map(|d| format!("{} ({})", protocol_display_label(d.kind), d.base_url))
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        primary.note.clone()
    };
    RemoteProviderPreview {
        detected_kind: provider_kind_slug(primary.kind).into(),
        detected_base_url: primary.base_url.clone(),
        detected_protocols,
        display_name: provider.name.clone(),
        avatar_url: branding
            .avatar_url
            .clone()
            .or_else(|| provider.avatar_url.clone()),
        note,
        passthrough_mode: provider.passthrough_mode,
        remote_models: provider.remote_models.clone(),
        model_aliases: suggested_aliases(provider),
        balance: financials.balance.clone(),
        usage: financials.usage.clone(),
        capabilities: RemoteProviderCapabilities {
            can_fetch_branding: branding.display_name.is_some() || branding.avatar_url.is_some(),
            can_fetch_models: !provider.remote_models.is_empty(),
            can_fetch_balance: financials.balance.is_some(),
            can_fetch_usage: financials.usage.is_some(),
        },
        fetched_at: chrono::Utc::now().timestamp(),
    }
}

fn credential_label_from_secret(secret: &str) -> String {
    let short = if secret.len() > 10 {
        format!(
            "{}…{}",
            &secret[..4],
            &secret[secret.len().saturating_sub(4)..]
        )
    } else {
        secret.to_string()
    };
    format!("API Key {short}")
}

fn default_aliases_for_kind(kind: ProviderKind) -> Vec<ModelAlias> {
    model_defaults::default_aliases(kind)
}

// ---------------------------------------------------------------------------
// Vendor fingerprinting (fake-key probe)
// ---------------------------------------------------------------------------

/// Probe `url` without a real API key and return the best-guess `VendorDiscovery`.
///
/// Strategy:
/// 1. GET /v1/models with NO auth   → 200 means no-auth provider (Ollama, LM Studio, …)
/// 2. GET /v1/models with fake Bearer → read body + headers to fingerprint wire format
/// 3. If /v1/models → 404, POST /v1/messages with fake x-api-key → confirms Anthropic wire
async fn discover_vendor_by_url(
    http: &reqwest::Client,
    raw_url: &str,
    timeout: Duration,
) -> Result<VendorDiscovery> {
    let base = normalize_remote_base(raw_url).map_err(|e| anyhow!("invalid URL: {e}"))?;
    let openai_base = normalize_openai_base(&base);

    // ── Step 0: probe management platform (NewAPI / Sub2API) ─────────────
    // Fire this early; wire detection probes run below. We await the result
    // once at the end and patch it into the struct we already built.
    let upstream_vendor = probe_upstream_vendor(http, &base, timeout).await;

    // ── Step 1: no-auth GET /v1/models ────────────────────────────────────
    let models_url = format!("{openai_base}/v1/models");
    let no_auth_resp = tokio::time::timeout(timeout, http.get(&models_url).send()).await;

    if let Ok(Ok(resp)) = no_auth_resp {
        if resp.status().is_success() {
            let mut model_ids = Vec::new();
            let mut vendor_hint: Option<String> = None;
            vendor_hint = vendor_hint.or_else(|| header_vendor_hint(resp.headers()));
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = body.get("data").and_then(|x| x.as_array()) {
                    for item in arr {
                        if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                            let s = id.trim().to_string();
                            if !s.is_empty() {
                                model_ids.push(s);
                            }
                        }
                    }
                }
            }
            model_ids.sort();
            let kind = kind_from_model_list(&model_ids);
            return Ok(VendorDiscovery {
                kind: provider_kind_slug(kind).to_string(),
                base_url: openai_base,
                no_auth: true,
                note: "GET /v1/models with no auth returned 200 (no authentication required)"
                    .into(),
                confidence: "high".into(),
                vendor_hint,
                model_ids,
                upstream_vendor: upstream_vendor.clone(),
            });
        }
    }

    // ── Step 2: fake Bearer → GET /v1/models ──────────────────────────────
    let fake_bearer_resp = tokio::time::timeout(
        timeout,
        http.get(&models_url).bearer_auth(FAKE_PROBE_KEY).send(),
    )
    .await;

    if let Ok(Ok(resp)) = fake_bearer_resp {
        let status = resp.status().as_u16();
        let vendor_hint = header_vendor_hint(resp.headers());
        let headers_kind = header_kind_hint(resp.headers());
        let body_bytes = resp.bytes().await.unwrap_or_default();

        // 200 with fake key: server ignores auth (some self-hosted proxies)
        if (200..300).contains(&status) {
            let mut model_ids = Vec::new();
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                if let Some(arr) = v.get("data").and_then(|x| x.as_array()) {
                    for item in arr {
                        if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                            let s = id.trim().to_string();
                            if !s.is_empty() {
                                model_ids.push(s);
                            }
                        }
                    }
                }
            }
            model_ids.sort();
            let kind = headers_kind
                .or_else(|| Some(kind_from_model_list(&model_ids)))
                .unwrap_or(ProviderKind::OpenaiChat);
            return Ok(VendorDiscovery {
                kind: provider_kind_slug(kind).to_string(),
                base_url: openai_base,
                no_auth: true,
                note: "GET /v1/models with fake key returned 200 (auth ignored)".into(),
                confidence: "medium".into(),
                vendor_hint,
                model_ids,
                upstream_vendor: upstream_vendor.clone(),
            });
        }

        // 401/403: read error body to distinguish wire format
        if status == 401 || status == 403 || status == 429 {
            if let Some((kind, note, confidence)) =
                fingerprint_from_error_body(&body_bytes, headers_kind)
            {
                return Ok(VendorDiscovery {
                    kind: provider_kind_slug(kind).to_string(),
                    base_url: openai_base,
                    no_auth: false,
                    note,
                    confidence,
                    vendor_hint,
                    model_ids: vec![],
                    upstream_vendor: upstream_vendor.clone(),
                });
            }
        }

        // 404 on /v1/models could be Anthropic (they don't expose that endpoint)
        if status == 404 {
            let anthropic_base = normalize_anthropic_base(&base);
            let anthr_resp = tokio::time::timeout(
                timeout,
                http.post(format!("{anthropic_base}/v1/messages"))
                    .header("x-api-key", FAKE_PROBE_KEY)
                    .header("anthropic-version", "2023-06-01")
                    .json(&serde_json::json!({
                        "model": "claude-haiku-4-5-20251001",
                        "max_tokens": 1,
                        "messages": [{"role": "user", "content": "ping"}]
                    }))
                    .send(),
            )
            .await;

            if let Ok(Ok(aresp)) = anthr_resp {
                let astatus = aresp.status().as_u16();
                if matches!(astatus, 400 | 401 | 403 | 429) {
                    let abytes = aresp.bytes().await.unwrap_or_default();
                    if is_anthropic_error_body(&abytes) {
                        return Ok(VendorDiscovery {
                            kind: provider_kind_slug(ProviderKind::Anthropic).to_string(),
                            base_url: anthropic_base,
                            no_auth: false,
                            note: "POST /v1/messages with fake key returned Anthropic error format"
                                .into(),
                            confidence: "high".into(),
                            vendor_hint,
                            model_ids: vec![],
                            upstream_vendor: upstream_vendor.clone(),
                        });
                    }
                }
            }
        }
    }

    // Fallback: URL-based heuristic only
    let kind =
        crate::model_defaults::detect_kind_from_base_url(&base).unwrap_or(ProviderKind::OpenaiChat);
    Ok(VendorDiscovery {
        kind: provider_kind_slug(kind).to_string(),
        base_url: normalize_openai_base(&base),
        no_auth: false,
        note: "no probe response; kind inferred from URL heuristics only".into(),
        confidence: "low".into(),
        vendor_hint: None,
        model_ids: vec![],
        upstream_vendor,
    })
}

/// Probe a base URL to detect whether it runs NewAPI or Sub2API.
///
/// Fingerprint rules (from live traffic):
/// - `GET /api/v1/auth/me → 401` (not 404) ⇒ Sub2API
/// - `GET /api/user/self  → 401` (not 404) ⇒ NewAPI
/// - HTML title contains "Sub2API"           ⇒ Sub2API (high confidence)
///
/// Returns the `CredentialVendor` kebab-case slug or None when unrecognized.
pub(crate) async fn probe_upstream_vendor(
    http: &reqwest::Client,
    base_url: &str,
    timeout: Duration,
) -> Option<String> {
    let root = base_url.trim_end_matches('/');

    // Check HTML title for explicit "Sub2API" branding first (fast, single request)
    let title_future = async {
        let resp = tokio::time::timeout(timeout, http.get(root).send())
            .await
            .ok()?
            .ok()?;
        let text = resp.text().await.ok()?;
        if text.contains("Sub2API") || text.contains("sub2api") {
            return Some("sub2-api".to_string());
        }
        if text.contains("New API") || text.contains("One API") || text.contains("new-api") {
            return Some("new-api".to_string());
        }
        None
    };

    // Probe both management API endpoints in parallel
    let sub2_future = async {
        let resp = tokio::time::timeout(timeout, http.get(format!("{root}/api/v1/auth/me")).send())
            .await
            .ok()?
            .ok()?;
        let code = resp.status().as_u16();
        // 401 = endpoint exists but auth required → Sub2API
        // 404 = endpoint not found → not Sub2API
        if code == 401 || code == 403 {
            Some("sub2-api")
        } else {
            None
        }
    };

    let newapi_future = async {
        let resp = tokio::time::timeout(timeout, http.get(format!("{root}/api/user/self")).send())
            .await
            .ok()?
            .ok()?;
        let code = resp.status().as_u16();
        if code == 401 || code == 403 {
            Some("new-api")
        } else {
            None
        }
    };

    // Run all three concurrently and take the first definitive answer
    let (title_res, sub2_res, newapi_res) = tokio::join!(title_future, sub2_future, newapi_future);

    // Title takes priority (explicit branding)
    if let Some(v) = title_res {
        return Some(v);
    }
    if let Some(v) = sub2_res {
        return Some(v.to_string());
    }
    if let Some(v) = newapi_res {
        return Some(v.to_string());
    }
    None
}

/// Read response headers for vendor-specific signals.
fn header_vendor_hint(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let h = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    if h("x-groq-request-id").is_some() {
        return Some("groq".into());
    }
    if h("x-kong-request-id").is_some() {
        return Some("openrouter".into());
    }
    if let Some(server) = h("server") {
        let s = server.to_ascii_lowercase();
        if s.starts_with("lm-studio") {
            return Some("lm-studio".into());
        }
        if s.starts_with("ollama") {
            return Some("ollama".into());
        }
    }
    None
}

/// Read headers for a wire-protocol kind signal (without error body).
fn header_kind_hint(headers: &reqwest::header::HeaderMap) -> Option<ProviderKind> {
    let has = |name: &str| headers.get(name).is_some();
    if has("openai-version") || has("openai-processing-ms") {
        return Some(ProviderKind::OpenaiChat);
    }
    if has("anthropic-ratelimit-requests-limit") || has("anthropic-version") {
        return Some(ProviderKind::Anthropic);
    }
    None
}

/// Parse error body JSON to distinguish Anthropic vs OpenAI vs Gemini wire format.
/// Returns `(kind, note, confidence)` on success.
fn fingerprint_from_error_body(
    body: &[u8],
    header_hint: Option<ProviderKind>,
) -> Option<(ProviderKind, String, String)> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;

    // Anthropic: {"type":"error","error":{"type":"authentication_error",...}}
    if is_anthropic_error_body_value(&v) {
        return Some((
            ProviderKind::Anthropic,
            "fake-key probe: /v1/models returned Anthropic error format {\"type\":\"error\",...}"
                .into(),
            "high".into(),
        ));
    }

    // Gemini: {"error":{"code":N,"status":"INVALID_ARGUMENT"|"PERMISSION_DENIED",...}}
    if let Some(err) = v.get("error") {
        if let Some(status) = err.get("status").and_then(|x| x.as_str()) {
            if matches!(
                status,
                "INVALID_ARGUMENT" | "PERMISSION_DENIED" | "UNAUTHENTICATED"
            ) {
                return Some((
                    ProviderKind::GeminiNative,
                    format!("fake-key probe: /v1/models returned Gemini error status \"{status}\""),
                    "high".into(),
                ));
            }
        }

        // OpenAI: {"error":{"code":"invalid_api_key",...}} or {"error":{"type":"invalid_request_error"}}
        let code = err.get("code").and_then(|x| x.as_str()).unwrap_or_default();
        let typ = err.get("type").and_then(|x| x.as_str()).unwrap_or_default();
        if code == "invalid_api_key"
            || code == "missing_api_key"
            || typ == "invalid_request_error"
            || typ == "authentication_error"
        {
            let kind = header_hint.unwrap_or(ProviderKind::OpenaiChat);
            return Some((
                kind,
                format!("fake-key probe: /v1/models returned OpenAI-style error (code={code}, type={typ})"),
                if header_hint.is_some() { "high" } else { "medium" }.into(),
            ));
        }
    }

    // Any other response with a header hint
    if let Some(kind) = header_hint {
        return Some((
            kind,
            "fake-key probe: kind inferred from response headers".into(),
            "medium".into(),
        ));
    }

    None
}

fn is_anthropic_error_body(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .map(|v| is_anthropic_error_body_value(&v))
        .unwrap_or(false)
}

fn is_anthropic_error_body_value(v: &serde_json::Value) -> bool {
    v.get("type").and_then(|x| x.as_str()) == Some("error")
        && v.get("error")
            .and_then(|e| e.get("type"))
            .and_then(|x| x.as_str())
            .map(|t| t.contains("auth") || t.contains("permission"))
            .unwrap_or(false)
}

fn kind_from_model_list(model_ids: &[String]) -> ProviderKind {
    for id in model_ids {
        if let Some(kind) = crate::model_defaults::detect_kind_from_model(id) {
            return kind;
        }
    }
    ProviderKind::OpenaiChat
}

fn suggested_aliases(provider: &Provider) -> Vec<ModelAlias> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for alias in &provider.model_aliases {
        if seen.insert(alias.alias.clone()) {
            out.push(alias.clone());
        }
    }

    for model in &provider.remote_models {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(ModelAlias {
                alias: trimmed.to_string(),
                upstream_model: trimmed.to_string(),
            });
        }
    }

    for alias in default_aliases_for_kind(provider.kind) {
        if provider.remote_models.is_empty()
            || provider
                .remote_models
                .iter()
                .any(|model| model == &alias.upstream_model)
        {
            if seen.insert(alias.alias.clone()) {
                out.push(alias);
            }
        }
    }

    out
}

async fn upsert_remote_provider(state: AppState, input: ProviderInput) -> Result<Provider> {
    let host_key = input
        .host
        .clone()
        .or_else(|| host_from_base_url(&input.base_url))
        .and_then(|h| canonical_provider_host(&h));

    let existing = if let Some(ref hk) = host_key {
        tokio::task::spawn_blocking({
            let state = state.clone();
            let hk = hk.clone();
            move || state.db.provider_find_by_host(&hk)
        })
        .await??
    } else {
        None
    };

    let provider = if let Some(existing) = existing {
        let provider_id = existing.id.clone();
        let state = state.clone();
        tokio::task::spawn_blocking(move || state.db.provider_update(&provider_id, input)).await??
    } else {
        let state = state.clone();
        tokio::task::spawn_blocking(move || state.db.provider_insert(input)).await??
    };

    if let Some(hk) = host_key {
        let keep_id = provider.id.clone();
        let state_cl = state.clone();
        tokio::task::spawn_blocking(move || {
            state_cl.db.provider_consolidate_by_host(&keep_id, &hk)
        })
        .await??;
        let state_cl = state.clone();
        let keep_id = provider.id.clone();
        if let Some(refreshed) =
            tokio::task::spawn_blocking(move || state_cl.db.provider_get(&keep_id)).await??
        {
            return Ok(refreshed);
        }
    }
    Ok(provider)
}

async fn upsert_remote_credential(
    state: AppState,
    provider_id: &str,
    input: CredentialInput,
) -> Result<Credential> {
    let pid = provider_id.to_string();
    let auth_ref = input.auth_ref.clone();
    let oauth_access = input.oauth_access_token.clone();
    let fp = crate::auth_fingerprint::credential_fingerprint(
        auth_ref.as_deref(),
        oauth_access.as_deref(),
    );
    let existing = tokio::task::spawn_blocking({
        let state = state.clone();
        let pid2 = pid.clone();
        let fp2 = fp.clone();
        move || {
            state
                .db
                .credential_get_by_provider_and_fingerprint(&pid2, &fp2)
        }
    })
    .await??;

    if let Some(existing) = existing {
        let cred_id = existing.id.clone();
        return tokio::task::spawn_blocking(move || {
            state.db.credential_update(&cred_id, input, Some(fp))
        })
        .await?;
    }

    tokio::task::spawn_blocking(move || state.db.credential_insert(&pid, input, Some(fp))).await?
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use vibe_protocol::{ModelAlias, ProviderKind};

    fn fake_provider(kind: ProviderKind, base: &str) -> Provider {
        Provider {
            id: "p1".into(),
            name: "p".into(),
            group_name: None,
            avatar_url: None,
            kind,
            base_url: base.into(),
            protocols: vec![],
            host: None,
            auth_ref: None,
            enabled: true,
            priority: 100,
            supports_websocket: None,
            passthrough_mode: true,
            remote_models: Vec::new(),
            remote_models_fetched_at: None,
            last_speedtest: None,
            model_aliases: vec![ModelAlias {
                alias: "a".into(),
                upstream_model: "u".into(),
            }],
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn openai_plan_uses_models_endpoint() {
        let p = fake_provider(ProviderKind::OpenaiChat, "https://api.deepseek.com/");
        let plan = plan_probe_request(&p, "sk-1").expect("planned");
        match plan {
            ProbePlan::Get { url, bearer } => {
                assert!(url.ends_with("/v1/models"), "url={url}");
                assert!(bearer);
            }
            _ => panic!("expected GET /v1/models"),
        }
    }

    #[test]
    fn chatgpt_codex_endpoint_is_skipped() {
        let p = fake_provider(
            ProviderKind::OpenaiResponses,
            "https://chatgpt.com/backend-api/codex",
        );
        assert!(plan_probe_request(&p, "sk-1").is_err());
    }

    #[test]
    fn anthropic_plan_uses_messages() {
        let p = fake_provider(ProviderKind::Anthropic, "https://api.anthropic.com");
        let plan = plan_probe_request(&p, "sk-1").expect("planned");
        match plan {
            ProbePlan::Post { url, headers, .. } => {
                assert!(url.ends_with("/v1/messages"));
                assert!(headers.iter().any(|(k, v)| k == "x-api-key" && v == "sk-1"));
            }
            _ => panic!("expected POST /v1/messages"),
        }
    }

    #[test]
    fn gemini_plan_carries_key_in_query() {
        let p = fake_provider(
            ProviderKind::GeminiNative,
            "https://generativelanguage.googleapis.com/v1beta",
        );
        let plan = plan_probe_request(&p, "AI-xxx").expect("planned");
        match plan {
            ProbePlan::Post { url, .. } => assert!(url.contains("?key=AI-xxx")),
            _ => panic!("expected POST gemini"),
        }
    }

    #[test]
    fn api_key_assignment_wraps_literal_prefix() {
        let assignment = ImportAssignment {
            candidate: IntakeCandidate {
                id: "c1".into(),
                label: None,
                auth: CandidateAuth::ApiKey {
                    value: "sk-abc".into(),
                },
                hints: None,
            },
            provider_id: "p".into(),
            label: None,
            plan_type: None,
            priority: None,
            notes: None,
            enabled: None,
        };
        let input = build_credential_input(&assignment).expect("ok");
        assert_eq!(input.auth_ref.as_deref(), Some("literal:sk-abc"));
        assert!(input.oauth_access_token.is_none());
    }

    #[test]
    fn oauth_assignment_writes_tokens() {
        let assignment = ImportAssignment {
            candidate: IntakeCandidate {
                id: "c1".into(),
                label: Some("user@x".into()),
                auth: CandidateAuth::Oauth {
                    access: "eyJ.aaa.bbb".into(),
                    refresh: Some("r".into()),
                    expires_at: Some(123),
                },
                hints: Some(CandidateHints {
                    email: Some("user@x".into()),
                    subject: None,
                    plan_slug: Some("codex-plus".into()),
                }),
            },
            provider_id: "p".into(),
            label: None,
            plan_type: None,
            priority: None,
            notes: None,
            enabled: None,
        };
        let input = build_credential_input(&assignment).expect("ok");
        assert!(input.auth_ref.is_none());
        assert_eq!(input.oauth_access_token.as_deref(), Some("eyJ.aaa.bbb"));
        assert_eq!(input.oauth_refresh_token.as_deref(), Some("r"));
        assert_eq!(input.oauth_expires_at, Some(123));
        assert_eq!(input.oauth_cached_plan_slug.as_deref(), Some("codex-plus"));
        assert_eq!(input.plan_type.as_deref(), Some("codex-plus"));
    }

    #[test]
    fn remote_snippet_parses_newapi_channel_conn_json() {
        let raw = r#"{"_type":"newapi_channel_conn","key":"sk-test-placeholder-not-a-real-key","url":"https://new-api.example.invalid"}"#;
        let parsed = parse_remote_snippet(raw).expect("parsed");

        assert_eq!(parsed.url, "https://new-api.example.invalid");
        assert_eq!(parsed.secret, "sk-test-placeholder-not-a-real-key");
    }

    #[test]
    fn remote_snippet_parses_url_plus_key() {
        let parsed = parse_remote_snippet("https://proxy.example.invalid sk-test-url-plus-key")
            .expect("parsed");

        assert_eq!(parsed.url, "https://proxy.example.invalid");
        assert_eq!(parsed.secret, "sk-test-url-plus-key");
    }

    #[test]
    fn fingerprint_detects_anthropic_error_body() {
        let body = br#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let result = fingerprint_from_error_body(body, None).expect("detected");
        assert_eq!(result.0, ProviderKind::Anthropic);
        assert_eq!(result.2, "high");
    }

    #[test]
    fn fingerprint_detects_openai_error_body() {
        let body = br#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}"#;
        let result = fingerprint_from_error_body(body, None).expect("detected");
        assert_eq!(result.0, ProviderKind::OpenaiChat);
    }

    #[test]
    fn fingerprint_detects_gemini_error_body() {
        let body =
            br#"{"error":{"code":400,"message":"API key not valid","status":"INVALID_ARGUMENT"}}"#;
        let result = fingerprint_from_error_body(body, None).expect("detected");
        assert_eq!(result.0, ProviderKind::GeminiNative);
        assert_eq!(result.2, "high");
    }

    #[test]
    fn fingerprint_openai_header_hint_elevates_confidence() {
        let body = br#"{"error":{"code":"invalid_api_key","type":"invalid_request_error"}}"#;
        let result =
            fingerprint_from_error_body(body, Some(ProviderKind::OpenaiChat)).expect("detected");
        assert_eq!(result.0, ProviderKind::OpenaiChat);
        assert_eq!(result.2, "high");
    }

    #[test]
    fn kind_from_model_list_prefers_first_match() {
        let ids = vec!["claude-sonnet-4-6".into(), "gpt-4o".into()];
        assert_eq!(kind_from_model_list(&ids), ProviderKind::Anthropic);
    }

    #[test]
    fn kind_from_model_list_falls_back_to_openai_chat() {
        let ids = vec!["llama-3.1-70b".into()];
        assert_eq!(kind_from_model_list(&ids), ProviderKind::OpenaiChat);
    }

    #[test]
    fn remote_base_normalization_preserves_meaningful_path_and_port() {
        let base = normalize_remote_base("https://example.com:8443/proxy/v1/").expect("normalized");

        assert_eq!(base, "https://example.com:8443/proxy/v1");
    }

    #[test]
    fn protocol_base_normalization_avoids_double_version_prefixes() {
        assert_eq!(
            normalize_openai_base("https://agent.example/v1"),
            "https://agent.example"
        );
        assert_eq!(
            normalize_openai_base("https://api.deepseek.com/anthropic/v1"),
            "https://api.deepseek.com"
        );
        assert_eq!(
            normalize_gemini_base("https://generativelanguage.googleapis.com/v1beta"),
            "https://generativelanguage.googleapis.com"
        );
        assert_eq!(
            normalize_anthropic_base("https://api.deepseek.com/anthropic/v1"),
            "https://api.deepseek.com"
        );
        assert_eq!(
            remote_api_root("https://api.deepseek.com/anthropic/v1"),
            "https://api.deepseek.com"
        );
    }

    #[test]
    fn response_confirms_rejects_gemini_fingerprint_on_openai_error() {
        let body = br#"{"error":{"code":"invalid_api_key","type":"invalid_request_error"}}"#;
        assert!(!response_confirms_probe_kind(
            ProviderKind::GeminiNative,
            401,
            &reqwest::header::HeaderMap::new(),
            body,
        ));
        assert!(response_confirms_probe_kind(
            ProviderKind::OpenaiChat,
            401,
            &reqwest::header::HeaderMap::new(),
            body,
        ));
    }

    #[test]
    fn response_confirms_openai_models_list() {
        let body = br#"{"object":"list","data":[{"id":"deepseek-chat"}]}"#;
        assert!(response_confirms_probe_kind(
            ProviderKind::OpenaiChat,
            200,
            &reqwest::header::HeaderMap::new(),
            body,
        ));
    }

    #[test]
    fn suggested_aliases_include_remote_models_first() {
        let mut provider = fake_provider(ProviderKind::OpenaiResponses, "https://example.com");
        provider.model_aliases.clear();
        provider.remote_models = vec!["gpt-5.4".into(), "o4-mini".into()];

        let aliases = suggested_aliases(&provider);

        assert_eq!(aliases[0].alias, "gpt-5.4");
        assert_eq!(aliases[0].upstream_model, "gpt-5.4");
        assert!(aliases.iter().any(|a| a.alias == "o4-mini"));
    }

    #[test]
    fn default_aliases_are_filtered_by_remote_models_when_present() {
        let mut provider = fake_provider(ProviderKind::OpenaiResponses, "https://example.com");
        provider.model_aliases.clear();
        provider.remote_models = vec!["gpt-5.4".into()];

        let aliases = suggested_aliases(&provider);

        assert!(aliases.iter().any(|a| a.alias == "gpt-5.4"));
        assert!(!aliases.iter().any(|a| a.alias == "gpt-5.3-codex"));
    }
}
