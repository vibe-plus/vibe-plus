//! NewAPI (one-api fork) client: login, balance, groups.

use anyhow::{bail, Context, Result};
use vibe_protocol::{ProviderBalanceSnapshot, UpstreamGroupInfo, UsageWindow};

/// POST /api/user/login  →  session token.
pub async fn login(
    http: &reqwest::Client,
    base_url: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let url = format!("{}/api/user/login", base_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .context("newapi login request failed")?;
    if !resp.status().is_success() {
        bail!("newapi login HTTP {}", resp.status());
    }
    // NewAPI stores session via cookie; the JSON body also contains access_token
    let v: serde_json::Value = resp.json().await.context("newapi login parse")?;
    // Some forks return { "data": { "access_token": "..." } }, others { "token": "..." }
    let token = v
        .pointer("/data/access_token")
        .or_else(|| v.pointer("/access_token"))
        .or_else(|| v.pointer("/token"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    token.context("newapi login: no token in response")
}

/// GET /api/user/self  →  balance snapshot.
pub async fn fetch_balance(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Option<ProviderBalanceSnapshot> {
    let url = format!("{}/api/user/self", base_url.trim_end_matches('/'));
    let resp = http.get(&url).bearer_auth(token).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    let data = v.get("data").unwrap_or(&v);

    let quota = data.get("quota").and_then(|x| x.as_f64());
    let used = data.get("used_quota").and_then(|x| x.as_f64());
    let remaining = match (quota, used) {
        (Some(q), Some(u)) => Some(q - u),
        (Some(q), None) => Some(q),
        _ => None,
    };

    // NewAPI quota is in internal units (500 units = $0.001 USD by default).
    // We store as raw unit strings and let the UI apply price_multiplier.
    Some(ProviderBalanceSnapshot {
        currency: "quota".into(),
        balance: remaining.map(|v| format!("{v:.0}")),
        remaining: remaining.map(|v| format!("{v:.0}")),
        used: used.map(|v| format!("{v:.0}")),
        total: quota.map(|v| format!("{v:.0}")),
        period: None,
        note: Some("NewAPI internal quota units".into()),
    })
}

/// GET /api/user/groups  →  group list.
pub async fn fetch_groups(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Vec<UpstreamGroupInfo> {
    let url = format!("{}/api/user/groups", base_url.trim_end_matches('/'));
    let resp = match http.get(&url).bearer_auth(token).send().await {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    if !resp.status().is_success() {
        return vec![];
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    // NewAPI returns { "data": { "group-name": { "ratio": 1.0, "desc": "…" }, … } }
    let data = v.get("data").unwrap_or(&v);
    if let Some(obj) = data.as_object() {
        return obj
            .iter()
            .map(|(k, val)| {
                let ratio = val
                    .get("ratio")
                    .and_then(|r| r.as_f64())
                    .unwrap_or(1.0);
                let desc = val
                    .get("desc")
                    .and_then(|d| d.as_str())
                    .map(str::to_string);
                UpstreamGroupInfo {
                    id: k.clone(),
                    name: k.clone(),
                    description: desc,
                    platform: None,
                    rate_multiplier: ratio,
                }
            })
            .collect();
    }
    vec![]
}

/// GET /api/usage/token with Bearer key  →  window snapshots.
/// This endpoint accepts the API key directly (not the session token).
pub async fn fetch_key_usage(
    http: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Vec<UsageWindow> {
    let url = format!("{}/api/usage/token", base_url.trim_end_matches('/'));
    let resp = match http.get(&url).bearer_auth(api_key).send().await {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    if !resp.status().is_success() {
        return vec![];
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let data = v.get("data").unwrap_or(&v);
    let total = data.get("total_granted").and_then(|x| x.as_f64());
    let used = data.get("total_used").and_then(|x| x.as_f64());
    let _available = data.get("total_available").and_then(|x| x.as_f64());
    let expires_at = data
        .get("expires_at")
        .and_then(|x| x.as_i64())
        .filter(|&x| x > 0);

    if total.is_none() && used.is_none() {
        return vec![];
    }
    let used_pct = match (used, total) {
        (Some(u), Some(t)) if t > 0.0 => Some(u / t * 100.0),
        _ => None,
    };
    vec![UsageWindow {
        label: "total".into(),
        used_usd: used.unwrap_or(0.0),
        limit_usd: total,
        used_pct,
        reset_at: expires_at,
    }]
}
