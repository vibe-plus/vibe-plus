//! Redact secrets before logging or CLI output.

use serde_json::Value;

const SENSITIVE_KEY_PARTS: &[&str] = &[
    "api_key",
    "apikey",
    "token",
    "password",
    "secret",
    "authorization",
    "id_token",
    "refresh_token",
    "access_token",
    "OPENAI_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
];

pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if is_sensitive_key(k) {
                    out.insert(k.clone(), Value::String("<redacted>".into()));
                } else {
                    out.insert(k.clone(), redact_value(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        other => other.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEY_PARTS
        .iter()
        .any(|part| lower.contains(&part.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_nested_api_keys() {
        let v = json!({
            "auth": { "OPENAI_API_KEY": "sk-secret", "auth_mode": "apikey" }
        });
        let out = redact_value(&v);
        assert_eq!(
            out["auth"]["OPENAI_API_KEY"],
            Value::String("<redacted>".into())
        );
        assert_eq!(out["auth"]["auth_mode"], "apikey");
    }
}
