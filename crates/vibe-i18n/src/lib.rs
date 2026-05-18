//! Fluent-based localization helpers shared by Vibe Plus Rust crates.
//!
//! Keep protocol and diagnostic strings out of this crate. It is for user-facing
//! CLI, desktop-shell, dashboard-facing app log, and opt-in status copy only.

use fluent_templates::{static_loader, Loader};
use unic_langid::{langid, LanguageIdentifier};

static_loader! {
    static LOCALES = {
        locales: "./locales",
        fallback_language: "en-US",
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

pub const DEFAULT_LOCALE: &str = "en-US";
pub const ZH_CN_LOCALE: &str = "zh-CN";

pub fn default_locale() -> LanguageIdentifier {
    langid!("en-US")
}

pub fn normalize_locale(input: impl AsRef<str>) -> LanguageIdentifier {
    let raw = input.as_ref().trim();
    if raw.eq_ignore_ascii_case("zh")
        || raw.eq_ignore_ascii_case("zh-cn")
        || raw.eq_ignore_ascii_case("zh_hans")
        || raw.eq_ignore_ascii_case("zh-hans")
        || raw.to_ascii_lowercase().starts_with("zh_cn")
    {
        return langid!("zh-CN");
    }
    raw.parse().unwrap_or_else(|_| default_locale())
}

pub fn detect_locale_from_env() -> LanguageIdentifier {
    for key in ["VIBE_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return normalize_locale(
                    value.split('.').next().unwrap_or(&value).replace('_', "-"),
                );
            }
        }
    }
    default_locale()
}

pub fn text(locale: &LanguageIdentifier, key: &str) -> String {
    LOCALES.lookup(locale, key)
}

pub fn text_env(key: &str) -> String {
    let locale = detect_locale_from_env();
    text(&locale, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_english() {
        assert_eq!(text(&langid!("en-US"), "common-vibe-plus"), "Vibe Plus");
    }

    #[test]
    fn normalizes_zh_cn() {
        assert_eq!(normalize_locale("zh_CN.UTF-8"), langid!("zh-CN"));
    }
}
