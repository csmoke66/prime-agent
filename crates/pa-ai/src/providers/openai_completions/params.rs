//! `OpenAI` Completions request params assembly.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::models::supports_thinking;
use crate::providers::openai_completions::convert::{convert_messages, convert_tools};
use crate::providers::openai_completions::has_tool_history;
use crate::providers::openai_completions::{
    OpenAICompatCacheControl, OpenAICompletionsOptions, ResolvedCompat,
};
use crate::types::{CacheRetention, Context, Model, ModelExt, ModelThinkingLevel};

// Long by design: mirrors the provider's stream shape.
#[allow(clippy::too_many_lines)]
pub(crate) fn build_params(
    model: &Model,
    context: &Context,
    options: Option<&OpenAICompletionsOptions>,
    compat: &ResolvedCompat,
    cache_retention: CacheRetention,
    cache_control: Option<&OpenAICompatCacheControl>,
) -> Value {
    let options = options.cloned().unwrap_or_default();
    let messages = convert_messages(model, context, compat);
    let mut params = Map::new();
    params.insert("model".into(), json!(model.id));
    params.insert("messages".into(), json!(messages));
    params.insert("stream".into(), json!(true));

    let prompt_cache_key = if (model.base_url.contains("api.openai.com")
        && cache_retention != CacheRetention::None)
        || (cache_retention == CacheRetention::Long && compat.supports_long_cache_retention)
    {
        options.base.session_id.clone().map(Value::String)
    } else {
        None
    };
    if let Some(key) = prompt_cache_key {
        params.insert("prompt_cache_key".into(), key);
    }
    if cache_retention == CacheRetention::Long && compat.supports_long_cache_retention {
        params.insert("prompt_cache_retention".into(), json!("24h"));
    }

    if compat.supports_usage_in_streaming {
        params.insert("stream_options".into(), json!({ "include_usage": true }));
    }

    if compat.supports_store {
        params.insert("store".into(), json!(false));
    }

    if let Some(max_tokens) = options.base.max_tokens {
        if compat.max_tokens_field == crate::types::MaxTokensField::MaxTokens {
            params.insert("max_tokens".into(), json!(max_tokens));
        } else {
            params.insert("max_completion_tokens".into(), json!(max_tokens));
        }
    }

    if let Some(temperature) = options.base.temperature {
        params.insert("temperature".into(), json!(temperature));
    }

    let mut tools: Option<Vec<Value>> = None;
    if let Some(context_tools) = &context.tools {
        if !context_tools.is_empty() {
            tools = Some(convert_tools(context_tools, compat));
            if compat.zai_tool_stream {
                params.insert("tool_stream".into(), json!(true));
            }
        }
    }
    if tools.is_none() && has_tool_history(&context.messages) {
        // Anthropic (via LiteLLM/proxy) requires the tools param when the conversation has
        // tool_calls/tool_results.
        tools = Some(Vec::new());
    }
    if let Some(tools) = &tools {
        params.insert("tools".into(), json!(tools));
    }

    if let Some(cache_control) = cache_control {
        apply_anthropic_cache_control(&mut params, cache_control);
    }

    if let Some(tool_choice) = &options.tool_choice {
        params.insert("tool_choice".into(), tool_choice.to_json());
    }

    if supports_thinking(model) {
        match compat.thinking_format {
            crate::types::ThinkingFormat::Zai | crate::types::ThinkingFormat::Qwen => {
                params.insert(
                    "enable_thinking".into(),
                    json!(options.reasoning_effort.is_some()),
                );
            }
            crate::types::ThinkingFormat::QwenChatTemplate => {
                params.insert(
                    "chat_template_kwargs".into(),
                    json!({
                        "enable_thinking": options.reasoning_effort.is_some(),
                        "preserve_thinking": true,
                    }),
                );
            }
            crate::types::ThinkingFormat::Deepseek => {
                params.insert(
                    "thinking".into(),
                    json!({
                        "type": if options.reasoning_effort.is_some() { "enabled" } else { "disabled" },
                    }),
                );
                if let Some(effort) = options.reasoning_effort {
                    let mapped = model
                        .thinking_level_map_value(effort)
                        .flatten()
                        .cloned()
                        .unwrap_or_else(|| effort.wire_name().to_string());
                    params.insert("reasoning_effort".into(), json!(mapped));
                }
            }
            crate::types::ThinkingFormat::Openrouter => {
                // OpenRouter distinguishes an omitted reasoning preference (use the model default),
                // an explicit toggle, and an explicit effort selection.
                let declared_effort = options
                    .reasoning_effort
                    .filter(|_| compat.supports_reasoning_effort);
                if let Some(effort) = declared_effort {
                    let mapped = model
                        .thinking_level_map_value(effort)
                        .flatten()
                        .cloned()
                        .unwrap_or_else(|| effort.wire_name().to_string());
                    params.insert("reasoning".into(), json!({ "effort": mapped }));
                } else if options.reasoning_enabled == Some(true) {
                    params.insert("reasoning".into(), json!({ "enabled": true }));
                } else if options.reasoning_enabled == Some(false) {
                    let off = model
                        .thinking_level_map
                        .as_ref()
                        .and_then(|map| map.get(&ModelThinkingLevel::Off));
                    // TS `thinkingLevelMap?.off !== null`: only an explicit null suppresses the
                    // disable; a missing key or map still disables reasoning.
                    if !off.is_some_and(std::option::Option::is_none) {
                        if compat.supports_reasoning_effort {
                            let off_value = off
                                .and_then(|value| value.as_deref())
                                .unwrap_or("none")
                                .to_string();
                            params.insert("reasoning".into(), json!({ "effort": off_value }));
                        } else {
                            params.insert("reasoning".into(), json!({ "enabled": false }));
                        }
                    }
                }
            }
            crate::types::ThinkingFormat::Openai => {
                if let Some(effort) = options.reasoning_effort {
                    if compat.supports_reasoning_effort {
                        let mapped = model
                            .thinking_level_map_value(effort)
                            .flatten()
                            .cloned()
                            .unwrap_or_else(|| effort.wire_name().to_string());
                        params.insert("reasoning_effort".into(), json!(mapped));
                    }
                } else if options.reasoning_enabled == Some(false)
                    && compat.supports_reasoning_effort
                {
                    let off = model
                        .thinking_level_map
                        .as_ref()
                        .and_then(|map| map.get(&ModelThinkingLevel::Off));
                    // TS `thinkingLevelMap?.off !== null`: only an explicit null suppresses the
                    // disable; a missing key or map still sends the off value.
                    if !off.is_some_and(std::option::Option::is_none) {
                        let off_value = off
                            .and_then(|value| value.as_deref())
                            .unwrap_or("none")
                            .to_string();
                        params.insert("reasoning_effort".into(), json!(off_value));
                    }
                }
            }
        }
    }

    // OpenAI and OpenRouter accept a top-level service_tier
    // (https://openrouter.ai/docs/guides/features/service-tiers); other OpenAI-compatible
    // gateways may reject unknown fields.
    if let Some(service_tier) = options.base.service_tier {
        if model.provider == "openai" || model.provider == "openrouter" {
            params.insert(
                "service_tier".into(),
                serde_json::to_value(service_tier).unwrap_or(Value::Null),
            );
        }
    }

    if model.base_url.contains("openrouter.ai") {
        if let Some(crate::types::CompatKind::OpenAiCompletions(compat)) = model.compat_kind() {
            if let Some(routing) = &compat.as_ref().open_router_routing {
                params.insert(
                    "provider".into(),
                    serde_json::to_value(routing).unwrap_or(Value::Null),
                );
            }
        }
    }

    if model.base_url.contains("ai-gateway.vercel.sh") {
        if let Some(crate::types::CompatKind::OpenAiCompletions(compat)) = model.compat_kind() {
            if let Some(routing) = &compat.as_ref().vercel_gateway_routing {
                let mut gateway_options = Map::new();
                if let Some(only) = &routing.only {
                    gateway_options.insert("only".into(), json!(only));
                }
                if let Some(order) = &routing.order {
                    gateway_options.insert("order".into(), json!(order));
                }
                if !gateway_options.is_empty() {
                    params.insert(
                        "providerOptions".into(),
                        json!({ "gateway": Value::Object(gateway_options) }),
                    );
                }
            }
        }
    }

    Value::Object(params)
}

fn apply_anthropic_cache_control(
    params: &mut Map<String, Value>,
    cache_control: &OpenAICompatCacheControl,
) {
    // Last tool.
    if let Some(tools) = params
        .get_mut("tools")
        .and_then(|value| value.as_array_mut())
    {
        if let Some(last_tool) = tools.last_mut() {
            last_tool
                .as_object_mut()
                .expect("tools entries are objects")
                .insert("cache_control".into(), cache_control.to_json());
        }
    }
    let Some(messages) = params
        .get_mut("messages")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    // System prompt.
    for message in messages.iter_mut() {
        let role = message.get("role").and_then(|value| value.as_str());
        if role == Some("system") || role == Some("developer") {
            add_cache_control_to_message(message, cache_control);
            break;
        }
    }
    // Last conversation message (user/assistant/tool), from the end.
    for message in messages.iter_mut().rev() {
        let role = message
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if (role == "user" || role == "assistant" || role == "tool")
            && add_cache_control_to_message(message, cache_control)
        {
            break;
        }
    }
}

fn add_cache_control_to_message(
    message: &mut Value,
    cache_control: &OpenAICompatCacheControl,
) -> bool {
    let cache_json = cache_control.to_json();
    match message.get_mut("content") {
        Some(Value::String(content)) => {
            if content.is_empty() {
                return false;
            }
            let text = content.clone();
            message
                .as_object_mut()
                .expect("messages are objects")
                .insert(
                    "content".into(),
                    json!([{
                        "type": "text",
                        "text": text,
                        "cache_control": cache_json,
                    }]),
                );
            true
        }
        Some(Value::Array(content)) => {
            for part in content.iter_mut().rev() {
                if part.get("type").and_then(|value| value.as_str()) == Some("text") {
                    part.as_object_mut()
                        .expect("text parts are objects")
                        .insert("cache_control".into(), cache_json);
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

pub(crate) fn build_headers(
    model: &Model,
    api_key: &str,
    options_headers: Option<&HashMap<String, String>>,
    cache_session_id: Option<&str>,
    compat: &ResolvedCompat,
    conversation_id: Option<&str>,
) -> Vec<(String, String)> {
    // The default Authorization goes in first so model and caller headers replace it (header names
    // compare case-insensitively) instead of shipping a second Authorization alongside theirs.
    let mut headers: Vec<(String, String)> =
        vec![("Authorization".into(), format!("Bearer {api_key}"))];
    for (name, value) in model.headers.iter().flatten() {
        headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        headers.push((name.clone(), value.clone()));
    }

    if let Some(session_id) = cache_session_id {
        if compat.send_session_affinity_headers {
            headers.push(("session_id".into(), session_id.to_string()));
            headers.push(("x-client-request-id".into(), session_id.to_string()));
            headers.push(("x-session-affinity".into(), session_id.to_string()));
        }
    }

    if let Some(options_headers) = options_headers {
        for (name, value) in options_headers {
            headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
            headers.push((name.clone(), value.clone()));
        }
    }

    crate::providers::opencode_headers::apply(&mut headers, &model.provider, conversation_id);
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::clamp_thinking_level;
    use crate::models_generated;
    use crate::types::{Message, StreamOptions, UserMessage, UserMessageContent};

    /// Discussion #3562: an explicit Authorization from the model or the caller replaces the
    /// default bearer header instead of shipping beside it, whatever the header name's case.
    #[test]
    fn explicit_authorization_replaces_the_default_without_case_duplicates() {
        let mut model = models_generated::get_model("zai", "glm-4.7")
            .expect("the catalog carries a zai model")
            .clone();
        let authorizations = |model: &Model, caller: Option<&HashMap<String, String>>| {
            let compat = crate::providers::openai_completions::get_compat(model);
            build_headers(model, "default-key", caller, None, &compat, None)
                .into_iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .collect::<Vec<_>>()
        };

        model.headers = None;
        assert_eq!(
            authorizations(&model, None),
            vec![(
                "Authorization".to_string(),
                "Bearer default-key".to_string()
            )]
        );

        let caller = HashMap::from([(
            "authorization".to_string(),
            "Bearer caller-token".to_string(),
        )]);
        assert_eq!(
            authorizations(&model, Some(&caller)),
            vec![(
                "authorization".to_string(),
                "Bearer caller-token".to_string()
            )]
        );

        model.headers = Some(std::collections::BTreeMap::from([(
            "AUTHORIZATION".to_string(),
            "Bearer model-token".to_string(),
        )]));
        assert_eq!(
            authorizations(&model, None),
            vec![(
                "AUTHORIZATION".to_string(),
                "Bearer model-token".to_string()
            )]
        );
        // The caller still wins over the model.
        assert_eq!(
            authorizations(&model, Some(&caller)),
            vec![(
                "authorization".to_string(),
                "Bearer caller-token".to_string()
            )]
        );
    }

    /// Assemble params for a compiled catalog model with a reasoning level requested, mirroring
    /// `streamSimpleOpenAICompletions`: the level clamps through the model's thinking-level map;
    /// the effort and the on/off toggle ride along unless clamped to off.
    fn reasoning_params_for(model: &Model, level: ModelThinkingLevel) -> Map<String, Value> {
        let context = Context {
            system_prompt: None,
            messages: vec![Message::User(UserMessage {
                content: UserMessageContent::Text("Hi".into()),
                timestamp: 1,
                rest: Map::default(),
            })],
            tools: None,
        };
        let clamped = clamp_thinking_level(model, level);
        let mut options = OpenAICompletionsOptions::from_base(StreamOptions {
            api_key: Some("test".into()),
            ..Default::default()
        });
        options.reasoning_effort = (clamped != ModelThinkingLevel::Off).then_some(clamped);
        options.reasoning_enabled = Some(clamped != ModelThinkingLevel::Off);
        let params = build_params(
            model,
            &context,
            Some(&options),
            &crate::providers::openai_completions::get_compat(model),
            CacheRetention::None,
            None,
        );
        match params {
            Value::Object(map) => map,
            _ => panic!("build_params returns a JSON object"),
        }
    }

    /// Assemble params with a service tier requested over the base stream options (the shape the
    /// daemon's provider adapter and `stream_simple` both hand the completions path).
    fn tiered_params(
        provider: &str,
        model_id: &str,
        tier: Option<crate::types::ServiceTier>,
    ) -> Map<String, Value> {
        let model = models_generated::get_model(provider, model_id)
            .unwrap_or_else(|| panic!("compiled catalog carries {provider}/{model_id}"));
        let context = Context {
            system_prompt: None,
            messages: vec![Message::User(UserMessage {
                content: UserMessageContent::Text("Hi".into()),
                timestamp: 1,
                rest: Map::default(),
            })],
            tools: None,
        };
        let options = OpenAICompletionsOptions::from_base(StreamOptions {
            api_key: Some("test".into()),
            service_tier: tier,
            ..Default::default()
        });
        let params = build_params(
            model,
            &context,
            Some(&options),
            &crate::providers::openai_completions::get_compat(model),
            CacheRetention::None,
            None,
        );
        match params {
            Value::Object(map) => map,
            _ => panic!("build_params returns a JSON object"),
        }
    }

    /// TS #2144: `service_tier` forwards for `OpenAI` and `OpenRouter` only.
    #[test]
    fn forwards_service_tier_for_openai_and_openrouter_only() {
        use crate::types::ServiceTier;
        for (provider, model_id) in [
            ("openai", "gpt-4"),
            ("openrouter", "~anthropic/claude-fable-latest"),
        ] {
            let params = tiered_params(provider, model_id, Some(ServiceTier::Priority));
            assert_eq!(
                params.get("service_tier"),
                Some(&json!("priority")),
                "{provider}/{model_id}: the requested tier must reach the request"
            );
            let unset = tiered_params(provider, model_id, None);
            assert!(
                !unset.contains_key("service_tier"),
                "{provider}/{model_id}: no tier requested means no field"
            );
        }
        let gateway = tiered_params("zai", "glm-4.7", Some(ServiceTier::Priority));
        assert!(
            !gateway.contains_key("service_tier"),
            "gateways never receive the field"
        );
    }

    /// A compiled fallback catalog entry, straight from `models_generated` (the conservative
    /// offline floor).
    fn compiled_params(
        provider: &str,
        model_id: &str,
        level: ModelThinkingLevel,
    ) -> Map<String, Value> {
        let model = models_generated::get_model(provider, model_id)
            .unwrap_or_else(|| panic!("compiled catalog carries {provider}/{model_id}"));
        reasoning_params_for(model, level)
    }

    /// The direct z.ai routes keep the toggle: their compat still selects the zai thinking format,
    /// so reasoning requests send `enable_thinking`.
    #[test]
    fn keeps_the_zai_thinking_toggle_on_direct_zai_routes() {
        let params = compiled_params("zai", "glm-4.7", ModelThinkingLevel::High);
        assert_eq!(params.get("enable_thinking"), Some(&json!(true)));
    }

    /// A `reasoning: false` model whose map addresses levels (the live catalog's
    /// `gpt-5.3-chat-latest` / `openai/gpt-5.2-chat` shape) is thinking-capable: the requested
    /// level clamps through the map and the request carries the mapped reasoning parameter. The
    /// flag alone must not veto a route that declares addressable levels.
    #[test]
    fn a_map_addressable_model_sends_reasoning_without_the_flag() {
        let model = serde_json::from_value::<Model>(json!({
            "id": "openai/gpt-5.2-chat", "name": "GPT-5.2 Chat",
            "api": "openai-completions", "provider": "openrouter",
            "baseUrl": "https://openrouter.ai/api/v1", "reasoning": false,
            "thinkingLevelMap": { "off": null, "xhigh": "xhigh" }, "input": ["text"],
            "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 },
            "contextWindow": 128_000, "maxTokens": 4_096
        }))
        .unwrap();
        // An unlisted level sends its wire name (the implicit default).
        let params = reasoning_params_for(&model, ModelThinkingLevel::High);
        assert_eq!(params.get("reasoning"), Some(&json!({ "effort": "high" })));
        // The clamped ladder stops at the map's addressable top.
        let params = reasoning_params_for(&model, ModelThinkingLevel::Max);
        assert_eq!(params.get("reasoning"), Some(&json!({ "effort": "xhigh" })));
    }
}
