//! Protocol format transforms between OpenAI Responses API and Chat Completions.
//!
//! Codex CLI uses the OpenAI Responses API (both over WebSocket and HTTP).
//! Most third-party providers only support Chat Completions (`/v1/chat/completions`).
//!
//! This module handles two directions:
//!
//! **Request**: Responses API → Chat Completions
//!   - `input`            → `messages` (with `instructions` prepended as system)
//!   - `max_output_tokens`→ `max_tokens`
//!   - Strip WS envelope  (`type: "response.create"`)
//!   - Remove Responses-only fields (`store`, `service_tier`, `include`, etc.)
//!
//! **Response (streaming)**: Chat Completions SSE event → Responses API WS events
//!   - `choices[0].delta.content` → `response.output_text.delta` events
//!   - `choices[0].delta.tool_calls` (aggregated across chunks) plus `finish_reason: "tool_calls"`
//!     → assistant `output_item.done`, then per-call `function_call` `output_item.done`, then
//!     `response.completed` with `end_turn: false` (Codex continues the tool loop on the same WS).
//!   - `finish_reason: "stop"` → `response.output_text.done` + `response.completed` (no `end_turn`)
//!
//! **Response (non-streaming)**: Chat Completions JSON → Responses API JSON
//!   - `choices[0].message.content` → `output[0].content[0].text`
//!   - `usage.prompt_tokens`        → `usage.input_tokens`

use bytes::Bytes;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Request: strip WS envelope
// ---------------------------------------------------------------------------

/// Strip the WebSocket `{"type":"response.create", ...}` envelope from a Codex WS message,
/// returning the underlying Responses API body.
///
/// Codex CLI has two envelope formats:
///
/// **v0.124 (flat)**:
///   `{"type":"response.create","model":"gpt-5.4","input":[...],...}`
///   → strip `type`, use remaining fields as HTTP body
///
/// **v0.129+ (nested)**:
///   `{"type":"response.create","response":{"model":"gpt-5.5","input":[...],...}}`
///   → unwrap `response` sub-object as HTTP body
pub fn strip_ws_envelope(body: &[u8]) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    if let Some(obj) = v.as_object_mut() {
        let t = obj
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if t == "response.create" {
            // v0.129+ nested format: {"type":"response.create","response":{...}}
            if let Some(inner) = obj.remove("response") {
                if inner.is_object() {
                    return serde_json::to_vec(&inner)
                        .map(Bytes::from)
                        .unwrap_or_else(|_| Bytes::copy_from_slice(body));
                }
                // If "response" is not an object, put it back and fall through to flat handling
                obj.insert("response".into(), inner);
            }
            // v0.124 flat format: remove "type", use rest as body
            obj.remove("type");
        }
    }
    serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

/// Remove locally injected Vibe+ Codex status messages before forwarding a
/// request upstream. Codex clients persist normal assistant messages in their
/// transcript, so our client-visible route banner can otherwise be replayed as
/// model input on later turns.
pub fn strip_vibe_codex_status_messages(body: &[u8]) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    strip_vibe_codex_status_messages_from_value(&mut v);
    serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

pub fn responses_input_ends_with_user_message(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    response_payload_value(&v)
        .map(response_value_input_ends_with_user_message)
        .unwrap_or(false)
}

pub fn rewrite_responses_model(body: &[u8], upstream_model: &str) -> anyhow::Result<Bytes> {
    let mut v: serde_json::Value = serde_json::from_slice(body)?;
    let Some(obj) = response_payload_object_mut(&mut v) else {
        anyhow::bail!("responses body is not an object");
    };
    obj.insert(
        "model".into(),
        serde_json::Value::String(upstream_model.to_string()),
    );
    Ok(Bytes::from(serde_json::to_vec(&v)?))
}

pub fn ensure_responses_instructions_if_missing(body: &[u8], fallback: &str) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(obj) = response_payload_object_mut(&mut v) else {
        return Bytes::copy_from_slice(body);
    };
    let empty = match obj.get("instructions") {
        None => true,
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        Some(_) => false,
    };
    if empty {
        obj.insert(
            "instructions".into(),
            serde_json::Value::String(fallback.to_string()),
        );
    }
    serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

pub fn force_responses_stream_true(body: &[u8]) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(obj) = response_payload_object_mut(&mut v) else {
        return Bytes::copy_from_slice(body);
    };
    obj.insert("stream".into(), serde_json::Value::Bool(true));
    serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

fn response_value_input_ends_with_user_message(v: &serde_json::Value) -> bool {
    let Some(input) = v.get("input").and_then(|input| input.as_array()) else {
        return false;
    };
    input
        .iter()
        .rev()
        .find(|item| !is_vibe_codex_status_message(item))
        .map(response_input_item_is_user_message)
        .unwrap_or(false)
}

fn response_input_item_is_user_message(item: &serde_json::Value) -> bool {
    let item_type = item.get("type").and_then(|t| t.as_str());
    let role = item.get("role").and_then(|role| role.as_str());
    matches!(item_type, None | Some("message")) && role == Some("user")
}

fn strip_vibe_codex_status_messages_from_value(v: &mut serde_json::Value) {
    let Some(payload) = response_payload_value_mut(v) else {
        return;
    };
    if let Some(input) = payload
        .get_mut("input")
        .and_then(|input| input.as_array_mut())
    {
        input.retain(|item| !is_vibe_codex_status_message(item));
    }
}

fn response_payload_value(v: &serde_json::Value) -> Option<&serde_json::Value> {
    if v.get("type").and_then(|t| t.as_str()) == Some("response.create") {
        if let Some(response) = v.get("response").filter(|response| response.is_object()) {
            return Some(response);
        }
    }
    Some(v)
}

fn response_payload_value_mut(v: &mut serde_json::Value) -> Option<&mut serde_json::Value> {
    if v.get("type").and_then(|t| t.as_str()) == Some("response.create")
        && v.get("response")
            .is_some_and(|response| response.is_object())
    {
        return v.get_mut("response");
    }
    Some(v)
}

fn response_payload_object_mut(
    v: &mut serde_json::Value,
) -> Option<&mut Map<String, serde_json::Value>> {
    response_payload_value_mut(v)?.as_object_mut()
}

fn is_vibe_codex_status_message(item: &serde_json::Value) -> bool {
    if item.get("type").and_then(|t| t.as_str()) != Some("message") {
        return false;
    }
    if item.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return false;
    }
    if item
        .get("id")
        .and_then(|id| id.as_str())
        .map(|id| {
            id.starts_with("vibe_route_")
                || id.starts_with("vibe_summary_")
                || id.starts_with("vibe_failover_")
        })
        .unwrap_or(false)
    {
        return true;
    }
    item.get("content")
        .and_then(|content| content.as_array())
        .map(|parts| {
            parts.iter().any(|part| {
                matches!(
                    part.get("type").and_then(|t| t.as_str()),
                    Some("output_text" | "text")
                ) && part
                    .get("text")
                    .and_then(|text| text.as_str())
                    .map(is_vibe_codex_status_text)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn is_vibe_codex_status_text(text: &str) -> bool {
    let trimmed = text.trim();
    let light = trimmed.trim_matches('_').trim();
    is_vibe_codex_formula_status_text(trimmed)
        || is_vibe_codex_summary_plain_text(light)
        || is_vibe_codex_summary_chips_text(light)
        || is_vibe_codex_summary_chinese_text(light)
}

fn is_vibe_codex_formula_status_text(text: &str) -> bool {
    let has_vibe_marker = text.contains("\\textsf{Vibe+}");
    let has_summary_color = text.contains("\\color{#64748b}");
    let has_metric = text.contains("\\textsf{TTFS}")
        || text.contains("\\textsf{speed}")
        || text.contains("\\mathrm{speed}")
        || text.contains("\\textsf{in}")
        || text.contains("\\mathrm{in}")
        || text.contains("\\textsf{out}")
        || text.contains("\\mathrm{out}")
        || text.contains("\\textsf{cache}")
        || text.contains("\\mathrm{cache}")
        || text.contains("\\textsf{lat}")
        || text.contains("\\mathrm{lat}")
        || text.contains("\\textsf{cost}")
        || text.contains("\\mathrm{cost}");
    (has_vibe_marker || has_summary_color) && has_metric
}

fn is_vibe_codex_summary_plain_text(text: &str) -> bool {
    let body = text.strip_prefix('↯').map(str::trim).unwrap_or(text);
    let keys = ["speed", "in", "out", "cache", "lat", "cost"];
    keys.iter().any(|key| {
        body.strip_prefix(key)
            .and_then(|rest| rest.chars().next())
            .map(|ch| ch.is_whitespace())
            .unwrap_or(false)
    }) && keys.iter().any(|key| body.contains(&format!("{key} ")))
        && (body.contains(" · ") || body.contains(" | ") || body.contains("/s"))
}

fn is_vibe_codex_summary_chips_text(text: &str) -> bool {
    let body = text.strip_prefix('↯').map(str::trim).unwrap_or(text);
    ["speed", "in", "out", "cache", "lat"]
        .iter()
        .any(|key| body.contains(&format!("`{key} ")) || body.contains(&format!("{key} `")))
}

fn is_vibe_codex_summary_chinese_text(text: &str) -> bool {
    text.starts_with("This turn: ")
        && ["speed ", "in ", "out ", "cache ", "lat "]
            .iter()
            .any(|label| text.contains(label))
}

// ---------------------------------------------------------------------------
// Request: Responses API → Chat Completions
// ---------------------------------------------------------------------------

/// Convert a Responses API request body to a Chat Completions request body.
///
/// This is needed when routing a `Wire::OpenaiResponses` request to an
/// `OpenaiCompat` provider that only supports `/v1/chat/completions`.
pub fn responses_to_chat(body: &[u8]) -> Bytes {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(obj) = v.as_object() else {
        return Bytes::copy_from_slice(body);
    };

    let mut chat: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    // model — required
    if let Some(m) = obj.get("model") {
        chat.insert("model".into(), m.clone());
    }

    // Build messages array
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // instructions → system message (prepend)
    if let Some(instr) = obj.get("instructions").and_then(|v| v.as_str()) {
        if !instr.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": instr}));
        }
    }

    // input → messages
    if let Some(input) = obj.get("input").and_then(|v| v.as_array()) {
        let mut declared_tool_calls = HashSet::new();
        let mut saw_skills_instructions = false;
        for item in input {
            if let Some(item_obj) = item.as_object() {
                let is_function_call =
                    item_obj.get("type").and_then(|t| t.as_str()) == Some("function_call");
                let is_custom_tool_call =
                    item_obj.get("type").and_then(|t| t.as_str()) == Some("custom_tool_call");
                if is_function_call || is_custom_tool_call {
                    let call_id = item_obj
                        .get("call_id")
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    if !call_id.is_empty() {
                        declared_tool_calls.insert(call_id.to_string());
                    }
                }
            }
        }

        for item in input {
            if let Some(item_obj) = item.as_object() {
                match item_obj.get("type").and_then(|t| t.as_str()) {
                    Some("function_call") => {
                        push_assistant_tool_call_message(item_obj, &mut messages, None);
                        continue;
                    }
                    Some("custom_tool_call") => {
                        push_assistant_tool_call_message(item_obj, &mut messages, Some("input"));
                        continue;
                    }
                    Some("function_call_output") => {
                        let call_id = item_obj
                            .get("call_id")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        let content = wire_function_output_as_tool_content(item_obj.get("output"));
                        if !call_id.is_empty() && declared_tool_calls.contains(&call_id) {
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": content
                            }));
                        }
                        continue;
                    }
                    Some("custom_tool_call_output") => {
                        let call_id = item_obj
                            .get("call_id")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        let content = wire_function_output_as_tool_content(item_obj.get("output"));
                        if !call_id.is_empty() && declared_tool_calls.contains(&call_id) {
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": content
                            }));
                        }
                        continue;
                    }
                    _ => {}
                }

                // Each item is `{"role":"user"|"assistant"|"developer", "content":"..."}`.
                // Responses API can also have structured content arrays; flatten those.
                // "developer" is a newer OpenAI system-like role — map to "system" for compatibility.
                let raw_role = item_obj
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user");
                let role = match raw_role {
                    "developer" => "system",
                    other => other,
                };
                let content = match item_obj.get("content") {
                    Some(serde_json::Value::String(s)) => serde_json::Value::String(s.clone()),
                    Some(serde_json::Value::Array(parts)) => {
                        // Flatten content parts (e.g. {"type":"text","text":"..."}) into a string
                        let text: String = parts
                            .iter()
                            .filter_map(|p| {
                                p.get("text").and_then(|t| t.as_str()).map(str::to_string)
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        serde_json::Value::String(text)
                    }
                    _ => serde_json::Value::String(String::new()),
                };
                if role == "system" {
                    if let Some(text) = content.as_str() {
                        if text.contains("<skills_instructions>") {
                            if saw_skills_instructions {
                                continue;
                            }
                            saw_skills_instructions = true;
                        }
                    }
                }
                messages.push(serde_json::json!({"role": role, "content": content}));
            }
        }
    }

    if !messages.is_empty() {
        chat.insert("messages".into(), serde_json::Value::Array(messages));
    }

    // max_output_tokens → max_tokens
    if let Some(mot) = obj.get("max_output_tokens") {
        chat.insert("max_tokens".into(), mot.clone());
    }

    // Pass-through scalar fields
    for key in &[
        "temperature",
        "top_p",
        "stream",
        "stop",
        "n",
        "presence_penalty",
        "frequency_penalty",
        "logit_bias",
        "user",
        "seed",
        "response_format",
        "reasoning_effort",
    ] {
        if let Some(v) = obj.get(*key) {
            chat.insert(key.to_string(), v.clone());
        }
    }

    // tools: Responses API format → Chat Completions / OpenAI-compat body.
    //
    // DeepSeek/OpenAI-compatible Chat endpoints usually accept only `tools[].type=function`,
    // and non-function tools (web_search/image_generation/namespace...) can return 400.
    // Therefore Responses->Chat conversion keeps only function tools.
    if let Some(tools_val) = obj.get("tools").and_then(|v| v.as_array()) {
        let converted: Vec<serde_json::Value> = tools_val
            .iter()
            .filter_map(|t| {
                let t_obj = t.as_object()?;
                let tool_type = t_obj.get("type").and_then(|v| v.as_str())?;
                match tool_type {
                    "function" => {
                        // Already Chat-compatible if wrapped; wrap if needed.
                        if t_obj.contains_key("function") {
                            // Already in Chat Completions format — pass through.
                            Some(t.clone())
                        } else {
                            // Responses API flat format: hoist fields into "function" sub-object.
                            let mut func_obj = serde_json::Map::new();
                            for (k, v) in t_obj.iter() {
                                if k != "type" {
                                    func_obj.insert(k.clone(), v.clone());
                                }
                            }
                            Some(serde_json::json!({
                                "type": "function",
                                "function": func_obj
                            }))
                        }
                    }
                    "custom" => {
                        let name = t_obj.get("name").and_then(|v| v.as_str())?;
                        if name != "apply_patch" {
                            return None;
                        }
                        Some(serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": "apply_patch",
                                "description": t_obj.get("description").cloned().unwrap_or_else(|| serde_json::Value::String("Apply a patch to files.".into())),
                                "parameters": {
                                    "type": "object",
                                    "properties": {
                                        "patch": { "type": "string" }
                                    },
                                    "required": ["patch"],
                                    "additionalProperties": false
                                }
                            }
                        }))
                    }
                    _ => None,
                }
            })
            .collect();
        if !converted.is_empty() {
            chat.insert("tools".into(), serde_json::Value::Array(converted));
        }
        // Pass tool_choice when upstream supplied tools.
        if let Some(tc) = obj.get("tool_choice") {
            chat.insert("tool_choice".into(), tc.clone());
        }
    }

    serde_json::to_vec(&serde_json::Value::Object(chat))
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

fn wire_function_output_as_tool_content(output: Option<&serde_json::Value>) -> String {
    match output {
        None => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| match p.get("type").and_then(|t| t.as_str()) {
                Some("input_text") | Some("output_text") => p.get("text").and_then(|t| t.as_str()),
                _ => p.get("text").and_then(|t| t.as_str()),
            })
            .collect::<Vec<_>>()
            .concat(),
        Some(other) => other.to_string(),
    }
}

fn custom_tool_input_from_arguments(arguments: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(arguments).ok();
    if let Some(text) = parsed
        .as_ref()
        .and_then(|v| v.get("patch"))
        .and_then(|v| v.as_str())
    {
        return text.to_string();
    }
    arguments.to_string()
}

fn push_assistant_tool_call_message(
    item_obj: &serde_json::Map<String, serde_json::Value>,
    messages: &mut Vec<serde_json::Value>,
    custom_input_field: Option<&str>,
) {
    let call_id = item_obj
        .get("call_id")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let name = item_obj
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("unknown_tool")
        .to_string();
    let arguments = match custom_input_field {
        Some(field) => item_obj
            .get(field)
            .and_then(|a| a.as_str())
            .map(|s| serde_json::json!({ "patch": s }).to_string())
            .unwrap_or_else(|| "{}".to_string()),
        None => item_obj
            .get("arguments")
            .and_then(|a| a.as_str())
            .unwrap_or("{}")
            .to_string(),
    };
    if !call_id.is_empty() {
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments
                }
            }]
        }));
    }
}

#[derive(Default, Clone)]
struct StreamingToolFragment {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// Aggregate Chat Completions streaming text plus `delta.tool_calls` fragments so C2R can emit `function_call` events at `finish_reason`.
#[derive(Default)]
pub struct ChatCompletionsC2rAccumulator {
    pub text: String,
    tool_calls_by_index: BTreeMap<u32, StreamingToolFragment>,
    response_created: bool,
    message_started: bool,
}

fn merge_tool_call_json_objects(
    into: &mut BTreeMap<u32, StreamingToolFragment>,
    parts: &[serde_json::Value],
) {
    for part in parts {
        let index = part
            .get("index")
            .and_then(|i| i.as_u64())
            .map(|u| u as u32)
            .unwrap_or(0);
        let entry = into.entry(index).or_default();
        if let Some(id) = part.get("id").and_then(|x| x.as_str()) {
            if !id.is_empty() {
                entry.id = Some(id.to_string());
            }
        }
        if let Some(f) = part.get("function").and_then(|x| x.as_object()) {
            if let Some(n) = f.get("name").and_then(|x| x.as_str()) {
                if !n.is_empty() {
                    entry.name = Some(n.to_string());
                }
            }
            if let Some(args) = f.get("arguments").and_then(|x| x.as_str()) {
                entry.arguments.push_str(args);
            }
        }
    }
}

impl ChatCompletionsC2rAccumulator {
    fn ensure_response_created(&mut self, out: &mut Vec<String>, session_id: &str) {
        if self.response_created {
            return;
        }
        if let Ok(s) = serde_json::to_string(&serde_json::json!({
            "type": "response.created",
            "response": {
                "id": session_id,
                "object": "response",
                "status": "in_progress",
                "output": []
            }
        })) {
            out.push(s);
            self.response_created = true;
        }
    }

    fn ensure_message_started(&mut self, out: &mut Vec<String>, session_id: &str, item_id: &str) {
        if self.message_started {
            return;
        }
        self.ensure_response_created(out, session_id);
        if let Ok(s) = serde_json::to_string(&serde_json::json!({
            "type": "response.output_item.added",
            "response_id": session_id,
            "output_index": 0,
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "content": []
            }
        })) {
            out.push(s);
        }
        if let Ok(s) = serde_json::to_string(&serde_json::json!({
            "type": "response.content_part.added",
            "response_id": session_id,
            "item_id": item_id,
            "output_index": 0,
            "content_index": 0,
            "part": {"type": "output_text", "text": ""}
        })) {
            out.push(s);
        }
        self.message_started = true;
    }

    fn merge_delta_tool_calls(&mut self, v: &serde_json::Value) {
        if let Some(arr) = v
            .pointer("/choices/0/delta/tool_calls")
            .and_then(|x| x.as_array())
        {
            merge_tool_call_json_objects(&mut self.tool_calls_by_index, arr);
        }
    }

    fn merge_message_tool_calls(&mut self, v: &serde_json::Value) {
        if let Some(arr) = v
            .pointer("/choices/0/message/tool_calls")
            .and_then(|x| x.as_array())
        {
            merge_tool_call_json_objects(&mut self.tool_calls_by_index, arr);
        }
    }

    /// Non-streaming Chat Completions `choices[0].message.tool_calls`.
    fn absorb_chat_message_tool_calls(&mut self, message: &serde_json::Value) {
        if let Some(arr) = message.get("tool_calls").and_then(|x| x.as_array()) {
            merge_tool_call_json_objects(&mut self.tool_calls_by_index, arr);
        }
    }

    fn push_function_call_done_events(
        &self,
        out: &mut Vec<String>,
        session_id: &str,
        start_output_index: u32,
    ) {
        let mut output_index: u32 = start_output_index;
        for frag in self.tool_calls_by_index.values() {
            let call_id = frag
                .id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("call_{session_id}-{output_index}"));
            let name = frag
                .name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown_tool".into());
            let arguments = if frag.arguments.is_empty() {
                "{}".into()
            } else {
                frag.arguments.clone()
            };
            let item = if name == "apply_patch" {
                serde_json::json!({
                    "type": "custom_tool_call",
                    "call_id": call_id,
                    "name": name,
                    "input": custom_tool_input_from_arguments(&arguments),
                })
            } else {
                serde_json::json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments,
                })
            };
            if let Ok(s) = serde_json::to_string(&serde_json::json!({
                "type": "response.output_item.done",
                "response_id": session_id,
                "output_index": output_index,
                "item": item,
            })) {
                out.push(s);
                output_index = output_index.saturating_add(1);
            }
        }
    }

    fn clear_tool_calls(&mut self) {
        self.tool_calls_by_index.clear();
    }
}

// ---------------------------------------------------------------------------
// Response (streaming): Chat Completions SSE event → Responses API WS events
// ---------------------------------------------------------------------------

/// Convert one Chat Completions SSE data payload to Responses API event JSON strings.
///
/// `accumulator` retains streamed assistant text plus `delta.tool_calls`/`message.tool_calls` fragments
/// so `finish_reason: "tool_calls"` can emit `function_call` output items for Codex.
///
/// Called for each `data: {...}` line in the upstream SSE stream when routing
/// to an `OpenaiCompat` provider. The returned strings are sent as WS text messages.
///
/// Returns an empty Vec if the event should be silently dropped.
pub fn chat_event_to_responses_events(
    event_json: &str,
    session_id: &str,
    item_id: &str,
    accumulator: &mut ChatCompletionsC2rAccumulator,
) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(event_json) else {
        return vec![];
    };

    accumulator.merge_delta_tool_calls(&v);
    accumulator.merge_message_tool_calls(&v);

    let mut out = Vec::new();

    // Role delta (first chunk from assistant) → only mark response as created.
    // We intentionally delay the empty assistant message shell until we see
    // actual text, otherwise pure tool-call turns render as blank blocks.
    if let Some(role) = v.pointer("/choices/0/delta/role").and_then(|v| v.as_str()) {
        if role == "assistant" {
            accumulator.ensure_response_created(&mut out, session_id);
        }
    }

    // Delta content → response.output_text.delta
    if let Some(content) = v
        .pointer("/choices/0/delta/content")
        .and_then(|v| v.as_str())
    {
        if !content.is_empty() {
            accumulator.ensure_message_started(&mut out, session_id, item_id);
            accumulator.text.push_str(content);
            if let Ok(s) = serde_json::to_string(&serde_json::json!({
                "type": "response.output_text.delta",
                "response_id": session_id,
                "item_id": item_id,
                "output_index": 0,
                "content_index": 0,
                "delta": content
            })) {
                out.push(s);
            }
        }
    }

    // Finish reason → done sequence + response.completed (Codex WS/SSE only treats
    // `response.completed` as terminal; `response.done` is ignored — see codex-rs process_responses_event).
    if let Some(reason) = v
        .pointer("/choices/0/finish_reason")
        .and_then(|v| v.as_str())
    {
        if !reason.is_empty() && reason != "null" {
            // "stop" = normal completion; "tool_calls" = model requested tools (Codex expects
            // `function_call` output items + response.completed.end_turn=false); other reasons = incomplete turn.
            let status = match reason {
                "stop" | "tool_calls" => "completed",
                _ => "incomplete",
            };

            accumulator.ensure_response_created(&mut out, session_id);
            let full_text = accumulator.text.clone();
            let has_assistant_text = !full_text.trim().is_empty();
            let emit_assistant_message = reason != "tool_calls" || has_assistant_text;

            if emit_assistant_message {
                accumulator.ensure_message_started(&mut out, session_id, item_id);
                // response.output_text.done — finalize the streamed text
                if let Ok(s) = serde_json::to_string(&serde_json::json!({
                    "type": "response.output_text.done",
                    "response_id": session_id,
                    "item_id": item_id,
                    "output_index": 0,
                    "content_index": 0,
                    "text": full_text
                })) {
                    out.push(s);
                }
                // response.content_part.done
                if let Ok(s) = serde_json::to_string(&serde_json::json!({
                    "type": "response.content_part.done",
                    "response_id": session_id,
                    "item_id": item_id,
                    "output_index": 0,
                    "content_index": 0,
                    "part": {"type": "output_text", "text": full_text}
                })) {
                    out.push(s);
                }
                let message_item = if reason == "tool_calls" {
                    serde_json::json!({
                        "id": item_id,
                        "type": "message",
                        "role": "assistant",
                        "phase": "commentary",
                        "status": status,
                        "content": [{"type": "output_text", "text": full_text}],
                    })
                } else {
                    serde_json::json!({
                        "id": item_id,
                        "type": "message",
                        "role": "assistant",
                        "status": status,
                        "content": [{"type": "output_text", "text": full_text}],
                    })
                };

                // response.output_item.done
                if let Ok(s) = serde_json::to_string(&serde_json::json!({
                    "type": "response.output_item.done",
                    "response_id": session_id,
                    "output_index": 0,
                    "item": message_item,
                })) {
                    out.push(s);
                }
            }

            if reason == "tool_calls" && !accumulator.tool_calls_by_index.is_empty() {
                let start_idx = if emit_assistant_message { 1 } else { 0 };
                accumulator.push_function_call_done_events(&mut out, session_id, start_idx);
            }

            let usage_val = v.get("usage");

            let input_tokens = usage_val
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let output_tokens = usage_val
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            let total_tokens = input_tokens + output_tokens;

            let mut completed_resp = serde_json::json!({
                "id": session_id,
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "total_tokens": total_tokens
                }
            });

            match reason {
                "tool_calls" => {
                    if let Some(obj) = completed_resp.as_object_mut() {
                        obj.insert("end_turn".into(), serde_json::Value::Bool(false));
                    }
                }
                _ => {}
            };

            if let Ok(s) = serde_json::to_string(&serde_json::json!({
                "type": "response.completed",
                "response": completed_resp,
            })) {
                out.push(s);
            }

            accumulator.clear_tool_calls();
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Response (non-streaming): Chat Completions → Responses API
// ---------------------------------------------------------------------------

fn assistant_message_content_from_completion(v: &serde_json::Value) -> String {
    match v.pointer("/choices/0/message/content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.as_str().unwrap_or("").to_string(),
    }
}

/// Convert non-streaming Chat Completions JSON into Codex WebSocket text-frame sequences.
///
/// Codex clients require each WS message to be an event with `"type"`; the previous raw `response` object had no `type`,
/// so codex-rs dropped it and waited forever for `response.completed`, eventually reporting `stream closed before response.completed`.
pub fn chat_completion_non_stream_to_ws_events(
    body: &[u8],
    session_id: &str,
    item_id: &str,
) -> Result<Vec<String>, ()> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| ())?;
    if v.pointer("/choices/0").is_none() {
        return Err(());
    }

    let mut accum = ChatCompletionsC2rAccumulator::default();
    let mut out: Vec<String> = Vec::new();

    let role_json = serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]}).to_string();
    out.extend(chat_event_to_responses_events(
        &role_json, session_id, item_id, &mut accum,
    ));

    let content = assistant_message_content_from_completion(&v);
    if !content.is_empty() {
        let delta_json =
            serde_json::json!({"choices":[{"delta":{"content": content}}]}).to_string();
        out.extend(chat_event_to_responses_events(
            &delta_json,
            session_id,
            item_id,
            &mut accum,
        ));
    }

    let reason = v
        .pointer("/choices/0/finish_reason")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("stop");
    if let Some(m) = v.pointer("/choices/0/message") {
        accum.absorb_chat_message_tool_calls(m);
    }
    let usage = v.get("usage").cloned().unwrap_or(serde_json::Value::Null);
    let finish_json = serde_json::json!({
        "choices": [{"finish_reason": reason}],
        "usage": usage
    })
    .to_string();
    out.extend(chat_event_to_responses_events(
        &finish_json,
        session_id,
        item_id,
        &mut accum,
    ));

    Ok(out)
}

/// Convert a non-streaming Chat Completions response body to Responses API format.
pub fn chat_body_to_responses(body: &[u8], session_id: &str, item_id: &str) -> Bytes {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Bytes::copy_from_slice(body);
    };

    let content = assistant_message_content_from_completion(&v);
    let finish_reason = v
        .pointer("/choices/0/finish_reason")
        .and_then(|x| x.as_str())
        .unwrap_or("stop");

    let mut output: Vec<serde_json::Value> = Vec::new();
    let has_assistant_text = !content.trim().is_empty();
    let emit_assistant_message = finish_reason != "tool_calls" || has_assistant_text;
    if emit_assistant_message {
        let assistant_message_json = if finish_reason == "tool_calls" {
            serde_json::json!({
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "commentary",
                "content": [{
                    "type": "output_text",
                    "text": content
                }]
            })
        } else {
            serde_json::json!({
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": content
                }]
            })
        };
        output.push(assistant_message_json);
    }

    let mut fragments = BTreeMap::new();
    if finish_reason == "tool_calls" {
        if let Some(msg) = v.pointer("/choices/0/message") {
            if let Some(arr) = msg.get("tool_calls").and_then(|a| a.as_array()) {
                merge_tool_call_json_objects(&mut fragments, arr);
            }
        }
    }
    for frag in fragments.values() {
        let call_id = frag
            .id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("call_{}-{}", session_id, output.len()));
        let name = frag
            .name
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown_tool".into());
        let arguments = if frag.arguments.is_empty() {
            "{}".into()
        } else {
            frag.arguments.clone()
        };
        if name == "apply_patch" {
            output.push(serde_json::json!({
                "type": "custom_tool_call",
                "call_id": call_id,
                "name": name,
                "input": custom_tool_input_from_arguments(&arguments),
            }));
        } else {
            output.push(serde_json::json!({
                "type": "function_call",
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
            }));
        }
    }

    let usage = v.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let mut response_inner = serde_json::json!({
        "id": session_id,
        "object": "response",
        "status": "completed",
        "output": output,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    });

    if finish_reason == "tool_calls" {
        if let Some(obj) = response_inner.as_object_mut() {
            obj.insert(
                "end_turn".into(),
                serde_json::Value::Bool(fragments.is_empty()),
            );
        }
    }

    serde_json::to_vec(&response_inner)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

/// Prepend a completed assistant message to a non-streaming Responses JSON body.
pub fn prepend_response_message(body: &[u8], item_id: &str, text: &str) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(output) = v.get_mut("output").and_then(|x| x.as_array_mut()) else {
        return Bytes::copy_from_slice(body);
    };
    output.insert(
        0,
        serde_json::json!({
            "id": item_id,
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        }),
    );
    serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

/// Append text to the last assistant message in a non-streaming Responses JSON body.
pub fn append_response_message_text(body: &[u8], text: &str) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<Value>(body) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(output) = v.get_mut("output").and_then(|x| x.as_array_mut()) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(message) = output
        .iter_mut()
        .rev()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("message"))
    else {
        return Bytes::copy_from_slice(body);
    };
    let Some(content) = message.get_mut("content").and_then(|x| x.as_array_mut()) else {
        return Bytes::copy_from_slice(body);
    };
    let Some(part) = content
        .iter_mut()
        .rev()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
    else {
        return Bytes::copy_from_slice(body);
    };
    let Some(existing) = part
        .get_mut("text")
        .and_then(|x| x.as_str())
        .map(str::to_owned)
    else {
        return Bytes::copy_from_slice(body);
    };
    let separator = if existing.trim().is_empty() {
        ""
    } else {
        "\n\n"
    };
    if let Some(obj) = part.as_object_mut() {
        obj.insert(
            "text".into(),
            Value::String(format!("{existing}{separator}{text}")),
        );
    }
    serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(body))
}

// ---------------------------------------------------------------------------
// Codex WebSocket: terminal error frames
// ---------------------------------------------------------------------------

/// One SSE `data:` line indicates the upstream has finished the assistant turn
/// (Chat Completions `finish_reason`, or native Responses `response.completed` / `response.done`).
pub fn upstream_sse_data_is_terminal(data: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(data.trim()) else {
        return false;
    };
    match v.get("type").and_then(|t| t.as_str()) {
        Some(t) if t == "response.completed" || t == "response.done" => return true,
        _ => {}
    }
    if let Some(fr) = v
        .pointer("/choices/0/finish_reason")
        .and_then(|x| x.as_str())
    {
        return !fr.is_empty() && fr != "null";
    }
    false
}

fn truncate_error_detail(s: &str) -> String {
    const MAX: usize = 2048;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…", &s[..MAX])
    }
}

fn extract_upstream_error_detail(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(msg) = v.pointer("/error/message").and_then(|x| x.as_str()) {
            return truncate_error_detail(msg);
        }
        if let Some(msg) = v.get("message").and_then(|x| x.as_str()) {
            return truncate_error_detail(msg);
        }
        if let Some(msg) = v.as_str() {
            return truncate_error_detail(msg);
        }
    }
    truncate_error_detail(trimmed)
}

fn map_http_status_to_codex_error(status: u16, detail: &str) -> (&'static str, String) {
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(" — {detail}")
    };
    let base = format!("HTTP {status}{suffix}");
    match status {
        429 => ("rate_limit_exceeded", base),
        402 => ("insufficient_quota", base),
        401 => ("invalid_api_key", base),
        502 | 503 | 504 => ("server_is_overloaded", base),
        _ => ("slow_down", base),
    }
}

/// JSON text for one WebSocket frame; matches Codex `response.failed` handling
/// (see codex-rs `process_responses_event` for `response.failed`).
pub fn codex_response_failed_event(response_id: &str, http_status: u16, body: &str) -> String {
    let detail = extract_upstream_error_detail(body);
    let (code, message) = map_http_status_to_codex_error(http_status, &detail);
    serde_json::json!({
        "type": "response.failed",
        "response": {
            "id": response_id,
            "object": "response",
            "status": "failed",
            "error": {
                "code": code,
                "message": message
            }
        }
    })
    .to_string()
}

/// Problems detected by the proxy side, such as truncated streams or unparsable bodies.
///
/// Do not map these errors to `server_is_overloaded`: codex-rs displays that as
/// "Selected model is at capacity", which is misleading. Use a `code` not recognized by `is_server_overloaded_error`
/// so the client takes the `Retryable` branch and displays `message`.
pub fn codex_response_proxy_fault_event(
    response_id: &str,
    wire_code: &str,
    message: &str,
) -> String {
    serde_json::json!({
        "type": "response.failed",
        "response": {
            "id": response_id,
            "object": "response",
            "status": "failed",
            "error": {
                "code": wire_code,
                "message": message.trim()
            }
        }
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// reasoning_effort translation
// ---------------------------------------------------------------------------

/// Translate `reasoning_effort` (OpenAI Responses API) into provider-specific
/// thinking parameters for DeepSeek and Qwen Chat Completions bodies.
///
/// - DeepSeek (r1 / reasoner variants): `{"thinking": {"type": "enabled", "budget_tokens": N}}`
/// - Qwen (qwq / thinking variants):    `{"enable_thinking": true}`
/// - All others (o-series, gpt-5, etc.): pass through unchanged.
///
/// Only mutates the body when `reasoning_effort` is present and the model matches a
/// provider that needs translation. Returns `None` when no change is needed.
pub fn translate_reasoning_effort(body: &[u8], upstream_model: &str) -> Option<Bytes> {
    let mut v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let obj = v.as_object_mut()?;
    let effort_val = obj.remove("reasoning_effort")?;
    let effort = effort_val.as_str().unwrap_or("medium");

    let m = upstream_model.to_ascii_lowercase();
    let m = if let Some(pos) = m.rfind('/') {
        &m[pos + 1..]
    } else {
        &m
    };

    if m.starts_with("deepseek-r1")
        || m.contains("deepseek-reasoner")
        || m.starts_with("deepseek-prover")
    {
        let budget = match effort {
            "low" => 4_096i64,
            "high" => 32_768,
            _ => 8_192, // medium
        };
        obj.insert(
            "thinking".into(),
            serde_json::json!({"type": "enabled", "budget_tokens": budget}),
        );
    } else if m.starts_with("qwq") || m.contains("-thinking") || m.starts_with("qwen3") {
        if effort == "low" {
            obj.insert("enable_thinking".into(), serde_json::Value::Bool(false));
        } else {
            obj.insert("enable_thinking".into(), serde_json::Value::Bool(true));
        }
    } else {
        // Not a provider that needs translation — put effort back.
        obj.insert("reasoning_effort".into(), effort_val);
        return None;
    }

    serde_json::to_vec(&v).ok().map(Bytes::from)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_reasoning_deepseek_r1_medium() {
        let body = br#"{"model":"deepseek-r1","messages":[],"reasoning_effort":"medium"}"#;
        let out = translate_reasoning_effort(body, "deepseek-r1").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("reasoning_effort").is_none());
        assert_eq!(v["thinking"]["type"], "enabled");
        assert_eq!(v["thinking"]["budget_tokens"], 8_192);
    }

    #[test]
    fn translate_reasoning_deepseek_high_budget() {
        let body = br#"{"model":"deepseek-r1","messages":[],"reasoning_effort":"high"}"#;
        let out = translate_reasoning_effort(body, "deepseek-r1").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["thinking"]["budget_tokens"], 32_768);
    }

    #[test]
    fn translate_reasoning_qwen_thinking_enabled() {
        let body = br#"{"model":"qwq-32b","messages":[],"reasoning_effort":"high"}"#;
        let out = translate_reasoning_effort(body, "qwq-32b").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("reasoning_effort").is_none());
        assert_eq!(v["enable_thinking"], true);
    }

    #[test]
    fn translate_reasoning_qwen_low_disables_thinking() {
        let body = br#"{"model":"qwq-32b","messages":[],"reasoning_effort":"low"}"#;
        let out = translate_reasoning_effort(body, "qwq-32b").unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["enable_thinking"], false);
    }

    #[test]
    fn translate_reasoning_openai_passthrough() {
        let body = br#"{"model":"gpt-4o","messages":[],"reasoning_effort":"medium"}"#;
        let out = translate_reasoning_effort(body, "gpt-4o");
        assert!(out.is_none(), "should not translate for gpt-4o");
    }

    #[test]
    fn translate_reasoning_no_effort_returns_none() {
        let body = br#"{"model":"deepseek-r1","messages":[]}"#;
        let out = translate_reasoning_effort(body, "deepseek-r1");
        assert!(out.is_none());
    }

    #[test]
    fn strip_ws_envelope_removes_type() {
        let input = br#"{"type":"response.create","model":"gpt-5.4","input":[{"role":"user","content":"hi"}]}"#;
        let out = strip_ws_envelope(input);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("type").is_none());
        assert_eq!(v["model"], "gpt-5.4");
        assert!(v.get("input").is_some());
    }

    #[test]
    fn strip_ws_envelope_non_response_create_unchanged() {
        let input = br#"{"model":"gpt-4o","messages":[]}"#;
        let out = strip_ws_envelope(input);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["model"], "gpt-4o");
        assert!(v.get("type").is_none());
    }

    #[test]
    fn strip_vibe_codex_status_messages_removes_only_local_banner() {
        let input = serde_json::json!({
            "model": "gpt-5.5",
            "input": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "$$\\n\\scriptsize\\n\\color{#38bdf8}{\\textsf{Vibe+}\\,\\mid\\,\\textsf{TTFS}=10\\textsf{ms}}\\n$$"
                    }]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "real assistant output"}]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "hello"}]
                }
            ]
        });
        let out = strip_vibe_codex_status_messages(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let items = v["input"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["content"][0]["text"], "real assistant output");
        assert_eq!(items[1]["role"], "user");
    }

    #[test]
    fn responses_input_ends_with_user_message_only_for_user_tail() {
        let user_tail = serde_json::json!({
            "input": [
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "old"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
            ]
        });
        assert!(responses_input_ends_with_user_message(
            &serde_json::to_vec(&user_tail).unwrap()
        ));

        let tool_tail = serde_json::json!({
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "run"}]},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        });
        assert!(!responses_input_ends_with_user_message(
            &serde_json::to_vec(&tool_tail).unwrap()
        ));

        let status_then_tool_tail = serde_json::json!({
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "run"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "$$\\n\\color{#38bdf8}{\\textsf{Vibe+}\\,\\textsf{TTFS}=1\\textsf{ms}}\\n$$"}]},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        });
        assert!(!responses_input_ends_with_user_message(
            &serde_json::to_vec(&status_then_tool_tail).unwrap()
        ));
    }

    #[test]
    fn responses_input_tail_under_ws_envelope_suppresses_tool_continuation_status() {
        let input = serde_json::json!({
            "type": "response.create",
            "previous_response_id": "resp_1",
            "input": [
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ]
        });
        assert!(!responses_input_ends_with_user_message(
            &serde_json::to_vec(&input).unwrap()
        ));
    }

    #[test]
    fn rewrites_model_inside_ws_response_create_without_stripping_envelope() {
        let input = serde_json::json!({
            "type": "response.create",
            "previous_response_id": "resp_1",
            "model": "gpt-5.5",
            "input": []
        });
        let out = rewrite_responses_model(&serde_json::to_vec(&input).unwrap(), "upstream-model")
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["type"], "response.create");
        assert_eq!(v["previous_response_id"], "resp_1");
        assert_eq!(v["model"], "upstream-model");
    }

    #[test]
    fn strips_vibe_status_inside_nested_ws_response_object() {
        let input = serde_json::json!({
            "type": "response.create",
            "response": {
                "model": "gpt-5.5",
                "input": [
                    {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "$$\\n\\color{#38bdf8}{\\textsf{Vibe+}\\,\\textsf{TTFS}=1\\textsf{ms}}\\n$$"}]},
                    {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "next"}]}
                ]
            }
        });
        let out = strip_vibe_codex_status_messages(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let items = v["response"]["input"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], "user");
    }

    #[test]
    fn strip_vibe_codex_status_messages_removes_summary_tail() {
        let input = serde_json::json!({
            "model": "gpt-5.5",
            "input": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "↯ speed 31.8/s · in 42.1k"}]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "real output"}]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": "next"}]
                }
            ]
        });
        let out = strip_vibe_codex_status_messages(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let items = v["input"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["content"][0]["text"], "real output");
        assert_eq!(items[1]["role"], "user");
    }

    #[test]
    fn strip_vibe_codex_status_messages_removes_all_summary_presets_without_id() {
        let summary_texts = [
            "$$\n\\scriptsize\n\\color{#64748b}{\\textsf{Vibe+}\\,\\mid\\,\\textsf{speed}=\\textsf{31.8/s}}\n$$",
            "$$\n\\small\n\\color{#64748b}{\\mathrm{speed}=\\textsf{31.8/s}\\quad\\mathrm{in}=\\textsf{42.1k}}\n$$",
            "↯ speed 31.8/s · in 42.1k",
            "_↯ speed `31.8/s` · in `42.1k`_",
            "`speed 31.8/s` · `in 42.1k`",
            "_speed 31.8/s · in 42.1k_",
            "_This turn: speed 31.8/s · in 42.1k_",
            "speed 31.8/s | in 42.1k",
        ];

        for text in summary_texts {
            let input = serde_json::json!({
                "model": "gpt-5.5",
                "input": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}]
                    },
                    {
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "input_text", "text": "next"}]
                    }
                ]
            });
            let out = strip_vibe_codex_status_messages(&serde_json::to_vec(&input).unwrap());
            let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
            let items = v["input"].as_array().unwrap();
            assert_eq!(items.len(), 1, "failed to strip {text:?}");
            assert_eq!(items[0]["role"], "user");
        }
    }

    #[test]
    fn responses_to_chat_basic() {
        let input = serde_json::json!({
            "model": "gpt-5.4",
            "instructions": "You are helpful.",
            "input": [{"role": "user", "content": "say hi"}],
            "max_output_tokens": 100,
            "stream": true
        });
        let out = responses_to_chat(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["model"], "gpt-5.4");
        assert_eq!(v["max_tokens"], 100);
        assert_eq!(v["stream"], true);
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "say hi");
        assert!(v.get("input").is_none());
        assert!(v.get("instructions").is_none());
        assert!(v.get("max_output_tokens").is_none());
    }

    #[test]
    fn responses_to_chat_no_instructions() {
        let input = serde_json::json!({
            "model": "gpt-4o",
            "input": [{"role": "user", "content": "hi"}]
        });
        let out = responses_to_chat(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn chat_event_to_responses_delta() {
        let event = r#"{"id":"c1","choices":[{"delta":{"content":"hello"},"index":0,"finish_reason":null}]}"#;
        let mut acc = ChatCompletionsC2rAccumulator::default();
        let events = chat_event_to_responses_events(event, "resp-1", "msg-1", &mut acc);
        let delta_evt = events
            .iter()
            .find(|e| e.contains("\"type\":\"response.output_text.delta\""))
            .expect("delta event");
        let v: serde_json::Value = serde_json::from_str(delta_evt).unwrap();
        assert_eq!(v["type"], "response.output_text.delta");
        assert_eq!(v["delta"], "hello");
    }

    #[test]
    fn chat_event_to_responses_role() {
        let event =
            r#"{"id":"c1","choices":[{"delta":{"role":"assistant","content":""},"index":0}]}"#;
        let mut acc = ChatCompletionsC2rAccumulator::default();
        let events = chat_event_to_responses_events(event, "resp-1", "msg-1", &mut acc);
        let types: Vec<String> = events
            .iter()
            .filter_map(|e| {
                serde_json::from_str::<serde_json::Value>(e)
                    .ok()
                    .and_then(|v| v["type"].as_str().map(str::to_string))
            })
            .collect();
        let type_set: std::collections::HashSet<_> = types.iter().map(|s| s.as_str()).collect();
        assert!(
            type_set.contains("response.created"),
            "missing response.created, got {:?}",
            types
        );
        assert!(
            !type_set.contains("response.output_item.added"),
            "unexpected empty message shell: {:?}",
            types
        );
    }

    #[test]
    fn chat_event_to_responses_done() {
        let event = r#"{"id":"c1","choices":[{"delta":{},"index":0,"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#;
        let mut acc = ChatCompletionsC2rAccumulator::default();
        let events = chat_event_to_responses_events(event, "resp-1", "msg-1", &mut acc);
        assert!(!events.is_empty());
        let done = events
            .iter()
            .find(|e| e.contains("response.completed"))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(done).unwrap();
        assert_eq!(v["type"], "response.completed");
        assert_eq!(v["response"]["id"], "resp-1");
        assert_eq!(v["response"]["usage"]["input_tokens"], 10);
        assert_eq!(v["response"]["usage"]["output_tokens"], 5);
        assert!(v["response"].get("end_turn").is_none());
    }

    #[test]
    fn chat_event_finish_tool_calls_emits_function_calls_and_end_turn_false() {
        let start = r#"{"choices":[{"delta":{"role":"assistant","content":""},"index":0}]}"#;
        let tool_delta = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"x\"}"}}]},"index":0}]}"#;
        let end = r#"{"choices":[{"finish_reason":"tool_calls","delta":{}}],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#;
        let mut acc = ChatCompletionsC2rAccumulator::default();
        let _ = chat_event_to_responses_events(start, "resp-9", "msg-9", &mut acc);
        let _ = chat_event_to_responses_events(tool_delta, "resp-9", "msg-9", &mut acc);
        let events = chat_event_to_responses_events(end, "resp-9", "msg-9", &mut acc);

        let fn_evt = events
            .iter()
            .find(|e| e.contains("\"type\":\"function_call\""))
            .expect("function_call");
        let v: serde_json::Value = serde_json::from_str(fn_evt).unwrap();
        assert_eq!(v["item"]["name"], "read_file");
        assert_eq!(v["item"]["call_id"], "call_1");

        let done = events
            .iter()
            .find(|e| e.contains("response.completed"))
            .expect("completed");
        let d: serde_json::Value = serde_json::from_str(done).unwrap();
        assert_eq!(d["response"]["end_turn"], false);
    }

    #[test]
    fn chat_event_finish_apply_patch_emits_custom_tool_call_without_empty_message() {
        let start = r#"{"choices":[{"delta":{"role":"assistant","content":""},"index":0}]}"#;
        let tool_delta = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_patch","type":"function","function":{"name":"apply_patch","arguments":"{\"patch\":\"*** Begin Patch\\n*** End Patch\"}"}}]},"index":0}]}"#;
        let end = r#"{"choices":[{"finish_reason":"tool_calls","delta":{}}],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#;
        let mut acc = ChatCompletionsC2rAccumulator::default();
        let _ = chat_event_to_responses_events(start, "resp-patch", "msg-patch", &mut acc);
        let _ = chat_event_to_responses_events(tool_delta, "resp-patch", "msg-patch", &mut acc);
        let events = chat_event_to_responses_events(end, "resp-patch", "msg-patch", &mut acc);

        assert!(!events
            .iter()
            .any(|e| e.contains("\"response.output_item.added\"")));
        let tool_evt = events
            .iter()
            .find(|e| e.contains("\"type\":\"custom_tool_call\""))
            .expect("custom_tool_call");
        let v: serde_json::Value = serde_json::from_str(tool_evt).unwrap();
        assert_eq!(v["item"]["name"], "apply_patch");
        assert_eq!(v["item"]["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn responses_to_chat_function_call_output_maps_to_tool_role() {
        let input = serde_json::json!({
            "model": "m",
            "input": [
                {"type": "function_call", "call_id": "c1", "name": "read_file", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "tool ok"}
            ]
        });
        let out = responses_to_chat(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[0]["tool_calls"][0]["id"], "c1");
        assert_eq!(msgs[0]["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[1]["tool_call_id"], "c1");
        assert_eq!(msgs[1]["content"], "tool ok");
    }

    #[test]
    fn responses_to_chat_ignores_orphan_tool_output() {
        let input = serde_json::json!({
            "model": "m",
            "input": [
                {"type": "function_call_output", "call_id": "missing", "output": "tool ok"}
            ]
        });
        let out = responses_to_chat(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("messages").is_none());
    }

    #[test]
    fn responses_to_chat_maps_custom_apply_patch_to_function_tool_and_call() {
        let input = serde_json::json!({
            "model": "m",
            "tools": [
                {
                    "type": "custom",
                    "name": "apply_patch",
                    "description": "Apply patch"
                }
            ],
            "input": [
                {"type": "custom_tool_call", "call_id": "c_patch", "name": "apply_patch", "input": "*** Begin Patch\n*** End Patch"},
                {"type": "custom_tool_call_output", "call_id": "c_patch", "output": "ok"}
            ]
        });
        let out = responses_to_chat(&serde_json::to_vec(&input).unwrap());
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let tools = v["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "apply_patch");
        let msgs = v["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["tool_calls"][0]["function"]["name"], "apply_patch");
        assert_eq!(
            msgs[0]["tool_calls"][0]["function"]["arguments"],
            "{\"patch\":\"*** Begin Patch\\n*** End Patch\"}"
        );
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[1]["tool_call_id"], "c_patch");
    }

    #[test]
    fn chat_body_to_responses_maps_apply_patch_to_custom_tool_call() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_patch",
                        "type": "function",
                        "function": {
                            "name": "apply_patch",
                            "arguments": "{\"patch\":\"*** Begin Patch\\n*** End Patch\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 8, "completion_tokens": 2}
        });
        let out = chat_body_to_responses(&serde_json::to_vec(&body).unwrap(), "resp-1", "msg-1");
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["output"][0]["type"], "custom_tool_call");
        assert_eq!(v["output"][0]["name"], "apply_patch");
        assert_eq!(v["output"][0]["input"], "*** Begin Patch\n*** End Patch");
    }

    #[test]
    fn chat_body_to_responses_basic() {
        let body = serde_json::json!({
            "id": "chatcmpl-1",
            "choices": [{"message": {"role": "assistant", "content": "Hi!"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 8, "completion_tokens": 2}
        });
        let out = chat_body_to_responses(&serde_json::to_vec(&body).unwrap(), "resp-1", "msg-1");
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["status"], "completed");
        assert_eq!(v["output"][0]["content"][0]["text"], "Hi!");
        assert_eq!(v["usage"]["input_tokens"], 8);
        assert_eq!(v["usage"]["output_tokens"], 2);
    }

    #[test]
    fn prepend_response_message_inserts_status_first() {
        let body = serde_json::json!({
            "id": "resp-1",
            "object": "response",
            "output": [{
                "id": "msg-1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "real"}]
            }]
        });
        let out = prepend_response_message(
            &serde_json::to_vec(&body).unwrap(),
            "vibe-route",
            "$$\\small Vibe+$$",
        );
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let output = v["output"].as_array().unwrap();
        assert_eq!(output[0]["id"], "vibe-route");
        assert_eq!(output[0]["content"][0]["text"], "$$\\small Vibe+$$");
        assert_eq!(output[1]["id"], "msg-1");
    }

    #[test]
    fn append_response_message_text_appends_to_last_message() {
        let body = serde_json::json!({
            "id": "resp-1",
            "object": "response",
            "output": [{
                "id": "msg-1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "real"}]
            }]
        });
        let out = append_response_message_text(&serde_json::to_vec(&body).unwrap(), "summary");
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["output"][0]["content"][0]["text"], "real\n\nsummary");
    }

    #[test]
    fn chat_completion_non_stream_to_ws_events_emits_completed() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Hi!"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 8, "completion_tokens": 2}
        });
        let frames = chat_completion_non_stream_to_ws_events(
            &serde_json::to_vec(&body).unwrap(),
            "resp-1",
            "msg-1",
        )
        .unwrap();
        let completed = frames
            .iter()
            .find(|e| e.contains("response.completed"))
            .expect("completed");
        let v: serde_json::Value = serde_json::from_str(completed).unwrap();
        assert_eq!(v["type"], "response.completed");
        assert_eq!(v["response"]["id"], "resp-1");
    }

    #[test]
    fn upstream_sse_data_is_terminal_chat_finish() {
        assert!(upstream_sse_data_is_terminal(
            r#"{"choices":[{"finish_reason":"stop"}]}"#
        ));
    }

    #[test]
    fn codex_response_failed_503_uses_server_overloaded_code() {
        let s = codex_response_failed_event("rid-1", 503, r#"{"error":{"message":"no capacity"}}"#);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["type"], "response.failed");
        assert_eq!(v["response"]["id"], "rid-1");
        assert_eq!(v["response"]["error"]["code"], "server_is_overloaded");
        assert!(v["response"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("HTTP 503"));
    }

    #[test]
    fn codex_response_proxy_fault_avoids_overloaded_code() {
        let s = codex_response_proxy_fault_event(
            "rid-x",
            "upstream_stream_truncated",
            "SSE ended early",
        );
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["response"]["error"]["code"], "upstream_stream_truncated");
        assert_eq!(v["response"]["error"]["message"], "SSE ended early");
    }
}
