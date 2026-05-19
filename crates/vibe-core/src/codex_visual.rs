//! Codex-client-visible route and quota status helpers.
//!
//! The official Codex clients render normal Responses message items and consume
//! `codex.rate_limits` snapshots. Keep proxy routing details in message items,
//! and reserve rate-limit events for Coding Plan windows only.

use chrono::Utc;
use serde_json::Value;
use vibe_protocol::{Credential, CredentialPlanSnapshot};

use crate::codex_summary::CodexClientKind;

#[derive(Clone, Debug, Default)]
pub struct CodexVisualContext {
    pub provider_id: String,
    pub provider_name: String,
    pub credential_id: Option<String>,
    pub credential_label: Option<String>,
    pub credential_plan_type: Option<String>,
    pub credential_chatgpt_plan_slug: Option<String>,
    pub requested_model: String,
    pub upstream_model: String,
    pub coding_plan_snapshot: Option<CredentialPlanSnapshot>,
    pub token_plan: Option<TokenPlanSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenPlanSummary {
    pub remaining: i64,
    pub limit: i64,
}

impl TokenPlanSummary {
    fn is_useful(&self) -> bool {
        self.limit > 0 && self.remaining >= 0
    }
}

pub fn token_plan_from_credential(c: &Credential) -> Option<TokenPlanSummary> {
    if !is_token_plan(c.plan_type.as_deref()) {
        return None;
    }
    let summary = TokenPlanSummary {
        remaining: c.rl_tokens_remaining?,
        limit: c.rl_tokens_limit?,
    };
    summary.is_useful().then_some(summary)
}

pub fn is_coding_plan(plan_type: Option<&str>, chatgpt_plan_slug: Option<&str>) -> bool {
    let plan = plan_type
        .or(chatgpt_plan_slug)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        plan.as_str(),
        "codex-plus"
            | "codex-pro"
            | "plus"
            | "pro"
            | "prolite"
            | "team"
            | "business"
            | "enterprise"
            | "edu"
            | "education"
            | "go"
            | "free"
    )
}

pub fn is_token_plan(plan_type: Option<&str>) -> bool {
    let plan = plan_type.unwrap_or("").trim().to_ascii_lowercase();
    plan.contains("token") || plan.contains("resource")
}

pub fn is_payg_plan(plan_type: Option<&str>) -> bool {
    let plan = plan_type.unwrap_or("").trim().to_ascii_lowercase();
    plan == "payg" || plan.contains("pay-as") || plan.contains("pay_as") || plan.contains("paygo")
}

pub fn status_message_text(
    ctx: &CodexVisualContext,
    ttfs_ms: i64,
    client: CodexClientKind,
) -> String {
    match client {
        CodexClientKind::App => status_message_latex(ctx, ttfs_ms),
        CodexClientKind::Cli | CodexClientKind::Unknown => status_message_plain(ctx, ttfs_ms),
    }
}

fn status_message_latex(ctx: &CodexVisualContext, ttfs_ms: i64) -> String {
    let mut parts = vec![
        format!("\\textsf{{TTFS}}={}\\textsf{{ms}}", ttfs_ms.max(0)),
        format!(
            "\\textsf{{upstream}}=\\textsf{{{}}}",
            latex_text(&ctx.provider_name)
        ),
    ];

    if ctx.requested_model != ctx.upstream_model {
        parts.push(format!(
            "\\textsf{{alias}}=\\textsf{{{}}}\\to\\textsf{{{}}}",
            latex_text(&ctx.requested_model),
            latex_text(&ctx.upstream_model)
        ));
    } else if !ctx.upstream_model.is_empty() {
        parts.push(format!(
            "\\textsf{{model}}=\\textsf{{{}}}",
            latex_text(&ctx.upstream_model)
        ));
    }

    if let Some(plan) = ctx.token_plan.as_ref().filter(|p| p.is_useful()) {
        parts.push(format!(
            "\\textsf{{Token}}={}\\textsf{{/}}{}",
            format_tokens(plan.remaining),
            format_tokens(plan.limit)
        ));
    }

    format!(
        "$$\n\\scriptsize\n\\color{{#38bdf8}}{{\\textsf{{Vibe+}}\\,\\mid\\,{}}}\n$$",
        parts.join("\\;\\cdot\\;")
    )
}

fn status_message_plain(ctx: &CodexVisualContext, ttfs_ms: i64) -> String {
    let mut parts = vec![
        format!("TTFS={}ms", ttfs_ms.max(0)),
        format!("upstream={}", ctx.provider_name),
    ];

    if ctx.requested_model != ctx.upstream_model {
        parts.push(format!(
            "alias={}→{}",
            ctx.requested_model, ctx.upstream_model
        ));
    } else if !ctx.upstream_model.is_empty() {
        parts.push(format!("model={}", ctx.upstream_model));
    }

    if let Some(plan) = ctx.token_plan.as_ref().filter(|p| p.is_useful()) {
        parts.push(format!(
            "Token={}/{}",
            format_plain_tokens(plan.remaining),
            format_plain_tokens(plan.limit)
        ));
    }

    format!("↯ Vibe+ · {}", parts.join(" · "))
}

pub fn route_signature(ctx: &CodexVisualContext) -> String {
    format!(
        "{}|{}|{}|{}",
        ctx.provider_id,
        ctx.credential_id.as_deref().unwrap_or(""),
        ctx.requested_model,
        ctx.upstream_model
    )
}

/// High synthetic output_index for the Vibe+ status item, chosen to avoid
/// conflicting with any real response items (which start at 0 and increment).
const STATUS_OUTPUT_INDEX: u32 = 9999;

/// Returns the pair of Responses-API events that render the Vibe+ route status.
///
/// Sends `response.output_item.added` then `response.output_item.done` at a
/// synthetic index that does not collide with the upstream's real output items.
/// Sending `.done` without a preceding `.added` is a protocol violation that
/// causes codex-rs to enter a bad state, close the WebSocket, and report
/// "stream closed before response.completed".
pub fn status_message_events(
    ctx: &CodexVisualContext,
    response_id: &str,
    ttfs_ms: i64,
    client: CodexClientKind,
) -> Vec<String> {
    let item_id = format!(
        "vibe_route_{}",
        response_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    );
    let text = status_message_text(ctx, ttfs_ms, client);
    let item = serde_json::json!({
        "id": item_id,
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}]
    });
    let added = serde_json::json!({
        "type": "response.output_item.added",
        "response_id": response_id,
        "output_index": STATUS_OUTPUT_INDEX,
        "item": {
            "id": item_id,
            "type": "message",
            "role": "assistant",
            "content": []
        }
    })
    .to_string();
    let done = serde_json::json!({
        "type": "response.output_item.done",
        "response_id": response_id,
        "output_index": STATUS_OUTPUT_INDEX,
        "item": item
    })
    .to_string();
    vec![added, done]
}

/// Synthesize a `codex.rate_limits` event from a token-plan snapshot so
/// third-party APIs (Kimi, DeepSeek, Qwen …) render a native quota bar in the
/// Codex app even though they don't emit official `x-codex-*` response headers.
///
/// Only fires for token plans; coding plans and PAYG are handled by
/// `coding_plan_rate_limit_event`.
pub fn token_plan_rate_limit_event(ctx: &CodexVisualContext) -> Option<String> {
    if !is_token_plan(ctx.credential_plan_type.as_deref()) {
        return None;
    }
    let plan = ctx.token_plan.as_ref()?;
    if plan.limit <= 0 || plan.remaining < 0 {
        return None;
    }
    let used_percent =
        ((plan.limit - plan.remaining) as f64 / plan.limit as f64 * 100.0).clamp(0.0, 100.0);
    let limit_name = ctx
        .credential_label
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&ctx.provider_name);
    Some(
        serde_json::json!({
            "type": "codex.rate_limits",
            "metered_limit_name": limit_id(ctx),
            "limit_name": limit_name,
            "plan_type": "token",
            "rate_limits": {
                "primary": {
                    "used_percent": used_percent,
                    "window_minutes": null,
                    "reset_at": null
                }
            },
            "code_review_rate_limits": null,
            "credits": null,
            "promo": null
        })
        .to_string(),
    )
}

/// Build the pair of Responses-API events that announce a provider failover.
///
/// Rendered in orange (warning colour) inside the Codex App using the same
/// LaTeX math block pattern as the route-status message, so it visually
/// distinguishes itself from regular assistant output without being intrusive.
pub fn failover_announcement_events(ctx: &CodexVisualContext, response_id: &str) -> Vec<String> {
    let item_id = format!(
        "vibe_failover_{}",
        response_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    );
    // Show "Provider · Credential" when the credential label is distinct from the
    // provider name (multiple credentials on same provider). Otherwise just provider.
    let cred_label = ctx
        .credential_label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(ctx.provider_name.trim()));
    let slot = if let Some(label) = cred_label {
        format!(
            "\\textsf{{{}}}\\;\\cdot\\;\\textsf{{{}}}",
            latex_text(&ctx.provider_name),
            latex_text(label)
        )
    } else {
        format!("\\textsf{{{}}}", latex_text(&ctx.provider_name))
    };
    let text = format!(
        "$$\n\\scriptsize\n\\color{{#f97316}}{{\\textsf{{Vibe+}}\\,\\mid\\,\\textsf{{Switched to}}\\;{}\\;\\cdot\\;\\textsf{{primary quota exhausted}}}}\n$$",
        slot
    );
    let item = serde_json::json!({
        "id": item_id,
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text}]
    });
    let added = serde_json::json!({
        "type": "response.output_item.added",
        "response_id": response_id,
        "output_index": FAILOVER_OUTPUT_INDEX,
        "item": {
            "id": item_id,
            "type": "message",
            "role": "assistant",
            "content": []
        }
    })
    .to_string();
    let done = serde_json::json!({
        "type": "response.output_item.done",
        "response_id": response_id,
        "output_index": FAILOVER_OUTPUT_INDEX,
        "item": item
    })
    .to_string();
    vec![added, done]
}

/// Output index for the failover announcement — placed just below the route
/// status (9999) so it appears right after without colliding.
const FAILOVER_OUTPUT_INDEX: u32 = 9998;

pub fn coding_plan_rate_limit_event(ctx: &CodexVisualContext) -> Option<String> {
    if !is_coding_plan(
        ctx.credential_plan_type.as_deref(),
        ctx.credential_chatgpt_plan_slug.as_deref(),
    ) || is_payg_plan(ctx.credential_plan_type.as_deref())
        || is_token_plan(ctx.credential_plan_type.as_deref())
    {
        return None;
    }
    let snap = ctx.coding_plan_snapshot.as_ref()?;
    let primary = rate_limit_window(
        snap.codex_5h_used_percent,
        snap.codex_5h_reset_after_seconds,
        Some(300),
    );
    let secondary = rate_limit_window(
        snap.codex_7d_used_percent,
        snap.codex_7d_reset_after_seconds,
        Some(10_080),
    );
    if primary.is_none() && secondary.is_none() {
        return None;
    }

    let plan_type = codex_plan_type(ctx)
        .map(Value::String)
        .unwrap_or(Value::Null);
    let limit_name = ctx
        .credential_label
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&ctx.provider_name);

    Some(
        serde_json::json!({
            "type": "codex.rate_limits",
            "metered_limit_name": limit_id(ctx),
            "limit_name": limit_name,
            "plan_type": plan_type,
            "rate_limits": {
                "primary": primary,
                "secondary": secondary
            },
            "code_review_rate_limits": null,
            "credits": null,
            "promo": null
        })
        .to_string(),
    )
}

fn rate_limit_window(
    used_percent: Option<f64>,
    reset_after_seconds: Option<i64>,
    window_minutes: Option<i64>,
) -> Option<Value> {
    let used_percent = used_percent?.clamp(0.0, 100.0);
    let reset_at = reset_after_seconds.map(|s| Utc::now().timestamp() + s.max(0));
    Some(serde_json::json!({
        "used_percent": used_percent,
        "window_minutes": window_minutes,
        "reset_at": reset_at
    }))
}

fn codex_plan_type(ctx: &CodexVisualContext) -> Option<String> {
    let raw = ctx
        .credential_chatgpt_plan_slug
        .as_deref()
        .or(ctx.credential_plan_type.as_deref())?
        .trim()
        .to_ascii_lowercase();
    let plan = raw.strip_prefix("codex-").unwrap_or(&raw);
    match plan {
        "education" => Some("edu".into()),
        "pro_lite" | "pro-lite" => Some("prolite".into()),
        "" => None,
        other => Some(other.to_string()),
    }
}

fn limit_id(ctx: &CodexVisualContext) -> String {
    ctx.credential_id
        .as_deref()
        .map(|id| format!("codex_{}", id))
        .unwrap_or_else(|| format!("codex_{}", ctx.provider_id))
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn latex_text(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '\\' => "\\backslash ".chars().collect::<Vec<_>>(),
            '{' => "\\{".chars().collect(),
            '}' => "\\}".chars().collect(),
            '_' => "\\_".chars().collect(),
            '^' => "\\^{}".chars().collect(),
            '%' => "\\%".chars().collect(),
            '&' => "\\&".chars().collect(),
            '#' => "\\#".chars().collect(),
            '$' => "\\$".chars().collect(),
            '~' => "\\~{}".chars().collect(),
            '-' => "\\text{-}".chars().collect(),
            other => vec![other],
        })
        .collect()
}

fn format_tokens(n: i64) -> String {
    let n = n.max(0) as f64;
    if n >= 1_000_000.0 {
        format!("{:.1}\\textsf{{M}}", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}\\textsf{{K}}", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}

fn format_plain_tokens(n: i64) -> String {
    let n = n.max(0) as f64;
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_summary::CodexClientKind;

    fn ctx() -> CodexVisualContext {
        CodexVisualContext {
            provider_id: "p1".into(),
            provider_name: "Kimi Coding".into(),
            credential_id: Some("cred-1".into()),
            credential_label: Some("Kimi Pro".into()),
            credential_plan_type: Some("codex-pro".into()),
            requested_model: "gpt-5.4".into(),
            upstream_model: "kimi-k2".into(),
            ..Default::default()
        }
    }

    #[test]
    fn status_includes_alias_when_models_differ() {
        let text = status_message_text(&ctx(), 842, CodexClientKind::App);
        assert!(text.contains("\\scriptsize"));
        assert!(text.contains("#38bdf8"));
        assert!(text.contains("\\textsf{TTFS}=842\\textsf{ms}"));
        assert!(text.contains("\\textsf{alias}="));
        assert!(text.contains("gpt\\text{-}5.4"));
        assert!(text.contains("kimi\\text{-}k2"));
    }

    #[test]
    fn status_omits_alias_when_models_match() {
        let mut ctx = ctx();
        ctx.upstream_model = ctx.requested_model.clone();
        let text = status_message_text(&ctx, 10, CodexClientKind::App);
        assert!(!text.contains("\\textsf{alias}="));
        assert!(text.contains("\\textsf{model}="));
    }

    #[test]
    fn token_plan_only_appears_in_status() {
        let mut ctx = ctx();
        ctx.credential_plan_type = Some("token".into());
        ctx.token_plan = Some(TokenPlanSummary {
            remaining: 2_100_000,
            limit: 3_000_000,
        });
        assert!(
            status_message_text(&ctx, 1, CodexClientKind::App).contains("\\textsf{Token}=2.1\\textsf{M}")
        );
    }

    #[test]
    fn cli_status_is_plain_text() {
        let text = status_message_text(&ctx(), 842, CodexClientKind::Cli);
        assert!(text.starts_with("↯ Vibe+ · "));
        assert!(text.contains("TTFS=842ms"));
        assert!(!text.contains("$$"));
        assert!(!text.contains("\\textsf"));
    }

    #[test]
    fn payg_does_not_emit_rate_limits() {
        let mut ctx = ctx();
        ctx.credential_plan_type = Some("payg".into());
        ctx.coding_plan_snapshot = Some(CredentialPlanSnapshot {
            id: "s1".into(),
            credential_id: "cred-1".into(),
            captured_at: 1,
            codex_5h_used_percent: Some(10.0),
            codex_7d_used_percent: Some(20.0),
            codex_5h_reset_after_seconds: Some(60),
            codex_7d_reset_after_seconds: Some(120),
            codex_primary_used_percent: None,
            codex_secondary_used_percent: None,
            summary: None,
            source: "test".into(),
        });
        assert!(coding_plan_rate_limit_event(&ctx).is_none());
    }

    #[test]
    fn coding_plan_maps_5h_7d_to_primary_secondary() {
        let mut ctx = ctx();
        ctx.coding_plan_snapshot = Some(CredentialPlanSnapshot {
            id: "s1".into(),
            credential_id: "cred-1".into(),
            captured_at: 1,
            codex_5h_used_percent: Some(12.0),
            codex_7d_used_percent: Some(34.0),
            codex_5h_reset_after_seconds: Some(60),
            codex_7d_reset_after_seconds: Some(120),
            codex_primary_used_percent: None,
            codex_secondary_used_percent: None,
            summary: None,
            source: "test".into(),
        });
        let event = coding_plan_rate_limit_event(&ctx).expect("event");
        let v: Value = serde_json::from_str(&event).unwrap();
        assert_eq!(v["type"], "codex.rate_limits");
        assert_eq!(v["rate_limits"]["primary"]["used_percent"], 12.0);
        assert_eq!(v["rate_limits"]["primary"]["window_minutes"], 300);
        assert_eq!(v["rate_limits"]["secondary"]["used_percent"], 34.0);
        assert_eq!(v["rate_limits"]["secondary"]["window_minutes"], 10_080);
        assert_eq!(v["plan_type"], "pro");
    }
}
