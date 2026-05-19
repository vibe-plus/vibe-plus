//! Map CC Switch provider rows into Vibe+ `ProviderInput` + optional credential material.

use super::types::{CcSwitchAppType, CcSwitchProvider};
use crate::model_defaults::{default_aliases, detect_kind_from_base_url};
use anyhow::{Context, Result};
use serde_json::Value;
use vibe_protocol::{ProviderInput, ProviderKind, ProviderProtocol};

const CCSWITCH_CLIENT_PREFIX: &str = "ccswitch";

pub fn ccswitch_client_id(app: &str, provider_id: &str) -> String {
    format!("{CCSWITCH_CLIENT_PREFIX}:{app}:{provider_id}")
}

pub fn parse_ccswitch_client_id(client: &str) -> Option<(&str, &str)> {
    let rest = client.strip_prefix(&format!("{CCSWITCH_CLIENT_PREFIX}:"))?;
    let (app, id) = rest.split_once(':')?;
    if app.is_empty() || id.is_empty() {
        return None;
    }
    Some((app, id))
}

pub struct CcSwitchImportDraft {
    pub client: String,
    pub provider: ProviderInput,
    pub credential_auth_ref: Option<String>,
    pub source_path: String,
    pub token_ok: bool,
}

pub fn draft_from_ccswitch(
    row: &CcSwitchProvider,
    cc_root: &std::path::Path,
) -> Result<Option<CcSwitchImportDraft>> {
    let app = CcSwitchAppType::parse(row.app_type.as_str())
        .with_context(|| format!("unknown cc-switch app_type {}", row.app_type))?;

    let (base_url, wire_hint, api_key) = extract_connection(&row.settings_config, app)?;
    let Some(base_url) = base_url else {
        return Ok(None);
    };

    let primary_kind = map_primary_kind(app, &base_url, wire_hint.as_deref(), row.meta.as_ref());
    let protocols = openai_compatible_protocols(&base_url, primary_kind);
    let aliases = default_aliases(primary_kind);

    let name = if row.name.trim().is_empty() {
        fallback_display_name(&base_url)
    } else {
        row.name.trim().to_string()
    };

    let group_name = row
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let provider = ProviderInput {
        name,
        group_name,
        avatar_url: None,
        kind: protocols.first().map(|p| p.kind).unwrap_or(primary_kind),
        base_url: protocols
            .first()
            .map(|p| p.base_url.clone())
            .unwrap_or(base_url),
        protocols,
        host: None,
        auth_ref: None,
        enabled: true,
        priority: row.sort_index.map(|i| i as i32).unwrap_or(100),
        supports_websocket: None,
        passthrough_mode: true,
        model_aliases: aliases,
    };

    let credential_auth_ref = api_key.map(|k| format!("literal:{k}"));
    let token_ok = credential_auth_ref.is_some();

    Ok(Some(CcSwitchImportDraft {
        client: ccswitch_client_id(app.as_str(), &row.id),
        provider,
        credential_auth_ref,
        source_path: cc_root.display().to_string(),
        token_ok,
    }))
}

fn map_primary_kind(
    app: CcSwitchAppType,
    base_url: &str,
    wire_hint: Option<&str>,
    meta: Option<&crate::ccswitch::types::CcSwitchProviderMeta>,
) -> ProviderKind {
    if let Some(fmt) = meta.and_then(|m| m.api_format.as_deref()) {
        if let Some(kind) = map_api_format(fmt) {
            return kind;
        }
    }
    if let Some(wire) = wire_hint {
        if wire.eq_ignore_ascii_case("chat") || wire.eq_ignore_ascii_case("chat_completions") {
            return ProviderKind::OpenaiChat;
        }
        if wire.eq_ignore_ascii_case("responses") {
            return ProviderKind::OpenaiResponses;
        }
    }
    match app {
        CcSwitchAppType::Claude => ProviderKind::Anthropic,
        CcSwitchAppType::Gemini => ProviderKind::GeminiNative,
        CcSwitchAppType::Codex | CcSwitchAppType::Opencode | CcSwitchAppType::Openclaw => {
            detect_kind_from_base_url(base_url).unwrap_or(ProviderKind::OpenaiResponses)
        }
        CcSwitchAppType::Hermes => {
            detect_kind_from_base_url(base_url).unwrap_or(ProviderKind::OpenaiResponses)
        }
    }
}

fn map_api_format(fmt: &str) -> Option<ProviderKind> {
    match fmt.trim().to_ascii_lowercase().as_str() {
        "anthropic" => Some(ProviderKind::Anthropic),
        "openai_chat" | "openai-chat" | "chat" => Some(ProviderKind::OpenaiChat),
        "openai_responses" | "openai-responses" | "responses" => {
            Some(ProviderKind::OpenaiResponses)
        }
        "gemini_native" | "gemini-native" | "gemini" => Some(ProviderKind::GeminiNative),
        _ => None,
    }
}

fn openai_compatible_protocols(base_url: &str, primary: ProviderKind) -> Vec<ProviderProtocol> {
    if matches!(
        primary,
        ProviderKind::Anthropic | ProviderKind::GeminiNative
    ) {
        return vec![ProviderProtocol {
            kind: primary,
            base_url: base_url.to_string(),
            model_aliases: Vec::new(),
        }];
    }
    let lower = base_url.to_ascii_lowercase();
    if lower.contains("api.openai.com") {
        return vec![
            ProviderProtocol {
                kind: ProviderKind::OpenaiResponses,
                base_url: base_url.to_string(),
                model_aliases: Vec::new(),
            },
            ProviderProtocol {
                kind: ProviderKind::OpenaiChat,
                base_url: base_url.to_string(),
                model_aliases: Vec::new(),
            },
        ];
    }
    // Third-party OpenAI-compatible relays: Responses first so Codex works out of the box.
    vec![
        ProviderProtocol {
            kind: ProviderKind::OpenaiResponses,
            base_url: base_url.to_string(),
            model_aliases: Vec::new(),
        },
        ProviderProtocol {
            kind: ProviderKind::OpenaiChat,
            base_url: base_url.to_string(),
            model_aliases: Vec::new(),
        },
    ]
}

fn extract_connection(
    settings: &Value,
    app: CcSwitchAppType,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    match app {
        CcSwitchAppType::Claude | CcSwitchAppType::Gemini => {
            let env = settings.get("env").and_then(Value::as_object);
            let base_url = env
                .and_then(|e| {
                    e.get("ANTHROPIC_BASE_URL")
                        .or_else(|| e.get("GOOGLE_GEMINI_BASE_URL"))
                })
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let api_key = env
                .and_then(|e| {
                    e.get("ANTHROPIC_API_KEY")
                        .or_else(|| e.get("GEMINI_API_KEY"))
                        .or_else(|| e.get("OPENAI_API_KEY"))
                })
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            Ok((base_url, None, api_key))
        }
        CcSwitchAppType::Codex => {
            let api_key = settings
                .get("auth")
                .and_then(|a| a.get("OPENAI_API_KEY"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let config_toml = settings.get("config").and_then(Value::as_str).unwrap_or("");
            let (base_url, wire) = parse_codex_config_toml(config_toml)?;
            Ok((base_url, wire, api_key))
        }
        CcSwitchAppType::Opencode | CcSwitchAppType::Openclaw | CcSwitchAppType::Hermes => {
            let base_url = settings
                .get("baseUrl")
                .or_else(|| settings.get("base_url"))
                .or_else(|| settings.pointer("/options/baseURL"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let api_key = settings
                .get("apiKey")
                .or_else(|| settings.get("api_key"))
                .or_else(|| settings.pointer("/options/apiKey"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let wire = settings
                .get("api")
                .or_else(|| settings.get("wire_api"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Ok((base_url, wire, api_key))
        }
    }
}

fn parse_codex_config_toml(raw: &str) -> Result<(Option<String>, Option<String>)> {
    if raw.trim().is_empty() {
        return Ok((None, None));
    }
    let doc: toml::Value = toml::from_str(raw).context("parse codex config toml")?;
    let mut base_url = toml_string(doc.get("base_url"));
    let mut wire_api = toml_string(doc.get("wire_api"));

    if let Some(table) = doc.get("model_providers").and_then(|v| v.as_table()) {
        for (_name, provider) in table {
            if base_url.is_none() {
                base_url = toml_string(provider.get("base_url"));
            }
            if wire_api.is_none() {
                wire_api = toml_string(provider.get("wire_api"));
            }
        }
    }
    Ok((base_url, wire_api))
}

fn toml_string(v: Option<&toml::Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn fallback_display_name(base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| base_url.to_string())
}
