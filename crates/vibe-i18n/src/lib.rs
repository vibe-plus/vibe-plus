//! Fluent-based localization helpers shared by Vibe Plus Rust crates.
//!
//! Keep protocol and diagnostic strings out of this crate. It is for user-facing
//! CLI, desktop-shell, dashboard-facing app log, and opt-in status copy only.

use std::borrow::Cow;
use std::collections::HashMap;

use fluent_templates::fluent_bundle::FluentValue;
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

fn parse_env_locale(value: &str) -> Option<LanguageIdentifier> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "C" || trimmed == "POSIX" {
        return None;
    }
    Some(normalize_locale(
        trimmed
            .split('.')
            .next()
            .unwrap_or(trimmed)
            .replace('_', "-"),
    ))
}

/// macOS GUI language preference (`defaults read -g AppleLanguages`).
#[cfg(target_os = "macos")]
fn detect_macos_apple_language() -> Option<LanguageIdentifier> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.split('"').nth(1)?;
    parse_env_locale(first)
}

pub fn detect_locale_from_env() -> LanguageIdentifier {
    if let Ok(value) = std::env::var("VIBE_LANG") {
        if let Some(locale) = parse_env_locale(&value) {
            return locale;
        }
    }

    let mut env_locale = None;
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if let Some(locale) = parse_env_locale(&value) {
                env_locale = Some(locale);
                break;
            }
        }
    }

    // Cursor / VS Code terminals often force LANG=en_US on a Chinese macOS UI.
    // When the shell locale is generic English, prefer the system UI language.
    #[cfg(target_os = "macos")]
    {
        let use_macos_pref = match &env_locale {
            None => true,
            Some(locale) if *locale == langid!("en-US") => true,
            Some(_) => false,
        };
        if use_macos_pref {
            if let Some(mac) = detect_macos_apple_language() {
                if mac != langid!("en-US") {
                    return mac;
                }
            }
        }
    }

    env_locale.unwrap_or_else(default_locale)
}

pub fn text(locale: &LanguageIdentifier, key: &str) -> String {
    LOCALES.lookup(locale, key)
}

pub fn text_with_args(locale: &LanguageIdentifier, key: &str, args: &[(&str, String)]) -> String {
    let mut map: HashMap<Cow<'static, str>, FluentValue> = HashMap::new();
    for (name, value) in args {
        map.insert(
            Cow::Owned(name.to_string()),
            FluentValue::from(value.clone()),
        );
    }
    LOCALES.lookup_with_args(locale, key, &map)
}

pub fn text_env_args_owned(key: &str, args: Vec<(&str, String)>) -> String {
    let locale = detect_locale_from_env();
    text_with_args(&locale, key, &args)
}

pub fn text_env(key: &str) -> String {
    let locale = detect_locale_from_env();
    text(&locale, key)
}

pub fn text_env_args(key: &str, args: &[(&str, &str)]) -> String {
    let owned: Vec<(&str, String)> = args.iter().map(|(k, v)| (*k, v.to_string())).collect();
    text_env_args_owned(key, owned)
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

    #[test]
    fn auto_update_checker_scheduled_zh() {
        assert_eq!(
            text(&langid!("zh-CN"), "auto-update-checker-scheduled"),
            "自动更新检查器已启动。"
        );
    }

    #[test]
    fn gateway_listening_zh() {
        assert_eq!(
            text_with_args(
                &langid!("zh-CN"),
                "gateway-listening",
                &[("addr", "127.0.0.1:15917".to_string())],
            ),
            "网关已监听 127.0.0.1:15917"
        );
    }
}
