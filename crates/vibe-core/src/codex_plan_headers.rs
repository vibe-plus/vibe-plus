//! ChatGPT Codex Plan usage from upstream response headers (`x-codex-*`).
//!
//! Contract (verified against sub2api `OpenAICodexUsageSnapshot` + tests using headers such as
//! `x-codex-primary-used-percent`, `x-codex-primary-reset-after-seconds`,
//! `x-codex-primary-window-minutes`). ChatGPT may rename headers without notice — treat as best-effort.
//!
//! **Normalize** maps primary/secondary slots onto canonical 5h vs 7d windows using window-minute
//! comparison (same strategy as sub2api `Normalize()`).

use http::HeaderMap;

#[derive(Debug, Clone, Default)]
pub struct CodexRawSnapshot {
    pub primary_used_percent: Option<f64>,
    pub primary_reset_after_seconds: Option<i64>,
    pub primary_window_minutes: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_reset_after_seconds: Option<i64>,
    pub secondary_window_minutes: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct NormalizedCodexLimits {
    pub used_5h_percent: Option<f64>,
    pub reset_5h_seconds: Option<i64>,
    pub window_5h_minutes: Option<i64>,
    pub used_7d_percent: Option<f64>,
    pub reset_7d_seconds: Option<i64>,
    pub window_7d_minutes: Option<i64>,
}

pub fn parse_codex_usage_headers(headers: &HeaderMap) -> Option<CodexRawSnapshot> {
    let p_pct = header_f64(headers, "x-codex-primary-used-percent");
    let s_pct = header_f64(headers, "x-codex-secondary-used-percent");
    if p_pct.is_none() && s_pct.is_none() {
        return None;
    }
    Some(CodexRawSnapshot {
        primary_used_percent: p_pct,
        primary_reset_after_seconds: header_i64(headers, "x-codex-primary-reset-after-seconds"),
        primary_window_minutes: header_i64(headers, "x-codex-primary-window-minutes"),
        secondary_used_percent: s_pct,
        secondary_reset_after_seconds: header_i64(headers, "x-codex-secondary-reset-after-seconds"),
        secondary_window_minutes: header_i64(headers, "x-codex-secondary-window-minutes"),
    })
}

fn header_f64(headers: &HeaderMap, name: &'static str) -> Option<f64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
}

fn header_i64(headers: &HeaderMap, name: &'static str) -> Option<i64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
}

impl CodexRawSnapshot {
    /// Maps primary/secondary into canonical 5h / 7d fields (sub2api-compatible).
    pub fn normalize(&self) -> NormalizedCodexLimits {
        let mut result = NormalizedCodexLimits::default();
        let primary_mins = self.primary_window_minutes.unwrap_or(0);
        let secondary_mins = self.secondary_window_minutes.unwrap_or(0);
        let has_primary = self.primary_window_minutes.is_some();
        let has_secondary = self.secondary_window_minutes.is_some();

        let mut use_5h_from_primary = false;
        let mut use_7d_from_primary = false;

        if has_primary && has_secondary {
            if primary_mins < secondary_mins {
                use_5h_from_primary = true;
            } else {
                use_7d_from_primary = true;
            }
        } else if has_primary {
            if primary_mins <= 360 {
                use_5h_from_primary = true;
            } else {
                use_7d_from_primary = true;
            }
        } else if has_secondary {
            if secondary_mins <= 360 {
                use_7d_from_primary = true;
            } else {
                use_5h_from_primary = true;
            }
        } else {
            use_7d_from_primary = true;
        }

        if use_5h_from_primary {
            result.used_5h_percent = self.primary_used_percent;
            result.reset_5h_seconds = self.primary_reset_after_seconds;
            result.window_5h_minutes = self.primary_window_minutes;
            result.used_7d_percent = self.secondary_used_percent;
            result.reset_7d_seconds = self.secondary_reset_after_seconds;
            result.window_7d_minutes = self.secondary_window_minutes;
        } else if use_7d_from_primary {
            result.used_7d_percent = self.primary_used_percent;
            result.reset_7d_seconds = self.primary_reset_after_seconds;
            result.window_7d_minutes = self.primary_window_minutes;
            result.used_5h_percent = self.secondary_used_percent;
            result.reset_5h_seconds = self.secondary_reset_after_seconds;
            result.window_5h_minutes = self.secondary_window_minutes;
        }
        result
    }

    pub fn summary_line(&self, norm: &NormalizedCodexLimits) -> String {
        let mut parts = Vec::new();
        if let Some(p) = norm.used_5h_percent {
            parts.push(format!("5h≈{p:.1}%"));
        }
        if let Some(p) = norm.used_7d_percent {
            parts.push(format!("7d≈{p:.1}%"));
        }
        if parts.is_empty() {
            return "Codex headers present".into();
        }
        parts.join(" · ")
    }
}
