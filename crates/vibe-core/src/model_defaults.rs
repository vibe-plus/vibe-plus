//! URL / model-name–based provider kind detection and built-in default alias presets.
//!
//! Detection rules (URL takes priority over model-name heuristics):
//! - `*.anthropic.com`           → Anthropic
//! - `*.openai.com`              → OpenaiCompat
//! - `*.googleapis.com` / AI Studio → GeminiNative
//! - `claude-*`                  → Anthropic
//! - `gpt-*`, `o1-*`, `o3-*`, `o4-*`, `chatgpt-*` → OpenaiCompat
//! - `gemini-*`                  → GeminiNative
//! - `deepseek-*`, `qwen-*`, `yi-*`, `glm-*`, `minimax-*` → OpenaiCompat (all expose OpenAI-compat API)

use vibe_protocol::{ModelAlias, ProviderKind};

// ---------------------------------------------------------------------------
// Kind detection from base URL
// ---------------------------------------------------------------------------

/// Infer `ProviderKind` from `base_url`.  Returns `None` when the URL doesn't
/// match any known pattern — callers should then fall back to model-name
/// detection or leave it unresolved.
pub fn detect_kind_from_base_url(base_url: &str) -> Option<ProviderKind> {
    let lower = base_url.to_lowercase();
    if lower.contains("anthropic.com") {
        Some(ProviderKind::Anthropic)
    } else if lower.contains("openai.com") || lower.contains("api.openai.azure.com") {
        Some(ProviderKind::OpenaiChat)
    } else if lower.contains("googleapis.com")
        || lower.contains("generativelanguage.googleapis.com")
        || lower.contains("aistudio.google.com")
        || lower.contains("aiplatform.googleapis.com")
    {
        Some(ProviderKind::GeminiNative)
    } else if lower.contains("openrouter.ai") || lower.contains("together.ai") {
        Some(ProviderKind::OpenaiChat)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Kind detection from model name
// ---------------------------------------------------------------------------

/// Infer `ProviderKind` from a model identifier string.
pub fn detect_kind_from_model(model: &str) -> Option<ProviderKind> {
    let lower = model.to_lowercase();
    if lower.starts_with("claude-") {
        Some(ProviderKind::Anthropic)
    } else if lower.starts_with("gpt-")
        || lower.starts_with("o1-")
        || lower.starts_with("o3-")
        || lower.starts_with("o4-")
        || lower.starts_with("chatgpt-")
        || lower.starts_with("text-embedding-")
        || lower.starts_with("whisper-")
        || lower.starts_with("dall-e-")
        || lower.starts_with("tts-")
    {
        Some(ProviderKind::OpenaiChat)
    } else if lower.starts_with("gemini-")
        || lower.starts_with("palm-")
        || lower.starts_with("bison")
    {
        Some(ProviderKind::GeminiNative)
    } else if lower.starts_with("deepseek-")
        || lower.starts_with("qwen")
        || lower.starts_with("yi-")
        || lower.starts_with("glm-")
        || lower.starts_with("minimax-")
        || lower.starts_with("moonshot-")
        || lower.starts_with("doubao-")
        || lower.starts_with("ernie-")
        || lower.starts_with("hunyuan-")
        || lower.starts_with("spark-")
    {
        Some(ProviderKind::OpenaiChat)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Default model alias presets
// ---------------------------------------------------------------------------

/// Returns a built-in set of (alias, upstream_model) pairs for the given
/// `ProviderKind`.  These are sensible defaults that can be used when the
/// user adds a new provider without specifying aliases.
///
/// Inspired by cc-switch's `universalProviderPresets.ts` and claude-code-router.
pub fn default_aliases(kind: ProviderKind) -> Vec<ModelAlias> {
    match kind {
        ProviderKind::Anthropic => vec![
            ma("claude-opus-4-7", "claude-opus-4-7-20251101"),
            ma("claude-sonnet-4-6", "claude-sonnet-4-6"),
            ma("claude-sonnet-4-5", "claude-sonnet-4-5-20251001"),
            ma("claude-haiku-4-5", "claude-haiku-4-5-20251001"),
            ma("claude-3-7-sonnet", "claude-3-7-sonnet-20250219"),
            ma("claude-3-5-sonnet", "claude-3-5-sonnet-20241022"),
            ma("claude-3-5-haiku", "claude-3-5-haiku-20241022"),
            ma("claude-3-opus", "claude-3-opus-20240229"),
        ],
        ProviderKind::OpenaiChat => vec![
            ma("gpt-4o", "gpt-4o"),
            ma("gpt-4o-mini", "gpt-4o-mini"),
            ma("gpt-4-turbo", "gpt-4-turbo"),
            ma("o1", "o1"),
            ma("o1-mini", "o1-mini"),
            ma("o3", "o3"),
            ma("o3-mini", "o3-mini"),
            ma("o4-mini", "o4-mini"),
        ],
        // Codex Responses：官方模型列表来自各 provider 的 `/models`（ChatGPT：`…/codex/models`）。
        // 此处仅提供与当前 codex-rs 一致的常见 slug，导入时可再由用户删减。
        ProviderKind::OpenaiResponses => vec![
            ma("gpt-5.3-codex", "gpt-5.3-codex"),
            ma("gpt-5.4", "gpt-5.4"),
            ma("gpt-5.1-codex-max", "gpt-5.1-codex-max"),
            ma("gpt-5.1-codex-mini", "gpt-5.1-codex-mini"),
        ],
        ProviderKind::GeminiNative => vec![
            ma("gemini-2.5-pro", "gemini-2.5-pro-preview-05-06"),
            ma("gemini-2.5-flash", "gemini-2.5-flash-preview-04-17"),
            ma("gemini-2.0-flash", "gemini-2.0-flash"),
            ma("gemini-1.5-pro", "gemini-1.5-pro-latest"),
            ma("gemini-1.5-flash", "gemini-1.5-flash-latest"),
        ],
    }
}

fn ma(alias: &str, upstream: &str) -> ModelAlias {
    ModelAlias {
        alias: alias.to_string(),
        upstream_model: upstream.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_anthropic() {
        assert_eq!(
            detect_kind_from_base_url("https://api.anthropic.com/v1"),
            Some(ProviderKind::Anthropic)
        );
    }

    #[test]
    fn url_openai() {
        assert_eq!(
            detect_kind_from_base_url("https://api.openai.com/v1"),
            Some(ProviderKind::OpenaiChat)
        );
    }

    #[test]
    fn url_gemini() {
        assert_eq!(
            detect_kind_from_base_url("https://generativelanguage.googleapis.com/v1beta"),
            Some(ProviderKind::GeminiNative)
        );
    }

    #[test]
    fn url_unknown_is_none() {
        assert_eq!(detect_kind_from_base_url("http://127.0.0.1:11434"), None);
    }

    #[test]
    fn model_claude() {
        assert_eq!(
            detect_kind_from_model("claude-sonnet-4-5"),
            Some(ProviderKind::Anthropic)
        );
    }

    #[test]
    fn model_gpt() {
        assert_eq!(
            detect_kind_from_model("gpt-4o-mini"),
            Some(ProviderKind::OpenaiChat)
        );
    }

    #[test]
    fn model_gemini() {
        assert_eq!(
            detect_kind_from_model("gemini-2.5-pro"),
            Some(ProviderKind::GeminiNative)
        );
    }

    #[test]
    fn model_deepseek() {
        assert_eq!(
            detect_kind_from_model("deepseek-chat"),
            Some(ProviderKind::OpenaiChat)
        );
    }

    #[test]
    fn model_qwen() {
        assert_eq!(
            detect_kind_from_model("qwen2.5-72b-instruct"),
            Some(ProviderKind::OpenaiChat)
        );
    }

    #[test]
    fn model_unknown() {
        assert_eq!(detect_kind_from_model("llama-3.1-70b"), None);
    }

    #[test]
    fn defaults_anthropic_nonempty() {
        let v = default_aliases(ProviderKind::Anthropic);
        assert!(!v.is_empty());
        assert!(v.iter().any(|a| a.alias == "claude-sonnet-4-5"));
    }

    #[test]
    fn defaults_openai_nonempty() {
        let v = default_aliases(ProviderKind::OpenaiChat);
        assert!(!v.is_empty());
        assert!(v.iter().any(|a| a.alias == "gpt-4o"));
    }

    #[test]
    fn defaults_openai_responses_nonempty() {
        let v = default_aliases(ProviderKind::OpenaiResponses);
        assert!(!v.is_empty());
        assert!(v.iter().any(|a| a.alias == "gpt-5.3-codex"));
    }

    #[test]
    fn defaults_gemini_nonempty() {
        let v = default_aliases(ProviderKind::GeminiNative);
        assert!(!v.is_empty());
        assert!(v.iter().any(|a| a.alias.starts_with("gemini-")));
    }
}
