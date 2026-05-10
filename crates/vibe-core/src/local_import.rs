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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use vibe_protocol::{CredentialInput, ModelAlias, ProviderInput, ProviderKind};

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
        default_aliases: model_defaults::default_aliases(ProviderKind::Anthropic),
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
        default_aliases: model_defaults::default_aliases(ProviderKind::OpenaiResponses),
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
            kind: c.kind.clone(),
            base_url: c.base_url.clone(),
            auth_ref: None,
            enabled: true,
            priority: 10,
            model_aliases: c.default_aliases.clone(),
        },
        credentials,
    })
}

fn claude_candidate_to_plan(c: &LocalCandidate) -> ImportPlan {
    ImportPlan {
        provider: ProviderInput {
            name: c.name.clone(),
            kind: c.kind.clone(),
            base_url: c.base_url.clone(),
            auth_ref: c.auth_ref.clone(),
            enabled: true,
            priority: 10,
            model_aliases: c.default_aliases.clone(),
        },
        credentials: vec![],
    }
}

pub fn candidate_to_plan(c: &LocalCandidate) -> anyhow::Result<ImportPlan> {
    match c.client.as_str() {
        "codex" => codex_candidate_to_plan(c),
        "claude" => Ok(claude_candidate_to_plan(c)),
        other => anyhow::bail!("unknown import client: {other}"),
    }
}

pub fn candidate_to_input(c: &LocalCandidate) -> ProviderInput {
    ProviderInput {
        name: c.name.clone(),
        kind: c.kind.clone(),
        base_url: c.base_url.clone(),
        auth_ref: c.auth_ref.clone(),
        enabled: true,
        priority: 10,
        model_aliases: c.default_aliases.clone(),
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
}
