//! Scan locally installed AI coding tools for **one-time import** into SQLite.
//!
//! Codex `auth.json` is read only during import POST — tokens are stored in
//! `credentials.oauth_*`; runtime no longer uses `codex-auth` file schemes.
//!
//! Claude Code layout follows official docs: user settings under `CLAUDE_CONFIG_DIR`
//! (default `~/.claude/`) in `settings.json` (`env` block); optional OAuth /
//! state in `~/.claude.json`. Some installs also keep sidecar `credentials.json`
//! or `.env` next to settings.

use crate::codex_auth_json::{credential_input_from_codex_tokens, parse_codex_auth_json};
use crate::model_defaults;
use base64::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use url::Url;
use vibe_protocol::{CredentialInput, ModelAlias, ProviderInput, ProviderKind};
use rusqlite;

fn is_localhost_url(url: &str) -> bool {
    let s = url.trim_start_matches("http://").trim_start_matches("https://");
    s.starts_with("127.") || s.starts_with("localhost") || s.starts_with("[::1]")
}

/// One importable local provider candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCandidate {
    pub client: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    /// Runtime auth hint (`literal:`, `env:`, …). Never `codex-auth` (removed).
    #[serde(default)]
    pub auth_ref: Option<String>,
    pub token_ok: bool,
    pub source_path: String,
    pub default_aliases: Vec<ModelAlias>,
    pub extra_credentials: Vec<ExtraCredential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtraCredential {
    pub label: String,
    pub source_path: String,
    pub token_ok: bool,
}

pub fn scan() -> Vec<LocalCandidate> {
    let mut out = Vec::new();
    if let Some(c) = scan_claude() {
        out.push(c);
    }
    if let Some(c) = scan_codex() {
        out.push(c);
    }
    // CC Switch DB comes first — it has the real upstream URLs and API keys.
    // Profile files come second for backwards compat with older CCS installs.
    out.extend(scan_ccs_db());
    out.extend(scan_ccs_profiles());
    out
}

fn home() -> Option<PathBuf> {
    directories::UserDirs::new().map(|d| d.home_dir().to_path_buf())
}

fn claude_config_dir(home: &Path) -> PathBuf {
    std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home.join(".claude"))
}

fn env_nonempty(name: &str) -> bool {
    std::env::var(name)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn expand_home_path(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(raw)
}

/// Prefer `ANTHROPIC_AUTH_TOKEN` then `ANTHROPIC_API_KEY` (matches Claude Code env semantics).
pub fn anthropic_env_auth_ref() -> Option<String> {
    if env_nonempty("ANTHROPIC_AUTH_TOKEN") {
        return Some("env:ANTHROPIC_AUTH_TOKEN".into());
    }
    if env_nonempty("ANTHROPIC_API_KEY") {
        return Some("env:ANTHROPIC_API_KEY".into());
    }
    None
}

fn anthropic_token_resolvable(auth_ref: Option<&String>) -> bool {
    if let Some(r) = auth_ref {
        if crate::secrets::resolve(r).is_ok() {
            return true;
        }
    }
    anthropic_env_auth_ref().is_some()
}

/// Map a single `env` value from Claude settings / dotenv into a gateway `auth_ref`.
fn claude_env_string_to_auth_ref(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s == "PROXY_MANAGED" {
        return None;
    }
    if let Some(inner) = s.strip_prefix("${").and_then(|x| x.strip_suffix('}')) {
        let name = inner.trim();
        if !name.is_empty() {
            return Some(format!("env:{name}"));
        }
    }
    if let Some(rest) = s.strip_prefix('$') {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Some(format!("env:{rest}"));
        }
    }
    // Same spelling as an exported env var name → resolve via env at runtime.
    if looks_like_shell_env_name(s) && env_nonempty(s) {
        return Some(format!("env:{s}"));
    }
    Some(format!("literal:{s}"))
}

fn looks_like_shell_env_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn merge_json_env_into(acc: &mut HashMap<String, String>, v: &Value) {
    let Some(env) = v.get("env").and_then(|e| e.as_object()) else {
        return;
    };
    for (k, val) in env {
        if let Some(t) = val.as_str() {
            acc.insert(k.clone(), t.to_string());
        }
    }
}

fn parse_dotenv_anthropic(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, raw_val)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key != "ANTHROPIC_AUTH_TOKEN" && key != "ANTHROPIC_API_KEY" {
            continue;
        }
        let mut val = raw_val.trim().to_string();
        if val.len() >= 2
            && ((val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\'')))
        {
            val = val[1..val.len().saturating_sub(1)].to_string();
        }
        if let Some(ar) = claude_env_string_to_auth_ref(&val) {
            return Some(ar);
        }
    }
    None
}

fn try_credentials_json(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    for ptr in [
        "/apiKey",
        "/api_key",
        "/anthropicApiKey",
        "/ANTHROPIC_API_KEY",
        "/anthropic_api_key",
    ] {
        if let Some(tok) = v.pointer(ptr).and_then(|t| t.as_str()) {
            if let Some(ar) = claude_env_string_to_auth_ref(tok) {
                return Some(ar);
            }
        }
    }
    None
}

/// Merge `env` maps from `settings.json` then optional keys from `~/.claude.json` (fill-only).
fn collect_claude_env(home: &Path, config_dir: &Path) -> HashMap<String, String> {
    let mut acc = HashMap::new();

    let settings_path = config_dir.join("settings.json");
    if let Ok(s) = std::fs::read_to_string(&settings_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            merge_json_env_into(&mut acc, &v);
        }
    }

    let global_path = home.join(".claude.json");
    if let Ok(s) = std::fs::read_to_string(&global_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(env) = v.get("env").and_then(|e| e.as_object()) {
                for (k, val) in env {
                    if acc.contains_key(k) {
                        continue;
                    }
                    if let Some(t) = val.as_str() {
                        acc.insert(k.clone(), t.to_string());
                    }
                }
            }
        }
    }

    acc
}

fn auth_ref_from_claude_env_map(env: &HashMap<String, String>) -> Option<String> {
    for key in ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"] {
        if let Some(raw) = env.get(key) {
            if let Some(ar) = claude_env_string_to_auth_ref(raw) {
                return Some(ar);
            }
        }
    }
    None
}

fn pick_claude_source_path(home: &Path, config_dir: &Path) -> String {
    let candidates = [
        config_dir.join("settings.json"),
        config_dir.join("credentials.json"),
        config_dir.join(".env"),
        home.join(".claude.json"),
    ];
    for p in candidates {
        if p.exists() {
            return p.display().to_string();
        }
    }
    if config_dir.exists() {
        return config_dir.display().to_string();
    }
    anthropic_env_auth_ref()
        .map(|_| "(environment)".into())
        .unwrap_or_else(|| config_dir.display().to_string())
}

fn claude_install_detected(home: &Path, config_dir: &Path) -> bool {
    config_dir.exists() || home.join(".claude.json").exists() || anthropic_env_auth_ref().is_some()
}

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

fn scan_claude() -> Option<LocalCandidate> {
    let home = home()?;
    let config_dir = claude_config_dir(&home);
    if !claude_install_detected(&home, &config_dir) {
        return None;
    }

    let merged_env = collect_claude_env(&home, &config_dir);
    let mut auth_ref = auth_ref_from_claude_env_map(&merged_env);

    let cred_path = config_dir.join("credentials.json");
    if auth_ref.is_none() {
        auth_ref = try_credentials_json(&cred_path);
    }
    let dotenv_path = config_dir.join(".env");
    if auth_ref.is_none() {
        auth_ref = parse_dotenv_anthropic(&dotenv_path);
    }
    if auth_ref.is_none() {
        auth_ref = anthropic_env_auth_ref();
    }

    let token_ok = anthropic_token_resolvable(auth_ref.as_ref());
    let source_path = pick_claude_source_path(&home, &config_dir);

    Some(LocalCandidate {
        client: "claude".into(),
        name: "Claude".into(),
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com".into(),
        auth_ref,
        token_ok,
        source_path,
        default_aliases: vec![],
        extra_credentials: vec![],
    })
}

// ---------------------------------------------------------------------------
// Codex CLI — multiple ~/.codex/auth*.json
// ---------------------------------------------------------------------------

fn scan_codex() -> Option<LocalCandidate> {
    let codex_dir = home()?.join(".codex");

    let mut auth_files: Vec<PathBuf> = Vec::new();
    let primary = codex_dir.join("auth.json");
    if primary.exists() {
        auth_files.push(primary);
    }

    if let Ok(entries) = std::fs::read_dir(&codex_dir) {
        let mut extras: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("auth-") && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();
        extras.sort();
        auth_files.extend(extras);
    }

    let first = auth_files.first()?;
    let primary_ok = std::fs::read_to_string(first)
        .ok()
        .and_then(|content| parse_codex_auth_json(&content).ok())
        .is_some();

    let (base_url, name) = codex_base_url_from_auth(first);

    let extra_credentials: Vec<ExtraCredential> = auth_files[1..]
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let label = p
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.strip_prefix("auth-").unwrap_or(s))
                .map(|s| format!("Codex account: {s}"))
                .unwrap_or_else(|| format!("Codex account #{}", i + 2));
            let token_ok = std::fs::read_to_string(p)
                .ok()
                .and_then(|content| parse_codex_auth_json(&content).ok())
                .is_some();
            ExtraCredential {
                label,
                source_path: p.display().to_string(),
                token_ok,
            }
        })
        .collect();

    Some(LocalCandidate {
        client: "codex".into(),
        name,
        kind: ProviderKind::OpenaiResponses,
        base_url,
        auth_ref: None,
        token_ok: primary_ok,
        source_path: first.display().to_string(),
        default_aliases: vec![],
        extra_credentials,
    })
}

fn codex_base_url_from_auth(path: &PathBuf) -> (String, String) {
    let try_read = || -> Option<String> {
        let s = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&s).ok()?;
        v.get("auth_mode")?.as_str().map(|s| s.to_string())
    };

    match try_read().as_deref() {
        Some("chatgpt") => (
            "https://chatgpt.com/backend-api/codex".into(),
            "Codex".into(),
        ),
        _ => ("https://api.openai.com".into(), "Codex".into()),
    }
}

// ---------------------------------------------------------------------------
// CC Switch / CCS API profiles — existing profile and ccswitch:// syntax
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct CcsProfileCandidate {
    client: String,
    name: String,
    source_path: PathBuf,
    settings: Value,
}

fn scan_ccs_profiles() -> Vec<LocalCandidate> {
    let Some(home) = home() else {
        return vec![];
    };
    let ccs_dir = ccs_dir(&home);
    if !ccs_dir.exists() {
        return vec![];
    }

    let mut profiles = Vec::new();
    profiles.extend(ccs_profiles_from_config_yaml(&home, &ccs_dir));
    profiles.extend(ccs_profiles_from_config_json(&home, &ccs_dir));
    profiles.extend(ccs_profile_orphans(&ccs_dir));
    profiles.sort_by(|a, b| a.client.cmp(&b.client));
    profiles.dedup_by(|a, b| a.client == b.client);

    profiles
        .into_iter()
        .filter_map(|p| ccs_profile_to_candidate(&p))
        // Skip profiles that route through CC Switch's local proxy — the real upstream
        // data comes from scan_ccs_db(). Keep these only for older CCS installs that
        // don't have a SQLite DB (i.e., when scan_ccs_db found nothing).
        .filter(|c| !is_localhost_url(&c.base_url))
        .collect()
}

fn ccs_dir(home: &Path) -> PathBuf {
    if let Some(raw) = std::env::var_os("CCS_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(raw);
    }
    if let Some(raw) = std::env::var_os("CCS_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(raw).join(".ccs");
    }
    home.join(".ccs")
}

fn ccs_profiles_from_config_yaml(home: &Path, ccs_dir: &Path) -> Vec<CcsProfileCandidate> {
    let path = ccs_dir.join("config.yaml");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    let Ok(v) = serde_yaml::from_str::<Value>(&raw) else {
        return vec![];
    };
    let Some(profiles) = v.get("profiles").and_then(|x| x.as_object()) else {
        return vec![];
    };

    profiles
        .iter()
        .filter_map(|(name, profile)| {
            let settings_ref = profile.get("settings").and_then(|x| x.as_str())?;
            let settings_path = expand_home_path(settings_ref, home);
            read_ccs_settings_profile(name, settings_path)
        })
        .collect()
}

fn ccs_profiles_from_config_json(home: &Path, ccs_dir: &Path) -> Vec<CcsProfileCandidate> {
    let path = ccs_dir.join("config.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return vec![];
    };
    let Some(profiles) = v.get("profiles").and_then(|x| x.as_object()) else {
        return vec![];
    };

    profiles
        .iter()
        .filter_map(|(name, settings)| {
            let settings_ref = settings.as_str()?;
            let settings_path = expand_home_path(settings_ref, home);
            read_ccs_settings_profile(name, settings_path)
        })
        .collect()
}

fn ccs_profile_orphans(ccs_dir: &Path) -> Vec<CcsProfileCandidate> {
    let Ok(entries) = std::fs::read_dir(ccs_dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(name) = file_name.strip_suffix(".settings.json") else {
            continue;
        };
        let name = name.to_string();
        if name == "cursor" || name.starts_with("base-") {
            continue;
        }
        if let Some(p) = read_ccs_settings_profile(&name, path) {
            out.push(p);
        }
    }
    out
}

fn read_ccs_settings_profile(name: &str, settings_path: PathBuf) -> Option<CcsProfileCandidate> {
    let raw = std::fs::read_to_string(&settings_path).ok()?;
    let settings: Value = serde_json::from_str(&raw).ok()?;
    if !settings.get("env").is_some_and(|x| x.is_object()) {
        return None;
    }
    Some(CcsProfileCandidate {
        client: format!("ccs:{name}"),
        name: name.to_string(),
        source_path: settings_path,
        settings,
    })
}

fn ccs_profile_to_candidate(profile: &CcsProfileCandidate) -> Option<LocalCandidate> {
    let env = profile.settings.get("env")?.as_object()?;
    let base_url = first_env_string(env, &["ANTHROPIC_BASE_URL", "OPENAI_BASE_URL", "BASE_URL"])
        .or_else(|| {
            first_env_string(env, &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"])
                .map(|_| "https://api.anthropic.com".to_string())
        })?;
    let auth_ref = first_env_string(
        env,
        &[
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "API_KEY",
        ],
    )
    .and_then(|raw| ccs_env_string_to_auth_ref(&raw));
    let has_anthropic_base = first_env_string(env, &["ANTHROPIC_BASE_URL"]).is_some();
    let model = first_env_string(
        env,
        &[
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "OPENAI_MODEL",
            "MODEL",
        ],
    );
    let kind = detect_ccs_provider_kind(&base_url, model.as_deref(), has_anthropic_base);
    let default_aliases = ccs_model_aliases(env, kind);
    let token_ok = auth_ref
        .as_ref()
        .map(|r| crate::secrets::resolve(r).is_ok())
        .unwrap_or(false);

    Some(LocalCandidate {
        client: profile.client.clone(),
        name: format!("CCS {}", profile.name),
        kind,
        base_url,
        auth_ref,
        token_ok,
        source_path: profile.source_path.display().to_string(),
        default_aliases,
        extra_credentials: vec![],
    })
}

fn first_env_string(env: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(v) = env.get(*key).and_then(|v| v.as_str()).map(str::trim) else {
            continue;
        };
        if v.is_empty() {
            continue;
        }
        return Some(v.to_string());
    }
    None
}

fn ccs_env_string_to_auth_ref(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty()
        || s.eq_ignore_ascii_case("ccs-internal-managed")
        || s.eq_ignore_ascii_case("proxy_managed")
        || s.contains("YOUR_")
        || s.contains("your-")
        || s == "__CCS_REDACTED__"
    {
        return None;
    }
    claude_env_string_to_auth_ref(s)
}

fn detect_ccs_provider_kind(
    base_url: &str,
    model: Option<&str>,
    has_anthropic_base: bool,
) -> ProviderKind {
    if has_anthropic_base {
        return ProviderKind::Anthropic;
    }
    if let Some(kind) = model_defaults::detect_kind_from_base_url(base_url) {
        return kind;
    }
    if let Some(kind) = model.and_then(model_defaults::detect_kind_from_model) {
        return kind;
    }
    ProviderKind::OpenaiChat
}

fn ccs_model_aliases(env: &serde_json::Map<String, Value>, _kind: ProviderKind) -> Vec<ModelAlias> {
    let mut out = Vec::new();
    for (key, alias) in [
        ("ANTHROPIC_MODEL", "default"),
        ("ANTHROPIC_DEFAULT_OPUS_MODEL", "opus"),
        ("ANTHROPIC_DEFAULT_SONNET_MODEL", "sonnet"),
        ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "haiku"),
        ("OPENAI_MODEL", "default"),
        ("GEMINI_MODEL", "default"),
        ("MODEL", "default"),
    ] {
        let Some(model) = env.get(key).and_then(|v| v.as_str()).map(str::trim) else {
            continue;
        };
        if model.is_empty() {
            continue;
        }
        push_alias_once(&mut out, alias, model);
        push_alias_once(&mut out, model, model);
    }

    if let Some(extra) = env.get("ANTHROPIC_EXTRA_MODELS").and_then(|v| v.as_str()) {
        for model in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            push_alias_once(&mut out, model, model);
        }
    }

    if out.is_empty() {
        vec![]
    } else {
        out
    }
}

fn push_alias_once(out: &mut Vec<ModelAlias>, alias: &str, upstream_model: &str) {
    if out
        .iter()
        .any(|x| x.alias == alias && x.upstream_model == upstream_model)
    {
        return;
    }
    out.push(ModelAlias {
        alias: alias.to_string(),
        upstream_model: upstream_model.to_string(),
    });
}

// ---------------------------------------------------------------------------
// CC Switch SQLite database — authoritative provider registry
// (~/.cc-switch/cc-switch.db, providers table)
// ---------------------------------------------------------------------------

fn ccs_db_path() -> Option<PathBuf> {
    let path = home()?.join(".cc-switch").join("cc-switch.db");
    path.exists().then_some(path)
}

fn scan_ccs_db() -> Vec<LocalCandidate> {
    let Some(db_path) = ccs_db_path() else {
        return vec![];
    };
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let mut stmt = match conn.prepare(
        "SELECT id, app_type, name, settings_config FROM providers",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let mut out = Vec::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    });
    if let Ok(rows) = rows {
        for (id, app_type, name, settings_json) in rows.filter_map(|r| r.ok()) {
            let Ok(cfg) = serde_json::from_str::<Value>(&settings_json) else {
                continue;
            };
            if let Some(c) = ccs_db_row_to_candidate(&id, &app_type, &name, &cfg, &db_path) {
                out.push(c);
            }
        }
    }
    out
}

fn ccs_db_row_to_candidate(
    id: &str,
    app_type: &str,
    name: &str,
    cfg: &Value,
    db_path: &Path,
) -> Option<LocalCandidate> {
    match app_type {
        "claude" => ccs_db_claude(id, name, cfg, db_path),
        "codex" => ccs_db_codex(id, name, cfg, db_path),
        "gemini" => ccs_db_gemini(id, name, cfg, db_path),
        "open-code" | "opencode" => ccs_db_opencode(id, name, cfg, db_path),
        _ => None,
    }
}

fn ccs_db_claude(id: &str, name: &str, cfg: &Value, db_path: &Path) -> Option<LocalCandidate> {
    let env = cfg.get("env")?.as_object()?;
    let base_url = first_env_string(env, &["ANTHROPIC_BASE_URL"])?;
    if is_localhost_url(&base_url) {
        return None;
    }
    let auth_ref = first_env_string(env, &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"])
        .and_then(|v| ccs_env_string_to_auth_ref(&v));
    let token_ok = auth_ref
        .as_ref()
        .map(|r| crate::secrets::resolve(r).is_ok())
        .unwrap_or(false);
    let default_aliases = ccs_model_aliases(env, ProviderKind::Anthropic);
    Some(LocalCandidate {
        client: format!("ccs-db:{id}"),
        name: name.to_string(),
        kind: ProviderKind::Anthropic,
        base_url,
        auth_ref,
        token_ok,
        source_path: db_path.display().to_string(),
        default_aliases,
        extra_credentials: vec![],
    })
}

fn ccs_db_codex(id: &str, name: &str, cfg: &Value, db_path: &Path) -> Option<LocalCandidate> {
    let api_key = cfg
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    let auth_ref = ccs_env_string_to_auth_ref(api_key);
    let token_ok = auth_ref
        .as_ref()
        .map(|r| crate::secrets::resolve(r).is_ok())
        .unwrap_or(false);

    let config_str = cfg.get("config").and_then(|v| v.as_str())?;
    let toml_val: toml::Value = toml::from_str(config_str).ok()?;
    let base_url = codex_toml_base_url(&toml_val)?;
    if is_localhost_url(&base_url) {
        return None;
    }

    let model = toml_val
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let default_aliases = if let Some(m) = model {
        vec![
            ModelAlias {
                alias: "default".into(),
                upstream_model: m.clone(),
            },
            ModelAlias {
                alias: m.clone(),
                upstream_model: m,
            },
        ]
    } else {
        vec![]
    };

    Some(LocalCandidate {
        client: format!("ccs-db:{id}"),
        name: name.to_string(),
        kind: ProviderKind::OpenaiResponses,
        base_url,
        auth_ref,
        token_ok,
        source_path: db_path.display().to_string(),
        default_aliases,
        extra_credentials: vec![],
    })
}

fn ccs_db_gemini(id: &str, name: &str, cfg: &Value, db_path: &Path) -> Option<LocalCandidate> {
    let env = cfg.get("env")?.as_object()?;
    let auth_ref = first_env_string(env, &["GEMINI_API_KEY", "GOOGLE_API_KEY"])
        .and_then(|v| ccs_env_string_to_auth_ref(&v));
    // Skip official Gemini entry that has no configured key.
    let token_ok = auth_ref
        .as_ref()
        .map(|r| crate::secrets::resolve(r).is_ok())
        .unwrap_or(false);
    if !token_ok {
        return None;
    }
    let base_url =
        first_env_string(env, &["GOOGLE_GEMINI_BASE_URL", "GEMINI_BASE_URL", "BASE_URL"])
            .unwrap_or_else(|| "https://generativelanguage.googleapis.com".into());
    if is_localhost_url(&base_url) {
        return None;
    }
    let default_aliases = ccs_model_aliases(env, ProviderKind::GeminiNative);
    Some(LocalCandidate {
        client: format!("ccs-db:{id}"),
        name: name.to_string(),
        kind: ProviderKind::GeminiNative,
        base_url,
        auth_ref,
        token_ok,
        source_path: db_path.display().to_string(),
        default_aliases,
        extra_credentials: vec![],
    })
}

fn ccs_db_opencode(id: &str, name: &str, cfg: &Value, db_path: &Path) -> Option<LocalCandidate> {
    // OpenCode entries store base_url and api_key at the top level of settings_config.
    let base_url = first_json_string(cfg, &["base_url", "baseUrl", "baseURL", "OPENAI_BASE_URL"])
        .or_else(|| {
            cfg.get("env")
                .and_then(|e| e.as_object())
                .and_then(|env| first_env_string(env, &["OPENAI_BASE_URL", "BASE_URL"]))
        })?;
    if is_localhost_url(&base_url) {
        return None;
    }
    let raw_key = first_json_string(cfg, &["api_key", "apiKey", "OPENAI_API_KEY"])
        .or_else(|| {
            cfg.get("env")
                .and_then(|e| e.as_object())
                .and_then(|env| first_env_string(env, &["OPENAI_API_KEY", "API_KEY"]))
        });
    let auth_ref = raw_key.and_then(|v| ccs_env_string_to_auth_ref(&v));
    let token_ok = auth_ref
        .as_ref()
        .map(|r| crate::secrets::resolve(r).is_ok())
        .unwrap_or(false);
    let kind = model_defaults::detect_kind_from_base_url(&base_url)
        .unwrap_or(ProviderKind::OpenaiChat);
    Some(LocalCandidate {
        client: format!("ccs-db:{id}"),
        name: name.to_string(),
        kind,
        base_url,
        auth_ref,
        token_ok,
        source_path: db_path.display().to_string(),
        default_aliases: vec![],
        extra_credentials: vec![],
    })
}

fn ccs_candidate_to_plan(c: &LocalCandidate) -> ImportPlan {
    ImportPlan {
        provider: ProviderInput {
            name: c.name.clone(),
            group_name: None,
            kind: c.kind,
            base_url: c.base_url.clone(),
            protocols: vec![],
            host: None,avatar_url: None,
            auth_ref: c.auth_ref.clone(),
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            model_aliases: c.default_aliases.clone(),
        },
        credentials: vec![],
    }
}

pub fn ccs_bundle_to_plan(bundle: &Value) -> anyhow::Result<ImportPlan> {
    let schema = bundle.get("schemaVersion").and_then(|v| v.as_i64());
    if schema != Some(1) {
        anyhow::bail!("unsupported CCS profile bundle schema");
    }
    let profile = bundle
        .get("profile")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("CCS bundle missing profile"))?;
    let name = profile
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Imported");
    let settings = bundle
        .get("settings")
        .ok_or_else(|| anyhow::anyhow!("CCS bundle missing settings"))?;
    let candidate = CcsProfileCandidate {
        client: format!("ccs:{name}"),
        name: name.to_string(),
        source_path: PathBuf::from("(ccs bundle)"),
        settings: settings.clone(),
    };
    let candidate = ccs_profile_to_candidate(&candidate)
        .ok_or_else(|| anyhow::anyhow!("CCS bundle has no importable env settings"))?;
    Ok(ccs_candidate_to_plan(&candidate))
}

#[derive(Debug, Clone, Default)]
struct CcSwitchProviderRequest {
    app: String,
    name: String,
    endpoint: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    haiku_model: Option<String>,
    sonnet_model: Option<String>,
    opus_model: Option<String>,
    config: Option<String>,
    config_format: Option<String>,
}

pub fn cc_switch_deeplink_to_plan(url: &str) -> anyhow::Result<ImportPlan> {
    let mut request = parse_cc_switch_provider_deeplink(url)?;
    merge_cc_switch_inline_config(&mut request)?;
    cc_switch_request_to_plan(&request)
}

fn parse_cc_switch_provider_deeplink(raw: &str) -> anyhow::Result<CcSwitchProviderRequest> {
    let url = Url::parse(raw).map_err(|e| anyhow::anyhow!("invalid CC Switch URL: {e}"))?;
    if url.scheme() != "ccswitch" {
        anyhow::bail!("unsupported URL scheme: expected ccswitch");
    }
    if url.host_str() != Some("v1") || url.path() != "/import" {
        anyhow::bail!("unsupported CC Switch deeplink path");
    }

    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if params.get("resource").map(String::as_str) != Some("provider") {
        anyhow::bail!("only CC Switch provider imports are supported");
    }

    let app = required_query(&params, "app")?;
    if !matches!(
        app.as_str(),
        "claude" | "codex" | "gemini" | "opencode" | "openclaw" | "hermes"
    ) {
        anyhow::bail!("unsupported CC Switch app: {app}");
    }

    Ok(CcSwitchProviderRequest {
        app,
        name: required_query(&params, "name")?,
        endpoint: optional_query(&params, "endpoint"),
        api_key: optional_query(&params, "apiKey"),
        model: optional_query(&params, "model"),
        haiku_model: optional_query(&params, "haikuModel"),
        sonnet_model: optional_query(&params, "sonnetModel"),
        opus_model: optional_query(&params, "opusModel"),
        config: optional_query(&params, "config"),
        config_format: optional_query(&params, "configFormat"),
    })
}

fn required_query(params: &HashMap<String, String>, key: &str) -> anyhow::Result<String> {
    optional_query(params, key).ok_or_else(|| anyhow::anyhow!("missing CC Switch parameter: {key}"))
}

fn optional_query(params: &HashMap<String, String>, key: &str) -> Option<String> {
    params
        .get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn merge_cc_switch_inline_config(request: &mut CcSwitchProviderRequest) -> anyhow::Result<()> {
    let Some(raw_config) = request.config.as_deref() else {
        return Ok(());
    };
    let decoded = decode_base64_url_param(raw_config)
        .map_err(|e| anyhow::anyhow!("invalid CC Switch config parameter: {e}"))?;
    let text = String::from_utf8(decoded)
        .map_err(|e| anyhow::anyhow!("CC Switch config is not UTF-8: {e}"))?;
    let format = request.config_format.as_deref().unwrap_or("json");
    let config: Value = match format {
        "json" => serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("invalid CC Switch JSON config: {e}"))?,
        "toml" => {
            let parsed: toml::Value = toml::from_str(&text)
                .map_err(|e| anyhow::anyhow!("invalid CC Switch TOML config: {e}"))?;
            serde_json::to_value(parsed)?
        }
        other => anyhow::bail!("unsupported CC Switch config format: {other}"),
    };

    match request.app.as_str() {
        "claude" => merge_cc_switch_claude_config(request, &config),
        "codex" => merge_cc_switch_codex_config(request, &config),
        "gemini" => merge_cc_switch_gemini_config(request, &config),
        "opencode" | "openclaw" | "hermes" => merge_cc_switch_additive_config(request, &config),
        _ => {}
    }

    Ok(())
}

fn merge_cc_switch_claude_config(request: &mut CcSwitchProviderRequest, config: &Value) {
    let Some(env) = config.get("env").and_then(|v| v.as_object()) else {
        return;
    };
    fill_from_env(
        request,
        env,
        "api_key",
        &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"],
    );
    fill_from_env(request, env, "endpoint", &["ANTHROPIC_BASE_URL"]);
    fill_from_env(request, env, "model", &["ANTHROPIC_MODEL"]);
    fill_from_env(
        request,
        env,
        "haiku_model",
        &["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
    );
    fill_from_env(
        request,
        env,
        "sonnet_model",
        &["ANTHROPIC_DEFAULT_SONNET_MODEL"],
    );
    fill_from_env(
        request,
        env,
        "opus_model",
        &["ANTHROPIC_DEFAULT_OPUS_MODEL"],
    );
}

fn merge_cc_switch_codex_config(request: &mut CcSwitchProviderRequest, config: &Value) {
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        request.api_key = config
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    let Some(config_text) = config.get("config").and_then(|v| v.as_str()) else {
        return;
    };
    if let Ok(toml_value) = toml::from_str::<toml::Value>(config_text) {
        if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
            request.endpoint = codex_toml_base_url(&toml_value);
        }
        if request.model.as_ref().is_none_or(|s| s.is_empty()) {
            request.model = toml_value
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
    }
}

fn merge_cc_switch_gemini_config(request: &mut CcSwitchProviderRequest, config: &Value) {
    let Some(env) = config.get("env").and_then(|v| v.as_object()) else {
        return;
    };
    fill_from_env(
        request,
        env,
        "api_key",
        &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
    );
    fill_from_env(
        request,
        env,
        "endpoint",
        &["GOOGLE_GEMINI_BASE_URL", "GEMINI_BASE_URL"],
    );
    fill_from_env(request, env, "model", &["GEMINI_MODEL"]);
}

fn merge_cc_switch_additive_config(request: &mut CcSwitchProviderRequest, config: &Value) {
    if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) {
        request.endpoint = first_json_string(config, &["base_url", "baseUrl", "baseURL", "url"]);
    }
    if request.api_key.as_ref().is_none_or(|s| s.is_empty()) {
        request.api_key = first_json_string(config, &["api_key", "apiKey", "key"]);
    }
    if request.model.as_ref().is_none_or(|s| s.is_empty()) {
        request.model = first_json_string(config, &["model", "default_model", "defaultModel"]);
    }
}

fn fill_from_env(
    request: &mut CcSwitchProviderRequest,
    env: &serde_json::Map<String, Value>,
    field: &str,
    keys: &[&str],
) {
    let value = first_env_string(env, keys);
    let Some(value) = value else {
        return;
    };
    match field {
        "api_key" if request.api_key.as_ref().is_none_or(|s| s.is_empty()) => {
            request.api_key = Some(value)
        }
        "endpoint" if request.endpoint.as_ref().is_none_or(|s| s.is_empty()) => {
            request.endpoint = Some(value)
        }
        "model" if request.model.as_ref().is_none_or(|s| s.is_empty()) => {
            request.model = Some(value)
        }
        "haiku_model" if request.haiku_model.as_ref().is_none_or(|s| s.is_empty()) => {
            request.haiku_model = Some(value)
        }
        "sonnet_model" if request.sonnet_model.as_ref().is_none_or(|s| s.is_empty()) => {
            request.sonnet_model = Some(value)
        }
        "opus_model" if request.opus_model.as_ref().is_none_or(|s| s.is_empty()) => {
            request.opus_model = Some(value)
        }
        _ => {}
    }
}

fn first_json_string(config: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = config.get(*key).and_then(|v| v.as_str()).map(str::trim) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn codex_toml_base_url(config: &toml::Value) -> Option<String> {
    let provider_id = config.get("model_provider").and_then(|v| v.as_str())?;
    config
        .get("model_providers")
        .and_then(|v| v.get(provider_id))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn cc_switch_request_to_plan(request: &CcSwitchProviderRequest) -> anyhow::Result<ImportPlan> {
    let base_url = primary_endpoint(request)?;
    let kind = match request.app.as_str() {
        "claude" => ProviderKind::Anthropic,
        "codex" => ProviderKind::OpenaiResponses,
        "gemini" => ProviderKind::GeminiNative,
        _ => detect_ccs_provider_kind(&base_url, request.model.as_deref(), request.app == "claude"),
    };
    let auth_ref = request
        .api_key
        .as_deref()
        .and_then(ccs_env_string_to_auth_ref);
    let name = format!("CC Switch {}", request.name.trim());

    let mut model_aliases = Vec::new();
    if let Some(m) = request.model.as_deref().filter(|s| !s.is_empty()) {
        push_alias_once(&mut model_aliases, "default", m);
        push_alias_once(&mut model_aliases, m, m);
    }
    if let Some(m) = request.haiku_model.as_deref().filter(|s| !s.is_empty()) {
        push_alias_once(&mut model_aliases, "haiku", m);
        push_alias_once(&mut model_aliases, m, m);
    }
    if let Some(m) = request.sonnet_model.as_deref().filter(|s| !s.is_empty()) {
        push_alias_once(&mut model_aliases, "sonnet", m);
        push_alias_once(&mut model_aliases, m, m);
    }
    if let Some(m) = request.opus_model.as_deref().filter(|s| !s.is_empty()) {
        push_alias_once(&mut model_aliases, "opus", m);
        push_alias_once(&mut model_aliases, m, m);
    }

    Ok(ImportPlan {
        provider: ProviderInput {
            name,
            group_name: None,
            kind,
            base_url,
            protocols: vec![],
            host: None,
            avatar_url: None,
            auth_ref,
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            model_aliases,
        },
        credentials: vec![],
    })
}

fn primary_endpoint(request: &CcSwitchProviderRequest) -> anyhow::Result<String> {
    if let Some(endpoint) = request.endpoint.as_deref() {
        if let Some(first) = endpoint.split(',').map(str::trim).find(|s| !s.is_empty()) {
            return Ok(first.trim_end_matches('/').to_string());
        }
    }
    if request.app == "claude" && request.api_key.as_ref().is_some_and(|s| !s.is_empty()) {
        return Ok("https://api.anthropic.com".to_string());
    }
    anyhow::bail!("CC Switch provider import missing endpoint")
}

fn decode_base64_url_param(raw: &str) -> anyhow::Result<Vec<u8>> {
    let trimmed = raw.trim_matches(|c| c == '\r' || c == '\n');
    let mut candidates = Vec::new();
    if trimmed.contains(' ') {
        candidates.push(trimmed.replace(' ', "+"));
    }
    candidates.push(trimmed.to_string());
    let existing = candidates.clone();
    for candidate in existing {
        let mut padded = candidate;
        let remainder = padded.len() % 4;
        if remainder != 0 {
            padded.extend(std::iter::repeat_n('=', 4 - remainder));
        }
        candidates.push(padded);
    }
    for candidate in candidates {
        for engine in [
            &BASE64_STANDARD,
            &BASE64_STANDARD_NO_PAD,
            &BASE64_URL_SAFE,
            &BASE64_URL_SAFE_NO_PAD,
        ] {
            if let Ok(bytes) = engine.decode(&candidate) {
                return Ok(bytes);
            }
        }
    }
    anyhow::bail!("base64 decode failed")
}

// ---------------------------------------------------------------------------
// Confirm import
// ---------------------------------------------------------------------------

pub struct ImportPlan {
    pub provider: ProviderInput,
    /// Inserted after provider (Codex: one row per auth*.json).
    pub credentials: Vec<CredentialInput>,
}

fn codex_candidate_to_plan(c: &LocalCandidate) -> anyhow::Result<ImportPlan> {
    let mut credentials: Vec<CredentialInput> = Vec::new();

    let primary_content = std::fs::read_to_string(&c.source_path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", c.source_path))?;
    let primary_tok = parse_codex_auth_json(&primary_content)?;
    credentials.push(credential_input_from_codex_tokens(
        primary_tok,
        "Codex".into(),
        format!("imported from {}", c.source_path),
        10,
    ));

    for ec in &c.extra_credentials {
        let content = std::fs::read_to_string(&ec.source_path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", ec.source_path))?;
        let tok = parse_codex_auth_json(&content)?;
        let priority = (credentials.len() as i32 + 1) * 10;
        credentials.push(credential_input_from_codex_tokens(
            tok,
            ec.label.clone(),
            format!("imported from {}", ec.source_path),
            priority,
        ));
    }

    Ok(ImportPlan {
        provider: ProviderInput {
            name: c.name.clone(),
            group_name: None,
            kind: c.kind.clone(),
            base_url: c.base_url.clone(),
            protocols: vec![],
            host: None,avatar_url: None,
            auth_ref: None,
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            model_aliases: vec![],
        },
        credentials,
    })
}

fn claude_candidate_to_plan(c: &LocalCandidate) -> ImportPlan {
    ImportPlan {
        provider: ProviderInput {
            name: c.name.clone(),
            group_name: None,
            kind: c.kind.clone(),
            base_url: c.base_url.clone(),
            protocols: vec![],
            host: None,avatar_url: None,
            auth_ref: c.auth_ref.clone(),
            enabled: true,
            priority: 10,
            supports_websocket: None,
            passthrough_mode: true,
            model_aliases: vec![],
        },
        credentials: vec![],
    }
}

pub fn candidate_to_plan(c: &LocalCandidate) -> anyhow::Result<ImportPlan> {
    match c.client.as_str() {
        "codex" => codex_candidate_to_plan(c),
        "claude" => Ok(claude_candidate_to_plan(c)),
        s if s.starts_with("ccs:") || s.starts_with("ccs-db:") => Ok(ccs_candidate_to_plan(c)),
        other => anyhow::bail!("unknown import client: {other}"),
    }
}

pub fn candidate_to_input(c: &LocalCandidate) -> ProviderInput {
    ProviderInput {
        name: c.name.clone(),
        group_name: None,
        kind: c.kind.clone(),
        base_url: c.base_url.clone(),
        protocols: vec![],
        host: None,avatar_url: None,
        auth_ref: c.auth_ref.clone(),
        enabled: true,
        priority: 10,
        supports_websocket: None,
        passthrough_mode: true,
        model_aliases: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn scan_returns_vec() {
        let _ = scan();
    }

    #[test]
    fn claude_env_ref_skips_proxy_managed() {
        assert!(claude_env_string_to_auth_ref("PROXY_MANAGED").is_none());
        assert!(claude_env_string_to_auth_ref("").is_none());
    }

    #[test]
    fn claude_env_ref_dollar_syntax() {
        assert_eq!(
            claude_env_string_to_auth_ref("$MY_VAR").as_deref(),
            Some("env:MY_VAR")
        );
        assert_eq!(
            claude_env_string_to_auth_ref("${OTHER}").as_deref(),
            Some("env:OTHER")
        );
    }

    #[test]
    fn claude_env_ref_literal_secret() {
        let r = claude_env_string_to_auth_ref("sk-ant-api03-short").unwrap();
        assert!(r.starts_with("literal:"));
    }

    #[test]
    fn dotenv_parses_quoted_value() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join(".env");
        let mut f = std::fs::File::create(&p)?;
        writeln!(f, r#"ANTHROPIC_API_KEY="sk-ant-from-dotenv""#)?;
        let ar = parse_dotenv_anthropic(&p).expect("dotenv parse");
        assert_eq!(ar, "literal:sk-ant-from-dotenv");
        Ok(())
    }

    #[test]
    fn ccs_bundle_imports_anthropic_settings_shape() -> anyhow::Result<()> {
        let bundle = serde_json::json!({
            "schemaVersion": 1,
            "profile": { "name": "kimi", "target": "claude" },
            "settings": {
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8317/api/provider/kimi",
                    "ANTHROPIC_AUTH_TOKEN": "ccs-internal-managed",
                    "ANTHROPIC_MODEL": "kimi-k2.5",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-k2-thinking"
                }
            }
        });

        let plan = ccs_bundle_to_plan(&bundle)?;
        assert_eq!(plan.provider.name, "CCS kimi");
        assert_eq!(plan.provider.kind, ProviderKind::Anthropic);
        assert_eq!(plan.provider.auth_ref, None);
        assert!(plan
            .provider
            .model_aliases
            .iter()
            .any(|a| a.alias == "sonnet" && a.upstream_model == "kimi-k2-thinking"));
        Ok(())
    }

    #[test]
    fn ccs_bundle_imports_openai_shape_without_anthropic_base() -> anyhow::Result<()> {
        let bundle = serde_json::json!({
            "schemaVersion": 1,
            "profile": { "name": "deepseek", "target": "codex" },
            "settings": {
                "env": {
                    "OPENAI_BASE_URL": "https://api.deepseek.com",
                    "OPENAI_API_KEY": "sk-deepseek-secret-value-1234567890",
                    "OPENAI_MODEL": "deepseek-chat"
                }
            }
        });

        let plan = ccs_bundle_to_plan(&bundle)?;
        assert_eq!(plan.provider.kind, ProviderKind::OpenaiChat);
        assert_eq!(
            plan.provider.auth_ref.as_deref(),
            Some("literal:sk-deepseek-secret-value-1234567890")
        );
        assert!(plan
            .provider
            .model_aliases
            .iter()
            .any(|a| a.alias == "default" && a.upstream_model == "deepseek-chat"));
        Ok(())
    }

    #[test]
    fn cc_switch_deeplink_imports_provider_url_shape() -> anyhow::Result<()> {
        let url = "ccswitch://v1/import?resource=provider&app=claude&name=Kimi&endpoint=https%3A%2F%2Fapi.moonshot.example%2Fanthropic&apiKey=sk-test-value-1234567890&model=kimi-k2";
        let plan = cc_switch_deeplink_to_plan(url)?;

        assert_eq!(plan.provider.name, "CC Switch Kimi");
        assert_eq!(plan.provider.kind, ProviderKind::Anthropic);
        assert_eq!(
            plan.provider.base_url,
            "https://api.moonshot.example/anthropic"
        );
        assert_eq!(
            plan.provider.auth_ref.as_deref(),
            Some("literal:sk-test-value-1234567890")
        );
        assert!(plan
            .provider
            .model_aliases
            .iter()
            .any(|a| a.alias == "default" && a.upstream_model == "kimi-k2"));
        Ok(())
    }

    #[test]
    fn cc_switch_deeplink_imports_inline_claude_config() -> anyhow::Result<()> {
        let config = serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.deepseek.example/anthropic",
                "ANTHROPIC_AUTH_TOKEN": "ccs-internal-managed",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "deepseek-reasoner"
            }
        });
        let config_b64 = BASE64_URL_SAFE_NO_PAD.encode(config.to_string());
        let url = format!(
            "ccswitch://v1/import?resource=provider&app=claude&name=DeepSeek&config={config_b64}&configFormat=json"
        );
        let plan = cc_switch_deeplink_to_plan(&url)?;

        assert_eq!(plan.provider.kind, ProviderKind::Anthropic);
        assert_eq!(
            plan.provider.base_url,
            "https://api.deepseek.example/anthropic"
        );
        assert_eq!(plan.provider.auth_ref, None);
        assert!(plan
            .provider
            .model_aliases
            .iter()
            .any(|a| a.alias == "sonnet" && a.upstream_model == "deepseek-reasoner"));
        Ok(())
    }
}
