//! Stable fingerprints for imported credentials (duplicate detection).
//!
//! Strategy mirrors cc-switch / Codex-Manager intent: same logical OAuth account
//! should map to the same fingerprint without storing raw tokens in plaintext twice.
//! Uses `auth_ref` path/key plus JWT `sub` from OAuth access token when decodable.

use base64::engine::general_purpose::URL_SAFE;
use base64::Engine;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Build stable fingerprint `fp:` + 16 hex chars.
pub fn credential_fingerprint(auth_ref: Option<&str>, oauth_access_token: Option<&str>) -> String {
    let mut canonical = String::new();
    canonical.push_str(auth_ref.unwrap_or(""));
    canonical.push('\x1f');
    if let Some(tok) = oauth_access_token.filter(|t| !t.is_empty()) {
        if let Some(sub) = jwt_sub_claim(tok) {
            canonical.push_str(&sub);
        } else {
            canonical.push_str("opaque:");
            canonical.push_str(&tok.len().to_string());
        }
    }
    let mut h = DefaultHasher::new();
    canonical.hash(&mut h);
    format!("fp:{:016x}", h.finish())
}

fn jwt_payload_json(token: &str) -> Option<serde_json::Value> {
    let mid = token.split('.').nth(1)?;
    let bytes = b64url_decode(mid)?;
    serde_json::from_slice(&bytes).ok()
}

fn jwt_sub_claim(token: &str) -> Option<String> {
    jwt_payload_json(token)?
        .get("sub")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

/// Unix expiry timestamp from a JWT `exp` claim.
pub fn jwt_exp_claim(token: &str) -> Option<i64> {
    jwt_payload_json(token)?.get("exp").and_then(|x| x.as_i64())
}

/// `chatgpt_account_id` for `ChatGPT-Account-Id` on `wham/usage` (matches cc-switch token parsing).
pub fn chatgpt_account_id_from_access_token(access_token: &str) -> Option<String> {
    let v = jwt_payload_json(access_token)?;
    v.get("chatgpt_account_id")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| {
            v.pointer("/openai_auth/chatgpt_account_id")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
        .or_else(|| {
            v.get("organizations")
                .and_then(|o| o.as_array())
                .and_then(|arr| arr.first())
                .and_then(|o| o.get("id"))
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
}

/// Subset of ChatGPT OAuth JWT useful for admin UI (aligns with Codex `login::token_data::parse_chatgpt_jwt_claims`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatgptOauthHints {
    pub email: Option<String>,
    pub subject: Option<String>,
    pub chatgpt_user_id: Option<String>,
    pub chatgpt_plan_slug: Option<String>,
}

/// Decode access-token JWT payload and read the same OpenAI claim namespaces Codex uses for `id_token`.
pub fn chatgpt_oauth_hints_from_access_token(token: &str) -> ChatgptOauthHints {
    let Some(v) = jwt_payload_json(token) else {
        return ChatgptOauthHints::default();
    };
    let mut out = ChatgptOauthHints::default();
    out.subject = v.get("sub").and_then(|x| x.as_str()).map(str::to_string);
    out.email = v
        .get("email")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| {
            v.get("https://api.openai.com/profile")
                .and_then(|p| p.get("email"))
                .and_then(|x| x.as_str())
                .map(str::to_string)
        });
    let Some(auth) = v.get("https://api.openai.com/auth") else {
        return out;
    };
    out.chatgpt_user_id = auth
        .get("chatgpt_user_id")
        .or_else(|| auth.get("user_id"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    out.chatgpt_plan_slug = auth
        .get("chatgpt_plan_type")
        .and_then(chatgpt_plan_type_json_as_slug);
    out
}

fn chatgpt_plan_type_json_as_slug(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => map.values().find_map(|x| x.as_str().map(str::to_string)),
        _ => None,
    }
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut s = input.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    URL_SAFE.decode(s.as_bytes()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde::Serialize;
    use serde_json::json;

    #[derive(Serialize)]
    struct JwtHeader {
        alg: &'static str,
        typ: &'static str,
    }

    fn fake_chatgpt_access_jwt(email: &str, plan: &str) -> String {
        let header = JwtHeader {
            alg: "none",
            typ: "JWT",
        };
        let payload = json!({
            "sub": "user-sub-1",
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": plan,
                "chatgpt_user_id": "u-99",
            },
        });
        let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("{h}.{p}.sig")
    }

    #[test]
    fn chatgpt_oauth_hints_reads_email_and_plan() {
        let jwt = fake_chatgpt_access_jwt("a@b.c", "plus");
        let h = chatgpt_oauth_hints_from_access_token(&jwt);
        assert_eq!(h.email.as_deref(), Some("a@b.c"));
        assert_eq!(h.subject.as_deref(), Some("user-sub-1"));
        assert_eq!(h.chatgpt_plan_slug.as_deref(), Some("plus"));
        assert_eq!(h.chatgpt_user_id.as_deref(), Some("u-99"));
    }

    #[test]
    fn chatgpt_oauth_hints_profile_email_fallback() {
        let header = JwtHeader {
            alg: "none",
            typ: "JWT",
        };
        let payload = json!({
            "sub": "s",
            "https://api.openai.com/profile": { "email": "prof@example.com" },
            "https://api.openai.com/auth": { "chatgpt_plan_type": "pro" },
        });
        let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let jwt = format!("{h}.{p}.x");
        let out = chatgpt_oauth_hints_from_access_token(&jwt);
        assert_eq!(out.email.as_deref(), Some("prof@example.com"));
        assert_eq!(out.chatgpt_plan_slug.as_deref(), Some("pro"));
    }

    #[test]
    fn jwt_exp_claim_reads_unix_timestamp() {
        let header = JwtHeader {
            alg: "none",
            typ: "JWT",
        };
        let payload = json!({
            "sub": "s",
            "exp": 1_779_999_999_i64,
        });
        let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let p = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        let jwt = format!("{h}.{p}.x");

        assert_eq!(jwt_exp_claim(&jwt), Some(1_779_999_999));
    }
}
