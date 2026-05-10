//! Scan locally installed AI coding tools for **one-time import** into SQLite.
//!
//! Codex `auth.json` is read only during import POST — tokens are stored in
//! `credentials.oauth_*`; runtime no longer uses `codex-auth` file schemes.

use crate::codex_auth_json::{credential_input_from_codex_tokens, parse_codex_auth_json};
use crate::model_defaults;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

fn scan_claude() -> Option<LocalCandidate> {
    let settings_path = home()?.join(".claude").join("settings.json");
    if !settings_path.exists() {
        return None;
    }

    let auth_ref = claude_auth_ref_from_settings(&settings_path);
    let token_ok = auth_ref
        .as_ref()
        .map(|r| crate::secrets::resolve(r).is_ok())
        .unwrap_or(false)
        || std::env::var("ANTHROPIC_API_KEY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);

    Some(LocalCandidate {
        client: "claude".into(),
        name: "Claude (Local)".into(),
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com".into(),
        auth_ref,
        token_ok,
        source_path: settings_path.display().to_string(),
        default_aliases: model_defaults::default_aliases(ProviderKind::Anthropic),
        extra_credentials: vec![],
    })
}

/// If `~/.claude/settings.json` embeds `env.ANTHROPIC_AUTH_TOKEN`, use `literal:` for import.
fn claude_auth_ref_from_settings(settings_path: &PathBuf) -> Option<String> {
    let s = std::fs::read_to_string(settings_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let tok = v.pointer("/env/ANTHROPIC_AUTH_TOKEN")?.as_str()?;
    if tok.is_empty() || tok == "PROXY_MANAGED" {
        return None;
    }
    Some(format!("literal:{tok}"))
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
            "Codex (ChatGPT Pro)".into(),
        ),
        _ => (
            "https://api.openai.com".into(),
            "Codex / OpenAI".into(),
        ),
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

    let primary_content = std::fs::read_to_string(&c.source_path).map_err(|e| {
        anyhow::anyhow!("failed to read {}: {e}", c.source_path)
    })?;
    let primary_tok = parse_codex_auth_json(&primary_content)?;
    credentials.push(credential_input_from_codex_tokens(
        primary_tok,
        "Codex (imported)".into(),
        format!("imported from {}", c.source_path),
        10,
    ));

    for ec in &c.extra_credentials {
        let content = std::fs::read_to_string(&ec.source_path).map_err(|e| {
            anyhow::anyhow!("failed to read {}: {e}", ec.source_path)
        })?;
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

    #[test]
    fn scan_returns_vec() {
        let _ = scan();
    }
}
