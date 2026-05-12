//! Vibe+ Claude Code controls: scenario routing and request shaping.

use crate::config::{
    ClaudeFallbackConfig, ClaudeRequestConfig, ClaudeRoutingConfig, ClaudeThinkingPolicy,
};
use crate::router;
use bytes::Bytes;
use serde_json::{Map, Value};
use std::collections::HashSet;
use vibe_protocol::{Provider, Route};

const SUBAGENT_MODEL_OPEN: &str = "<CCR-SUBAGENT-MODEL>";
const SUBAGENT_MODEL_CLOSE: &str = "</CCR-SUBAGENT-MODEL>";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaudeRouteScenario {
    Default,
    Background,
    Think,
    LongContext,
    WebSearch,
    Image,
}

impl ClaudeRouteScenario {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Background => "background",
            Self::Think => "think",
            Self::LongContext => "long_context",
            Self::WebSearch => "web_search",
            Self::Image => "image",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudePreparedRequest {
    pub body: Bytes,
    pub requested_model: String,
    pub route_model: Option<String>,
    pub scenario: ClaudeRouteScenario,
}

pub fn prepare_request(
    body: Bytes,
    requested_model: String,
    routing: &ClaudeRoutingConfig,
    request: &ClaudeRequestConfig,
) -> ClaudePreparedRequest {
    let Ok(mut v) = serde_json::from_slice::<Value>(&body) else {
        return ClaudePreparedRequest {
            body,
            requested_model,
            route_model: None,
            scenario: ClaudeRouteScenario::Default,
        };
    };

    let mut subagent_model = None;
    if routing.enabled && routing.enable_subagent_model_tag {
        subagent_model = extract_subagent_model_tag(&mut v);
    }

    apply_request_controls(&mut v, request);

    let scenario = detect_scenario(&v, &requested_model, routing);
    let route_model = if routing.enabled {
        subagent_model.or_else(|| route_model_for_scenario(scenario, routing))
    } else {
        None
    };
    let final_requested_model = route_model
        .as_ref()
        .map(|model| model_slug(model))
        .unwrap_or_else(|| requested_model.clone());

    let body = serde_json::to_vec(&v)
        .map(Bytes::from)
        .unwrap_or_else(|_| Bytes::copy_from_slice(&body));

    ClaudePreparedRequest {
        body,
        requested_model: final_requested_model,
        route_model,
        scenario,
    }
}

pub fn candidates_for_request(
    providers: &[Provider],
    routes: &[Route],
    wire: crate::providers::Wire,
    requested_model: &str,
    route_model: Option<&str>,
    scenario: ClaudeRouteScenario,
    fallback: &ClaudeFallbackConfig,
) -> (Option<Route>, Vec<router::Pick>) {
    let primary_model = route_model
        .map(model_slug)
        .unwrap_or_else(|| requested_model.to_string());
    let (route, mut candidates) =
        router::candidates_with_routes(providers, routes, wire, &primary_model);

    if fallback.enabled {
        let mut seen = candidate_keys(&candidates);
        for fallback_model in fallback_models_for_scenario(scenario, fallback) {
            let fallback_model = fallback_model.trim();
            if fallback_model.is_empty() {
                continue;
            }
            for pick in picks_for_route_model(providers, routes, wire, fallback_model) {
                let key = (pick.provider.id.clone(), pick.upstream_model.clone());
                if seen.insert(key) {
                    candidates.push(pick);
                }
            }
        }
    }

    (route, candidates)
}

fn candidate_keys(candidates: &[router::Pick]) -> HashSet<(String, String)> {
    candidates
        .iter()
        .map(|pick| (pick.provider.id.clone(), pick.upstream_model.clone()))
        .collect()
}

fn picks_for_route_model(
    providers: &[Provider],
    routes: &[Route],
    wire: crate::providers::Wire,
    route_model: &str,
) -> Vec<router::Pick> {
    let (provider_hint, model) = split_provider_model(route_model);
    let (_route, mut picks) = router::candidates_with_routes(providers, routes, wire, &model);
    if let Some(provider_hint) = provider_hint {
        let hint = provider_hint.to_ascii_lowercase();
        picks.retain(|pick| {
            pick.provider.id.eq_ignore_ascii_case(&hint)
                || pick.provider.name.to_ascii_lowercase() == hint
        });
    }
    picks
}

fn fallback_models_for_scenario<'a>(
    scenario: ClaudeRouteScenario,
    fallback: &'a ClaudeFallbackConfig,
) -> &'a [String] {
    match scenario {
        ClaudeRouteScenario::Default => &fallback.default,
        ClaudeRouteScenario::Background => &fallback.background,
        ClaudeRouteScenario::Think => &fallback.think,
        ClaudeRouteScenario::LongContext => &fallback.long_context,
        ClaudeRouteScenario::WebSearch => &fallback.web_search,
        ClaudeRouteScenario::Image => &fallback.image,
    }
}

fn route_model_for_scenario(
    scenario: ClaudeRouteScenario,
    routing: &ClaudeRoutingConfig,
) -> Option<String> {
    let raw = match scenario {
        ClaudeRouteScenario::Default => &routing.default_model,
        ClaudeRouteScenario::Background => &routing.background_model,
        ClaudeRouteScenario::Think => &routing.think_model,
        ClaudeRouteScenario::LongContext => &routing.long_context_model,
        ClaudeRouteScenario::WebSearch => &routing.web_search_model,
        ClaudeRouteScenario::Image => &routing.image_model,
    };
    let raw = raw.trim();
    (!raw.is_empty()).then(|| raw.to_string())
}

fn detect_scenario(
    body: &Value,
    requested_model: &str,
    routing: &ClaudeRoutingConfig,
) -> ClaudeRouteScenario {
    if !routing.enabled {
        return ClaudeRouteScenario::Default;
    }
    if token_count_estimate(body) > routing.long_context_threshold_tokens as usize
        && !routing.long_context_model.trim().is_empty()
    {
        return ClaudeRouteScenario::LongContext;
    }
    if routing.route_haiku_to_background
        && requested_model.to_ascii_lowercase().contains("haiku")
        && !routing.background_model.trim().is_empty()
    {
        return ClaudeRouteScenario::Background;
    }
    if request_has_image(body) && !routing.image_model.trim().is_empty() {
        return ClaudeRouteScenario::Image;
    }
    if request_has_web_search_tool(body) && !routing.web_search_model.trim().is_empty() {
        return ClaudeRouteScenario::WebSearch;
    }
    if body.get("thinking").is_some() && !routing.think_model.trim().is_empty() {
        return ClaudeRouteScenario::Think;
    }
    ClaudeRouteScenario::Default
}

fn apply_request_controls(v: &mut Value, request: &ClaudeRequestConfig) {
    let Some(obj) = v.as_object_mut() else {
        return;
    };

    if request.disable_web_search {
        remove_web_search_tools(obj);
    }

    if let Some(default_max_tokens) = request.default_max_tokens {
        if obj.get("max_tokens").and_then(|x| x.as_u64()).is_none() {
            obj.insert(
                "max_tokens".into(),
                Value::Number(serde_json::Number::from(default_max_tokens as u64)),
            );
        }
    }
    if let Some(cap) = request.max_tokens_cap {
        let next = obj
            .get("max_tokens")
            .and_then(|x| x.as_u64())
            .map(|n| n.min(cap as u64))
            .unwrap_or(cap as u64);
        obj.insert(
            "max_tokens".into(),
            Value::Number(serde_json::Number::from(next)),
        );
    }

    match request.thinking_policy {
        ClaudeThinkingPolicy::Preserve => {}
        ClaudeThinkingPolicy::Remove => {
            obj.remove("thinking");
        }
        ClaudeThinkingPolicy::ForceEnabled => {
            let budget = request.thinking_budget_tokens.unwrap_or(4096);
            obj.insert(
                "thinking".into(),
                serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget
                }),
            );
        }
    }
}

fn remove_web_search_tools(obj: &mut Map<String, Value>) {
    let Some(Value::Array(tools)) = obj.get_mut("tools") else {
        return;
    };
    tools.retain(|tool| !tool_type(tool).is_some_and(|t| t.starts_with("web_search")));
}

fn extract_subagent_model_tag(v: &mut Value) -> Option<String> {
    let system = v.get_mut("system")?;
    match system {
        Value::String(s) => extract_tag_from_string(s),
        Value::Array(blocks) => {
            for block in blocks {
                if block.get("type").and_then(|x| x.as_str()) != Some("text") {
                    continue;
                }
                if let Some(Value::String(text)) = block.get_mut("text") {
                    if let Some(model) = extract_tag_from_string(text) {
                        return Some(model);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_tag_from_string(text: &mut String) -> Option<String> {
    let start = text.find(SUBAGENT_MODEL_OPEN)?;
    let after_start = start + SUBAGENT_MODEL_OPEN.len();
    let end_rel = text[after_start..].find(SUBAGENT_MODEL_CLOSE)?;
    let end = after_start + end_rel;
    let model = text[after_start..end].trim().to_string();
    text.replace_range(start..end + SUBAGENT_MODEL_CLOSE.len(), "");
    (!model.is_empty()).then_some(model)
}

fn request_has_web_search_tool(body: &Value) -> bool {
    body.get("tools")
        .and_then(|tools| tools.as_array())
        .map(|tools| {
            tools
                .iter()
                .any(|tool| tool_type(tool).is_some_and(|t| t.starts_with("web_search")))
        })
        .unwrap_or(false)
}

fn request_has_image(body: &Value) -> bool {
    value_has_image(body.get("messages").unwrap_or(&Value::Null))
}

fn value_has_image(v: &Value) -> bool {
    match v {
        Value::Array(items) => items.iter().any(value_has_image),
        Value::Object(obj) => {
            obj.get("type")
                .and_then(|x| x.as_str())
                .is_some_and(|t| t == "image" || t == "image_url")
                || obj.values().any(value_has_image)
        }
        _ => false,
    }
}

fn tool_type(tool: &Value) -> Option<&str> {
    tool.get("type").and_then(|x| x.as_str())
}

fn token_count_estimate(v: &Value) -> usize {
    // Cheap deterministic approximation good enough for routing thresholds.
    // JSON carries structure and escaping overhead, so divide by four as a rough
    // token estimate instead of pulling a tokenizer into the gateway hot path.
    serde_json::to_string(v).map(|s| s.len() / 4).unwrap_or(0)
}

fn split_provider_model(raw: &str) -> (Option<String>, String) {
    let raw = raw.trim();
    if let Some((provider, model)) = raw.split_once(',') {
        let provider = provider.trim();
        let model = model.trim();
        if !provider.is_empty() && !model.is_empty() {
            return (Some(provider.to_string()), model.to_string());
        }
    }
    (None, raw.to_string())
}

fn model_slug(raw: &str) -> String {
    split_provider_model(raw).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ClaudeRequestConfig, ClaudeRoutingConfig};

    #[test]
    fn subagent_tag_is_removed_and_used_as_route_model() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet","system":[{"type":"text","text":"<CCR-SUBAGENT-MODEL>anthropic,claude-haiku</CCR-SUBAGENT-MODEL> hi"}],"messages":[]}"#,
        );
        let out = prepare_request(
            body,
            "claude-sonnet".into(),
            &ClaudeRoutingConfig::default(),
            &ClaudeRequestConfig::default(),
        );
        assert_eq!(out.route_model.as_deref(), Some("anthropic,claude-haiku"));
        let v: Value = serde_json::from_slice(&out.body).unwrap();
        assert!(!v["system"][0]["text"]
            .as_str()
            .unwrap()
            .contains("CCR-SUBAGENT-MODEL"));
    }

    #[test]
    fn request_controls_cap_tokens_and_remove_web_search() {
        let mut req = ClaudeRequestConfig::default();
        req.max_tokens_cap = Some(1024);
        req.disable_web_search = true;
        let body = Bytes::from_static(
            br#"{"model":"claude","max_tokens":4096,"tools":[{"type":"web_search_20250305"},{"type":"custom"}],"messages":[]}"#,
        );
        let out = prepare_request(body, "claude".into(), &ClaudeRoutingConfig::default(), &req);
        let v: Value = serde_json::from_slice(&out.body).unwrap();
        assert_eq!(v["max_tokens"].as_u64(), Some(1024));
        assert_eq!(v["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn detects_think_scenario_when_configured() {
        let mut routing = ClaudeRoutingConfig::default();
        routing.think_model = "anthropic,claude-opus".into();
        let out = prepare_request(
            Bytes::from_static(
                br#"{"model":"claude-sonnet","thinking":{"type":"enabled","budget_tokens":4000},"messages":[]}"#,
            ),
            "claude-sonnet".into(),
            &routing,
            &ClaudeRequestConfig::default(),
        );
        assert_eq!(out.scenario, ClaudeRouteScenario::Think);
        assert_eq!(out.route_model.as_deref(), Some("anthropic,claude-opus"));
        assert_eq!(out.requested_model, "claude-opus");
    }
}
