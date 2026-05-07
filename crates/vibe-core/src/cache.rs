//! Anthropic prompt-cache injection.
//!
//! Injects `"cache_control": {"type": "ephemeral"}` into:
//!   1. The last block of the `system` array (or wraps a string system prompt).
//!   2. The last tool definition in `tools`.
//!
//! This mirrors cc-switch's approach: cache breakpoints are inserted in the
//! request body before forwarding, maximising cache hit rate without requiring
//! the caller to think about it.
//!
//! Only applied to Anthropic-shaped bodies (those sent to `/v1/messages`).

use bytes::Bytes;
use serde_json::{json, Value};

pub fn inject(body: &[u8]) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let changed = inject_into(&mut v);
    if changed {
        Bytes::from(serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec()))
    } else {
        Bytes::copy_from_slice(body)
    }
}

fn inject_into(v: &mut Value) -> bool {
    let mut changed = false;

    // --- system ----------------------------------------------------------
    if let Some(system) = v.get_mut("system") {
        match system {
            // Already an array of blocks — patch the last element.
            Value::Array(blocks) => {
                if let Some(last) = last_non_thinking_mut(blocks) {
                    if last.get("cache_control").is_none() {
                        last["cache_control"] = ephemeral();
                        changed = true;
                    }
                }
            }
            // Plain string — convert to block array so we can attach cache_control.
            Value::String(s) => {
                let text = s.clone();
                *system = json!([{
                    "type": "text",
                    "text": text,
                    "cache_control": {"type": "ephemeral"}
                }]);
                changed = true;
            }
            _ => {}
        }
    }

    // --- tools -----------------------------------------------------------
    if let Some(Value::Array(tools)) = v.get_mut("tools") {
        if let Some(last_tool) = tools.last_mut() {
            if last_tool.get("cache_control").is_none() {
                last_tool["cache_control"] = ephemeral();
                changed = true;
            }
        }
    }

    changed
}

/// Returns the last element of `blocks` that is not a thinking block.
fn last_non_thinking_mut(blocks: &mut Vec<Value>) -> Option<&mut Value> {
    blocks.iter_mut().rev().find(|b| {
        let t = b.get("type").and_then(|t| t.as_str()).unwrap_or("");
        t != "thinking" && t != "redacted_thinking"
    })
}

fn ephemeral() -> Value {
    json!({"type": "ephemeral"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wraps_string_system() {
        let body = serde_json::to_vec(&json!({
            "model": "claude-opus-4-7",
            "system": "You are helpful.",
            "messages": [{"role":"user","content":"hi"}]
        }))
        .unwrap();
        let out = inject(&body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        let sys = &v["system"];
        assert!(sys.is_array());
        assert_eq!(sys[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn patches_last_system_block() {
        let body = serde_json::to_vec(&json!({
            "system": [
                {"type": "text", "text": "Block 1"},
                {"type": "text", "text": "Block 2"}
            ],
            "messages": []
        }))
        .unwrap();
        let out = inject(&body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        let blocks = v["system"].as_array().unwrap();
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(blocks[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn patches_last_tool() {
        let body = serde_json::to_vec(&json!({
            "system": "s",
            "tools": [
                {"name": "tool_a", "description": "A"},
                {"name": "tool_b", "description": "B"}
            ],
            "messages": []
        }))
        .unwrap();
        let out = inject(&body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        let tools = v["tools"].as_array().unwrap();
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn skips_thinking_blocks() {
        let body = serde_json::to_vec(&json!({
            "system": [
                {"type": "text", "text": "Real system prompt"},
                {"type": "thinking", "thinking": "internal"}
            ],
            "messages": []
        }))
        .unwrap();
        let out = inject(&body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        let blocks = v["system"].as_array().unwrap();
        // cache_control should be on index 0 (last non-thinking), NOT index 1
        assert_eq!(blocks[0]["cache_control"]["type"], "ephemeral");
        assert!(blocks[1].get("cache_control").is_none());
    }
}
