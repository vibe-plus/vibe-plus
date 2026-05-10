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

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut s = input.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    URL_SAFE.decode(s.as_bytes()).ok()
}
