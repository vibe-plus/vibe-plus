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

mod remote;

pub use remote::*;

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
