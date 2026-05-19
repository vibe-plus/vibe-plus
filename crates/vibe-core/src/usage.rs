//! Token & cost extraction.

#[derive(Debug, Default, Clone)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_creation_5m_tokens: i64,
    pub cache_creation_1h_tokens: i64,
    pub audio_input_tokens: i64,
    pub audio_output_tokens: i64,
    pub accepted_prediction_tokens: i64,
    pub rejected_prediction_tokens: i64,
    pub cost_items: Option<String>,
}

/// Lightweight realtime token estimate used before providers send final usage.
/// It is intentionally conservative: exact provider usage overwrites it once available.
pub fn estimate_output_tokens(text: &str) -> i64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let chars = trimmed.chars().count() as f64;
    let words = trimmed.split_whitespace().count() as f64;
    let by_chars = (chars / 4.0).ceil();
    let by_words = (words * 1.3).ceil();
    by_chars.max(by_words).max(1.0) as i64
}

struct ModelPricing {
    /// USD per million input tokens
    input_per_m: f64,
    /// USD per million output tokens
    output_per_m: f64,
}

/// Return pricing for a model name using prefix/substring matching.
/// Returns `None` when the model is unknown so callers can omit the cost field.
fn model_pricing(model: &str) -> Option<ModelPricing> {
    let m = model.to_ascii_lowercase();
    // Strip provider prefix (e.g. "openai/gpt-5.4" → "gpt-5.4")
    let m = if let Some(pos) = m.rfind('/') {
        &m[pos + 1..]
    } else {
        &m
    };

    // Match longest/most-specific prefix first.
    let (inp, out) = if m.starts_with("o1-mini") {
        (3.00, 12.00)
    } else if m.starts_with("o1") {
        (15.00, 60.00)
    } else if m.starts_with("o3-mini") {
        (1.10, 4.40)
    } else if m.starts_with("o3") {
        (10.00, 40.00)
    } else if m.starts_with("o4-mini") {
        (1.10, 4.40)
    } else if m.starts_with("o4") {
        (6.00, 24.00)
    } else if m.starts_with("gpt-4o-mini") {
        (0.15, 0.60)
    } else if m.starts_with("gpt-4o") {
        (2.50, 10.00)
    } else if m.starts_with("gpt-4.1-nano") {
        (0.10, 0.40)
    } else if m.starts_with("gpt-4.1-mini") {
        (0.40, 1.60)
    } else if m.starts_with("gpt-4.1") {
        (2.00, 8.00)
    } else if m.starts_with("gpt-5") {
        (3.00, 12.00)
    } else if m.starts_with("claude-opus-4") {
        (15.00, 75.00)
    } else if m.starts_with("claude-sonnet-4") {
        (3.00, 15.00)
    } else if m.starts_with("claude-haiku-4") {
        (0.80, 4.00)
    } else if m.starts_with("deepseek-r1") || m.contains("deepseek-reasoner") {
        (0.55, 2.19)
    } else if m.starts_with("deepseek-v3") || m.starts_with("deepseek-chat") {
        (0.27, 1.10)
    } else if m.starts_with("kimi-k2") {
        (0.15, 0.60)
    } else if m.starts_with("gemini-2.5-pro") {
        (1.25, 10.00)
    } else if m.starts_with("gemini-2.5-flash") {
        (0.30, 2.50)
    } else if m.starts_with("gemini-2.0") {
        (0.10, 0.40)
    } else if m.starts_with("gemini-1.5-pro") {
        (1.25, 5.00)
    } else if m.starts_with("gemini-1.5-flash") {
        (0.075, 0.30)
    } else if m.starts_with("qwen3") {
        (0.40, 1.60)
    } else if m.starts_with("qwen2") {
        (0.20, 0.60)
    } else {
        return None;
    };

    Some(ModelPricing {
        input_per_m: inp,
        output_per_m: out,
    })
}

/// Return output-token USD cost for one token, or None for unknown models.
pub fn output_cost_usd_per_token(model: &str) -> Option<f64> {
    model_pricing(model).map(|p| p.output_per_m / 1_000_000.0)
}

impl Usage {
    /// Return estimated cost in USD, or `None` when the model is unknown.
    pub fn cost_usd(&self, model: &str) -> Option<f64> {
        let p = model_pricing(model)?;
        let cost = (self.input_tokens as f64 * p.input_per_m
            + self.output_tokens as f64 * p.output_per_m)
            / 1_000_000.0;
        Some(cost)
    }

    /// Formatted cost string for storage in `RequestLog.estimated_cost_usd`.
    pub fn estimated_cost_usd(&self, model: &str) -> String {
        match self.cost_usd(model) {
            None => "0".to_string(),
            Some(c) if c < 0.000_01 => format!("{:.7}", c),
            Some(c) if c < 0.001 => format!("{:.5}", c),
            Some(c) => format!("{:.4}", c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt4o_mini_cost() {
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Default::default()
        };
        let cost = u.cost_usd("gpt-4o-mini").unwrap();
        assert!((cost - 0.75).abs() < 0.001, "expected ~0.75 got {cost}");
    }

    #[test]
    fn unknown_model_returns_none() {
        let u = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        assert!(u.cost_usd("some-unknown-model-xyz").is_none());
    }

    #[test]
    fn provider_prefix_stripped() {
        let u = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            ..Default::default()
        };
        let with_prefix = u.cost_usd("openai/gpt-4o-mini").unwrap();
        let without_prefix = u.cost_usd("gpt-4o-mini").unwrap();
        assert!((with_prefix - without_prefix).abs() < 1e-9);
    }
}
