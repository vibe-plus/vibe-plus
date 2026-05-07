//! Token & cost extraction.

#[derive(Debug, Default, Clone, Copy)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

impl Usage {
    pub fn estimated_cost_usd(&self, _model: &str) -> String {
        // v1: pricing table not loaded yet; defer to model_pricing.
        "0".to_string()
    }
}
