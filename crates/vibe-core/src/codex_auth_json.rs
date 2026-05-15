//! Parse Codex CLI `auth.json` **content** for import into SQLite.
//! Runtime must not read local files; only import paths call this after a one-time read.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use vibe_protocol::CredentialInput;

use crate::auth_fingerprint::chatgpt_oauth_hints_from_access_token;

/// Tokens extracted from a single `auth.json` body (file read happens only in import code).
#[derive(Debug, Clone)]
pub struct CodexAuthImportTokens {
    pub oauth_access_token: String,
    pub oauth_refresh_token: Option<String>,
    pub oauth_expires_at: Option<i64>,
    /// OpenAI API key style (`auth_mode` apikey / legacy); no OAuth refresh.
    pub is_api_key_mode: bool,
    /// From `tokens.id_token` JWT (Codex puts email/plan here; access_token may omit them).
    pub oauth_cached_email: Option<String>,
    pub oauth_cached_subject: Option<String>,
    pub oauth_cached_plan_slug: Option<String>,
}

fn merge_hints_from_id_token_json(v: &Value, out: &mut CodexAuthImportTokens) {
    let Some(id) = v
        .pointer("/tokens/id_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    let h = chatgpt_oauth_hints_from_access_token(id);
    out.oauth_cached_email = h.email;
    out.oauth_cached_subject = h.subject.or(h.chatgpt_user_id);
    out.oauth_cached_plan_slug = h.chatgpt_plan_slug;
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
            let (access, refresh, exp) = oauth_triple_from_json(&v)
                .with_context(|| "chatgpt mode requires non-empty tokens.access_token")?;
            let mut t = CodexAuthImportTokens {
                oauth_access_token: access,
                oauth_refresh_token: refresh,
                oauth_expires_at: exp,
                is_api_key_mode: false,
                oauth_cached_email: None,
                oauth_cached_subject: None,
                oauth_cached_plan_slug: None,
            };
            merge_hints_from_id_token_json(&v, &mut t);
            Ok(t)
        }
        "apikey" | "" => {
            if let Some(key) = v.get("OPENAI_API_KEY").and_then(|k| k.as_str()) {
                if !key.is_empty() && key != "PROXY_MANAGED" {
                    return Ok(CodexAuthImportTokens {
                        oauth_access_token: key.to_string(),
                        oauth_refresh_token: None,
                        oauth_expires_at: None,
                        is_api_key_mode: true,
                        oauth_cached_email: None,
                        oauth_cached_subject: None,
                        oauth_cached_plan_slug: None,
                    });
                }
            }
            if let Some((access, refresh, exp)) = oauth_triple_from_json(&v) {
                let mut t = CodexAuthImportTokens {
                    oauth_access_token: access,
                    oauth_refresh_token: refresh,
                    oauth_expires_at: exp,
                    is_api_key_mode: false,
                    oauth_cached_email: None,
                    oauth_cached_subject: None,
                    oauth_cached_plan_slug: None,
                };
                merge_hints_from_id_token_json(&v, &mut t);
                return Ok(t);
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
        // Plan/tier is represented by upstream JWT and wham/usage snapshots; do not hard-code codex-pro during import to avoid misleading UI plan labels.
        plan_type: None,
        notes: Some(notes),
        enabled: true,
        priority,
        oauth_access_token: Some(tokens.oauth_access_token),
        oauth_refresh_token: tokens.oauth_refresh_token,
        oauth_expires_at: tokens.oauth_expires_at,
        oauth_cached_email: tokens.oauth_cached_email,
        oauth_cached_subject: tokens.oauth_cached_subject,
        oauth_cached_plan_slug: tokens.oauth_cached_plan_slug,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    #[test]
    fn apikey_mode() {
        let t = parse_codex_auth_json(r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-real-key"}"#)
            .unwrap();
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
    fn chatgpt_id_token_fills_cached_identity_when_access_is_opaque() {
        let header = r#"{"alg":"none","typ":"JWT"}"#;
        let payload = r#"{"sub":"sid-99","email":"me@test.dev","https://api.openai.com/auth":{"chatgpt_plan_type":"business","chatgpt_user_id":"cu1"}}"#;
        let h = URL_SAFE_NO_PAD.encode(header.as_bytes());
        let p = URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let id_jwt = format!("{h}.{p}.sig");
        let body = format!(
            r#"{{"auth_mode":"chatgpt","tokens":{{"access_token":"opaque-no-jwt-claims","refresh_token":"rt","id_token":"{id_jwt}"}}}}"#
        );
        let t = parse_codex_auth_json(&body).unwrap();
        assert_eq!(t.oauth_cached_email.as_deref(), Some("me@test.dev"));
        assert_eq!(t.oauth_cached_subject.as_deref(), Some("sid-99"));
        assert_eq!(t.oauth_cached_plan_slug.as_deref(), Some("business"));
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
