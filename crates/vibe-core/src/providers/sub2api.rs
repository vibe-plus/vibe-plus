//! Sub2API client: login, balance, groups, window usage.

use anyhow::{bail, Context, Result};
use vibe_protocol::{ProviderBalanceSnapshot, UpstreamGroupInfo, UsageWindow};

/// POST /api/v1/auth/login  →  JWT access token.
pub async fn login(
    http: &reqwest::Client,
    base_url: &str,
    email: &str,
    password: &str,
) -> Result<(String, Option<i64>)> {
    let url = format!("{}/api/v1/auth/login", base_url.trim_end_matches('/'));
    let resp = http
        .post(&url)
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .context("sub2api login request failed")?;
    if !resp.status().is_success() {
        bail!("sub2api login HTTP {}", resp.status());
    }
    let v: serde_json::Value = resp.json().await.context("sub2api login parse")?;
    let token = v
        .pointer("/access_token")
        .or_else(|| v.pointer("/data/access_token"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .context("sub2api login: no access_token in response")?;
    let expires_in = v
        .get("expires_in")
        .and_then(|x| x.as_i64());
    let expires_at = expires_in.map(|s| chrono::Utc::now().timestamp() + s);
    Ok((token, expires_at))
}

/// GET /api/v1/auth/me  →  user balance + concurrency.
pub async fn fetch_balance(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Option<ProviderBalanceSnapshot> {
    let url = format!("{}/api/v1/auth/me", base_url.trim_end_matches('/'));
    let resp = http.get(&url).bearer_auth(token).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    let balance = v.get("balance").and_then(|x| x.as_f64());
    let recharged = v
        .get("total_recharged")
        .and_then(|x| x.as_f64());
    Some(ProviderBalanceSnapshot {
        currency: "USD".into(),
        balance: balance.map(|b| format!("{b:.4}")),
        remaining: balance.map(|b| format!("{b:.4}")),
        used: match (recharged, balance) {
            (Some(r), Some(b)) => Some(format!("{:.4}", r - b)),
            _ => None,
        },
        total: recharged.map(|r| format!("{r:.4}")),
        period: None,
        note: None,
    })
}

/// GET /api/v1/subscriptions/progress  →  window usage (daily/weekly/monthly).
pub async fn fetch_windows(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Vec<UsageWindow> {
    let url = format!(
        "{}/api/v1/subscriptions/progress",
        base_url.trim_end_matches('/')
    );
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
    let data = v.get("data").unwrap_or(&v);

    let mut windows = Vec::new();
    for (label, used_key, limit_key, reset_key) in &[
        ("daily",   "daily_usage_usd",   "daily_limit_usd",   "daily_window_start"),
        ("weekly",  "weekly_usage_usd",  "weekly_limit_usd",  "weekly_window_start"),
        ("monthly", "monthly_usage_usd", "monthly_limit_usd", "monthly_window_start"),
    ] {
        let used = data.get(used_key).and_then(|x| x.as_f64());
        let limit = data.get(limit_key).and_then(|x| x.as_f64());
        if used.is_none() && limit.is_none() {
            continue;
        }
        let used_usd = used.unwrap_or(0.0);
        let used_pct = match limit {
            Some(lim) if lim > 0.0 => Some(used_usd / lim * 100.0),
            _ => None,
        };
        // reset_key is window start; we add the window duration to get reset time
        let reset_at = data
            .get(reset_key)
            .and_then(|x| x.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| {
                let base = dt.timestamp();
                match *label {
                    "daily" => base + 86_400,
                    "weekly" => base + 7 * 86_400,
                    "monthly" => base + 30 * 86_400,
                    _ => base,
                }
            });
        windows.push(UsageWindow {
            label: label.to_string(),
            used_usd,
            limit_usd: limit,
            used_pct,
            reset_at,
        });
    }
    windows
}

/// GET /api/v1/groups/available  →  groups the user can use.
pub async fn fetch_groups(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Vec<UpstreamGroupInfo> {
    let url = format!(
        "{}/api/v1/groups/available",
        base_url.trim_end_matches('/')
    );
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
    let arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .or_else(|| v.as_array());
    let Some(arr) = arr else { return vec![] };
    arr.iter()
        .filter_map(|g| {
            let id = g.get("id")?.as_u64()?.to_string();
            let name = g.get("name")?.as_str()?.to_string();
            let description = g
                .get("description")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let platform = g
                .get("platform")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let rate_multiplier = g
                .get("rate_multiplier")
                .and_then(|x| x.as_f64())
                .unwrap_or(1.0);
            Some(UpstreamGroupInfo {
                id,
                name,
                description,
                platform,
                rate_multiplier,
            })
        })
        .collect()
}
