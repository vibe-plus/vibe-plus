//! ChatGPT Codex usage from official `GET /backend-api/wham/usage` (same contract as cc-switch).

use reqwest::Client;
use serde::Deserialize;
use vibe_protocol::CredentialPlanSnapshot;

const WHAM_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Deserialize)]
struct CodexUsageBody {
    #[serde(alias = "rateLimit")]
    rate_limit: Option<CodexRateLimit>,
}

#[derive(Debug, Deserialize)]
struct CodexRateLimit {
    #[serde(alias = "primaryWindow")]
    primary_window: Option<CodexRateLimitWindow>,
    #[serde(alias = "secondaryWindow")]
    secondary_window: Option<CodexRateLimitWindow>,
}

#[derive(Debug, Deserialize)]
struct CodexRateLimitWindow {
    #[serde(alias = "usedPercent")]
    used_percent: Option<f64>,
    #[serde(alias = "limitWindowSeconds")]
    limit_window_seconds: Option<i64>,
    #[serde(alias = "resetAt")]
    reset_at: Option<i64>,
}

/// Fetch official Codex rate-limit windows and build a plan snapshot row (`source = wham_usage`).
pub async fn fetch_wham_plan_snapshot(
    http: &Client,
    access_token: &str,
    chatgpt_account_id: Option<&str>,
    credential_id: &str,
) -> anyhow::Result<CredentialPlanSnapshot> {
    let mut req = http
        .get(WHAM_USAGE_URL)
        .timeout(std::time::Duration::from_secs(15))
        .header(
            "Authorization",
            format!("Bearer {access_token}"),
        )
        .header("User-Agent", "codex-cli")
        .header("Accept", "application/json");
    if let Some(id) = chatgpt_account_id {
        req = req.header("ChatGPT-Account-Id", id);
    }
    let resp = req.send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let preview: String = text.chars().take(280).collect();
        anyhow::bail!("wham/usage HTTP {status}: {preview}");
    }
    let body: CodexUsageBody = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("wham/usage JSON: {e}: {}", text.chars().take(200).collect::<String>()))?;
    Ok(plan_snapshot_from_wham_body(credential_id, &body))
}

fn plan_snapshot_from_wham_body(credential_id: &str, body: &CodexUsageBody) -> CredentialPlanSnapshot {
    let now = chrono::Utc::now().timestamp();
    let mut p_pct: Option<f64> = None;
    let mut s_pct: Option<f64> = None;
    let mut w5: Option<f64> = None;
    let mut w7: Option<f64> = None;
    let mut r5: Option<i64> = None;
    let mut r7: Option<i64> = None;

    if let Some(rl) = &body.rate_limit {
        if let Some(w) = &rl.primary_window {
            p_pct = w.used_percent;
            apply_window_mapping(w, now, &mut w5, &mut w7, &mut r5, &mut r7);
        }
        if let Some(w) = &rl.secondary_window {
            s_pct = w.used_percent;
            apply_window_mapping(w, now, &mut w5, &mut w7, &mut r5, &mut r7);
        }
    }

    let summary = build_summary(w5, w7, p_pct, s_pct);

    CredentialPlanSnapshot {
        id: uuid::Uuid::new_v4().to_string(),
        credential_id: credential_id.to_string(),
        captured_at: now,
        codex_5h_used_percent: w5,
        codex_7d_used_percent: w7,
        codex_5h_reset_after_seconds: r5,
        codex_7d_reset_after_seconds: r7,
        codex_primary_used_percent: p_pct,
        codex_secondary_used_percent: s_pct,
        summary: Some(summary),
        source: "wham_usage".into(),
    }
}

fn apply_window_mapping(
    w: &CodexRateLimitWindow,
    now: i64,
    w5: &mut Option<f64>,
    w7: &mut Option<f64>,
    r5: &mut Option<i64>,
    r7: &mut Option<i64>,
) {
    let Some(secs) = w.limit_window_seconds else {
        return;
    };
    let Some(pct) = w.used_percent else {
        return;
    };
    let reset_after = w.reset_at.map(|t| (t - now).max(0));
    match secs {
        18_000 => {
            *w5 = Some(pct);
            *r5 = reset_after;
        }
        604_800 => {
            *w7 = Some(pct);
            *r7 = reset_after;
        }
        _ => {}
    }
}

fn build_summary(
    w5: Option<f64>,
    w7: Option<f64>,
    p_pct: Option<f64>,
    s_pct: Option<f64>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(a) = w5 {
        parts.push(format!("5h≈{a:.1}%"));
    }
    if let Some(a) = w7 {
        parts.push(format!("7d≈{a:.1}%"));
    }
    if !parts.is_empty() {
        return format!("{} · wham/usage", parts.join(" · "));
    }
    match (p_pct, s_pct) {
        (Some(a), Some(b)) => format!("primary {a:.1}% · secondary {b:.1}% · wham/usage"),
        (Some(a), None) => format!("primary {a:.1}% · wham/usage"),
        (None, Some(b)) => format!("secondary {b:.1}% · wham/usage"),
        (None, None) => "wham/usage (no windows)".into(),
    }
}
