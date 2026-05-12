//! Structured editor for Codex `config.toml`.
//!
//! The keys here mirror Codex's Rust config structs:
//! - `ConfigToml.model_provider`
//! - `ConfigToml.model_providers`
//! - `ModelProviderInfo.supports_websockets`
//! - `ModelProviderInfo.websocket_connect_timeout_ms`
//! - `FeaturesToml` boolean entries under `[features]`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

pub const CODEX_PROVIDER_ID: &str = "vibeplus";
pub const DEFAULT_WS_CONNECT_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_REQUEST_MAX_RETRIES: u64 = 4;
const DEFAULT_STREAM_MAX_RETRIES: u64 = 5;
const DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 300_000;

const FEATURE_KEYS: &[(&str, bool, &str)] = &[
    ("terminal_resize_reflow", true, "experimental"),
    ("unified_exec", true, "stable"),
    ("shell_snapshot", true, "stable"),
    ("apply_patch_freeform", true, "stable"),
    ("apps", true, "stable"),
    ("plugins", true, "stable"),
    ("tool_search", true, "stable"),
    ("image_generation", true, "stable"),
    ("workspace_dependencies", true, "stable"),
    ("prevent_idle_sleep", false, "experimental"),
    ("runtime_metrics", false, "under_development"),
    ("remote_compaction_v2", false, "under_development"),
    (
        "responses_websocket_response_processed",
        false,
        "under_development",
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfigSettings {
    pub tool: String,
    pub path: String,
    pub exists: bool,
    pub mtime_ms: Option<i64>,
    pub model_provider: String,
    pub provider: CodexProviderSettings,
    pub features: Vec<CodexFeatureSetting>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexProviderSettings {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub wire_api: String,
    pub requires_openai_auth: bool,
    pub supports_websockets: bool,
    pub websocket_connect_timeout_ms: u64,
    pub request_max_retries: u64,
    pub stream_max_retries: u64,
    pub stream_idle_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexFeatureSetting {
    pub key: String,
    pub enabled: bool,
    pub default_enabled: bool,
    pub stage: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexConfigSettingsInput {
    pub model_provider: Option<String>,
    pub provider: CodexProviderSettingsInput,
    #[serde(default)]
    pub features: Vec<CodexFeatureSettingInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexProviderSettingsInput {
    pub id: Option<String>,
    pub name: Option<String>,
    pub base_url: String,
    pub wire_api: Option<String>,
    pub requires_openai_auth: Option<bool>,
    pub supports_websockets: bool,
    pub websocket_connect_timeout_ms: Option<u64>,
    pub request_max_retries: Option<u64>,
    pub stream_max_retries: Option<u64>,
    pub stream_idle_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexFeatureSettingInput {
    pub key: String,
    pub enabled: bool,
}

pub fn read_settings(path: &Path) -> Result<CodexConfigSettings> {
    let (exists, mtime_ms, raw) = read_config(path)?;
    let doc = parse_document(&raw)?;
    Ok(settings_from_doc(path, exists, mtime_ms, &doc))
}

pub fn write_settings(path: &Path, input: CodexConfigSettingsInput) -> Result<CodexConfigSettings> {
    let (_, _, raw) = read_config(path)?;
    let mut doc = parse_document(&raw)?;
    apply_settings(&mut doc, input)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string())?;
    read_settings(path)
}

pub fn codex_config_path() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            directories::UserDirs::new()
                .map(|dirs| dirs.home_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("~"))
                .join(".codex")
        })
        .join("config.toml")
}

fn read_config(path: &Path) -> Result<(bool, Option<i64>, String)> {
    if !path.exists() {
        return Ok((false, None, String::new()));
    }
    let meta = std::fs::metadata(path)?;
    let raw = std::fs::read_to_string(path)?;
    Ok((true, file_mtime_ms(&meta), raw))
}

fn parse_document(raw: &str) -> Result<DocumentMut> {
    if raw.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    raw.parse::<DocumentMut>()
        .context("invalid TOML in codex config")
}

fn settings_from_doc(
    path: &Path,
    exists: bool,
    mtime_ms: Option<i64>,
    doc: &DocumentMut,
) -> CodexConfigSettings {
    let root = doc.as_table();
    let configured_model_provider = root
        .get("model_provider")
        .and_then(Item::as_str)
        .unwrap_or(CODEX_PROVIDER_ID)
        .to_string();
    let provider_id = provider_id_for_settings(root, &configured_model_provider);
    let provider_table = provider_table(root, &provider_id);
    let provider = provider_from_table(provider_id.clone(), provider_table);
    let features = feature_settings(root.get("features").and_then(Item::as_table));

    CodexConfigSettings {
        tool: "codex".into(),
        path: path.to_string_lossy().to_string(),
        exists,
        mtime_ms,
        model_provider: provider_id.clone(),
        provider,
        features,
    }
}

fn provider_id_for_settings(root: &Table, configured: &str) -> String {
    if provider_table(root, CODEX_PROVIDER_ID).is_some() {
        CODEX_PROVIDER_ID.to_string()
    } else if !is_reserved_provider_id(configured)
        && provider_table(root, configured).is_some_and(provider_looks_vibe_managed)
    {
        configured.to_string()
    } else {
        CODEX_PROVIDER_ID.to_string()
    }
}

fn provider_table<'a>(root: &'a Table, provider_id: &str) -> Option<&'a Table> {
    root.get("model_providers")
        .and_then(Item::as_table)
        .and_then(|mp| mp.get(provider_id))
        .and_then(Item::as_table)
}

fn provider_from_table(id: String, table: Option<&Table>) -> CodexProviderSettings {
    CodexProviderSettings {
        id,
        name: table
            .and_then(|t| t.get("name"))
            .and_then(Item::as_str)
            .unwrap_or("vibe+")
            .to_string(),
        base_url: table
            .and_then(|t| t.get("base_url"))
            .and_then(Item::as_str)
            .unwrap_or("http://127.0.0.1:15917/codex/v1")
            .to_string(),
        wire_api: table
            .and_then(|t| t.get("wire_api"))
            .and_then(Item::as_str)
            .unwrap_or("responses")
            .to_string(),
        requires_openai_auth: table
            .and_then(|t| t.get("requires_openai_auth"))
            .and_then(Item::as_bool)
            .unwrap_or(false),
        supports_websockets: table
            .and_then(|t| t.get("supports_websockets"))
            .and_then(Item::as_bool)
            .unwrap_or(false),
        websocket_connect_timeout_ms: table
            .and_then(|t| t.get("websocket_connect_timeout_ms"))
            .and_then(Item::as_integer)
            .and_then(non_negative_u64)
            .unwrap_or(DEFAULT_WS_CONNECT_TIMEOUT_MS),
        request_max_retries: table
            .and_then(|t| t.get("request_max_retries"))
            .and_then(Item::as_integer)
            .and_then(non_negative_u64)
            .unwrap_or(DEFAULT_REQUEST_MAX_RETRIES),
        stream_max_retries: table
            .and_then(|t| t.get("stream_max_retries"))
            .and_then(Item::as_integer)
            .and_then(non_negative_u64)
            .unwrap_or(DEFAULT_STREAM_MAX_RETRIES),
        stream_idle_timeout_ms: table
            .and_then(|t| t.get("stream_idle_timeout_ms"))
            .and_then(Item::as_integer)
            .and_then(non_negative_u64)
            .unwrap_or(DEFAULT_STREAM_IDLE_TIMEOUT_MS),
    }
}

fn provider_looks_vibe_managed(table: &Table) -> bool {
    table
        .get("base_url")
        .and_then(Item::as_str)
        .is_some_and(|base_url| base_url.contains("/codex/v1"))
        || table
            .get("name")
            .and_then(Item::as_str)
            .is_some_and(|name| name.to_ascii_lowercase().contains("vibe"))
}

fn non_negative_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

fn feature_settings(features_table: Option<&Table>) -> Vec<CodexFeatureSetting> {
    FEATURE_KEYS
        .iter()
        .map(|(key, default_enabled, stage)| CodexFeatureSetting {
            key: (*key).to_string(),
            enabled: features_table
                .and_then(|t| t.get(*key))
                .and_then(Item::as_bool)
                .unwrap_or(*default_enabled),
            default_enabled: *default_enabled,
            stage: (*stage).to_string(),
        })
        .collect()
}

fn apply_settings(doc: &mut DocumentMut, input: CodexConfigSettingsInput) -> Result<()> {
    let provider_id = sanitized_provider_id(
        input
            .provider
            .id
            .as_deref()
            .or(input.model_provider.as_deref())
            .unwrap_or(CODEX_PROVIDER_ID),
    )?;

    let base_url = input.provider.base_url.trim();
    if base_url.is_empty() {
        anyhow::bail!("provider base_url must not be empty");
    }
    let wire_api = input.provider.wire_api.as_deref().unwrap_or("responses");
    if wire_api != "responses" {
        anyhow::bail!("Codex only supports wire_api = \"responses\"");
    }

    let root = doc.as_table_mut();
    root.insert("model_provider", value(provider_id.clone()));
    root.remove("openai_base_url");

    let provider = ensure_provider_table(root, &provider_id)?;
    provider.insert(
        "name",
        value(input.provider.name.as_deref().unwrap_or("vibe+").trim()),
    );
    provider.insert("base_url", value(base_url));
    provider.insert("wire_api", value("responses"));
    provider.insert(
        "requires_openai_auth",
        value(input.provider.requires_openai_auth.unwrap_or(false)),
    );
    provider.insert(
        "supports_websockets",
        value(input.provider.supports_websockets),
    );
    provider.insert(
        "websocket_connect_timeout_ms",
        value(clamp_ms(
            input.provider.websocket_connect_timeout_ms,
            DEFAULT_WS_CONNECT_TIMEOUT_MS,
        ) as i64),
    );
    provider.insert(
        "request_max_retries",
        value(
            input
                .provider
                .request_max_retries
                .unwrap_or(DEFAULT_REQUEST_MAX_RETRIES)
                .min(100) as i64,
        ),
    );
    provider.insert(
        "stream_max_retries",
        value(
            input
                .provider
                .stream_max_retries
                .unwrap_or(DEFAULT_STREAM_MAX_RETRIES)
                .min(100) as i64,
        ),
    );
    provider.insert(
        "stream_idle_timeout_ms",
        value(clamp_ms(
            input.provider.stream_idle_timeout_ms,
            DEFAULT_STREAM_IDLE_TIMEOUT_MS,
        ) as i64),
    );

    apply_feature_settings(root, input.features)?;
    Ok(())
}

fn sanitized_provider_id(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("provider id must not be empty");
    }
    if is_reserved_provider_id(trimmed) {
        anyhow::bail!("provider id `{trimmed}` is reserved by Codex");
    }
    Ok(trimmed.to_string())
}

fn is_reserved_provider_id(provider_id: &str) -> bool {
    matches!(
        provider_id,
        "openai" | "amazon-bedrock" | "ollama" | "lmstudio"
    )
}

fn ensure_provider_table<'a>(root: &'a mut Table, provider_id: &str) -> Result<&'a mut Table> {
    let mp_item = root
        .entry("model_providers")
        .or_insert(Item::Table(Table::new()));
    let mp = mp_item
        .as_table_mut()
        .context("model_providers must be a TOML table")?;
    let provider_item = mp.entry(provider_id).or_insert(Item::Table(Table::new()));
    provider_item
        .as_table_mut()
        .context("model_providers.<provider> must be a TOML table")
}

fn clamp_ms(value: Option<u64>, default_value: u64) -> u64 {
    value.unwrap_or(default_value).clamp(1_000, 600_000)
}

fn apply_feature_settings(root: &mut Table, features: Vec<CodexFeatureSettingInput>) -> Result<()> {
    if features.is_empty() {
        return Ok(());
    }
    let features_item = root.entry("features").or_insert(Item::Table(Table::new()));
    let table = features_item
        .as_table_mut()
        .context("features must be a TOML table")?;

    for feature in features {
        if FEATURE_KEYS.iter().any(|(key, _, _)| *key == feature.key) {
            table.insert(feature.key.as_str(), value(feature.enabled));
        }
    }
    Ok(())
}

fn file_mtime_ms(meta: &std::fs::Metadata) -> Option<i64> {
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_defaults_when_file_missing() {
        let settings = settings_from_doc(
            Path::new("/tmp/config.toml"),
            false,
            None,
            &DocumentMut::new(),
        );

        assert_eq!(settings.model_provider, CODEX_PROVIDER_ID);
        assert_eq!(settings.provider.id, CODEX_PROVIDER_ID);
        assert!(!settings.provider.supports_websockets);
        assert_eq!(
            settings.provider.websocket_connect_timeout_ms,
            DEFAULT_WS_CONNECT_TIMEOUT_MS
        );
        assert!(settings.features.iter().any(|f| f.key == "unified_exec"));
    }

    #[test]
    fn write_preserves_unrelated_toml() {
        let mut doc: DocumentMut = r#"
model = "gpt-5.3-codex"

[projects."/tmp/example"]
trust_level = "trusted"
"#
        .parse()
        .unwrap();

        apply_settings(
            &mut doc,
            CodexConfigSettingsInput {
                model_provider: Some("vibeplus".into()),
                provider: CodexProviderSettingsInput {
                    id: Some("vibeplus".into()),
                    name: Some("vibe+".into()),
                    base_url: "http://127.0.0.1:15917/codex/v1".into(),
                    wire_api: Some("responses".into()),
                    requires_openai_auth: Some(false),
                    supports_websockets: false,
                    websocket_connect_timeout_ms: Some(10_000),
                    request_max_retries: Some(4),
                    stream_max_retries: Some(5),
                    stream_idle_timeout_ms: Some(300_000),
                },
                features: vec![CodexFeatureSettingInput {
                    key: "terminal_resize_reflow".into(),
                    enabled: false,
                }],
            },
        )
        .unwrap();

        let out = doc.to_string();
        assert!(out.contains("model = \"gpt-5.3-codex\""));
        assert!(out.contains("[projects.\"/tmp/example\"]"));
        assert!(out.contains("model_provider = \"vibeplus\""));
        assert!(out.contains("supports_websockets = false"));
        assert!(out.contains("terminal_resize_reflow = false"));
    }

    #[test]
    fn custom_non_vibe_provider_is_not_selected_for_editing() {
        let doc: DocumentMut = r#"
model_provider = "openrouter"

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
wire_api = "responses"
"#
        .parse()
        .unwrap();

        let settings = settings_from_doc(Path::new("/tmp/config.toml"), true, None, &doc);

        assert_eq!(settings.model_provider, CODEX_PROVIDER_ID);
        assert_eq!(settings.provider.id, CODEX_PROVIDER_ID);
        assert_eq!(
            settings.provider.base_url,
            "http://127.0.0.1:15917/codex/v1"
        );
    }
}
