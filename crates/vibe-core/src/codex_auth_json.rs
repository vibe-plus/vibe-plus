//! Parse Codex CLI `auth.json` **content** for import into SQLite.
//! Runtime must not read local files; only import paths call this after a one-time read.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use vibe_protocol::CredentialInput;

/// Tokens extracted from a single `auth.json` body (file read happens only in import code).
#[derive(Debug, Clone)]
pub struct CodexAuthImportTokens {
    pub oauth_access_token: String,
    pub oauth_refresh_token: Option<String>,
    pub oauth_expires_at: Option<i64>,
    /// OpenAI API key style (`auth_mode` apikey / legacy); no OAuth refresh.
    pub is_api_key_mode: bool,
}

fn oauth_triple_from_json(v: &Value) -> Option<(String, Option<String>, Option<i64>)> {
    let access = v
        .pointer("/tokens/access_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let refresh = v
        .pointer("/tokens/refresh_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let exp = v
        .pointer("/tokens/expires_at")
        .and_then(|x| x.as_i64())
        .or_else(|| v.pointer("/tokens/expiry").and_then(|x| x.as_i64()));
    Some((access, refresh, exp))
}

/// Parse Codex `auth.json` text into DB-storable OAuth / API key material.
pub fn parse_codex_auth_json(content: &str) -> Result<CodexAuthImportTokens> {
    let v: Value = serde_json::from_str(content).context("invalid JSON")?;
    let mode = v.get("auth_mode").and_then(|m| m.as_str()).unwrap_or("");

    match mode {
        "chatgpt" => {
            let (access, refresh, exp) = oauth_triple_from_json(&v).with_context(|| {
                "chatgpt mode requires non-empty tokens.access_token"
            })?;
            Ok(CodexAuthImportTokens {
                oauth_access_token: access,
                oauth_refresh_token: refresh,
                oauth_expires_at: exp,
                is_api_key_mode: false,
            })
        }
        "apikey" | "" => {
            if let Some(key) = v.get("OPENAI_API_KEY").and_then(|k| k.as_str()) {
                if !key.is_empty() && key != "PROXY_MANAGED" {
                    return Ok(CodexAuthImportTokens {
                        oauth_access_token: key.to_string(),
                        oauth_refresh_token: None,
                        oauth_expires_at: None,
                        is_api_key_mode: true,
                    });
                }
            }
            if let Some((access, refresh, exp)) = oauth_triple_from_json(&v) {
                return Ok(CodexAuthImportTokens {
                    oauth_access_token: access,
                    oauth_refresh_token: refresh,
                    oauth_expires_at: exp,
                    is_api_key_mode: false,
                });
            }
            bail!(
                "no usable token: need OPENAI_API_KEY or tokens.access_token (path imported once into DB)"
            )
        }
        other => bail!("unknown auth_mode '{other}'"),
    }
}

pub fn credential_input_from_codex_tokens(
    tokens: CodexAuthImportTokens,
    label: String,
    notes: String,
    priority: i32,
) -> CredentialInput {
    CredentialInput {
        label,
        auth_ref: None,
        plan_type: Some("codex-pro".into()),
        notes: Some(notes),
        enabled: true,
        priority,
        oauth_access_token: Some(tokens.oauth_access_token),
        oauth_refresh_token: tokens.oauth_refresh_token,
        oauth_expires_at: tokens.oauth_expires_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apikey_mode() {
        let t = parse_codex_auth_json(r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-real-key"}"#).unwrap();
        assert!(t.is_api_key_mode);
        assert_eq!(t.oauth_access_token, "sk-real-key");
    }

    #[test]
    fn chatgpt_oauth_mode() {
        let t = parse_codex_auth_json(
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"at","refresh_token":"rt"}}"#,
        )
        .unwrap();
        assert!(!t.is_api_key_mode);
        assert_eq!(t.oauth_access_token, "at");
        assert_eq!(t.oauth_refresh_token.as_deref(), Some("rt"));
    }

    #[test]
    fn legacy_apikey_only() {
        let t = parse_codex_auth_json(r#"{"OPENAI_API_KEY":"sk-legacy"}"#).unwrap();
        assert!(t.is_api_key_mode);
        assert_eq!(t.oauth_access_token, "sk-legacy");
    }

    #[test]
    fn proxy_managed_falls_back_oauth() {
        let t = parse_codex_auth_json(
            r#"{"OPENAI_API_KEY":"PROXY_MANAGED","tokens":{"access_token":"oauth"}}"#,
        )
        .unwrap();
        assert!(!t.is_api_key_mode);
        assert_eq!(t.oauth_access_token, "oauth");
    }
}
