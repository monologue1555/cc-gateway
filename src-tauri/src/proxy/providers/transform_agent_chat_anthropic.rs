//! OpenAI Chat Completions ↔ Anthropic Messages conversion for the Agent Gateway.
//!
//! This module deliberately does not depend on the Codex-specific Responses
//! converters.  A generic agent can therefore use `/v1/chat/completions` while
//! the selected upstream provider exposes Anthropic's native `/v1/messages`
//! endpoint.

use crate::proxy::{error::ProxyError, json_canonical::canonical_json_string};
use serde_json::{json, Map, Value};

const DEFAULT_CONTINUATION_MESSAGE: &str = "(continuing the conversation)";

/// Convert an OpenAI Chat Completions request into an Anthropic Messages request.
///
/// `default_max_tokens` is used because Anthropic requires `max_tokens`, while
/// Chat Completions clients are allowed to omit both output-token fields.
pub fn chat_completions_request_to_anthropic(
    body: Value,
    default_max_tokens: u64,
) -> Result<Value, ProxyError> {
    let object = body.as_object().ok_or_else(|| {
        ProxyError::InvalidRequest("Chat Completions request must be a JSON object".to_string())
    })?;

    let model = required_non_empty_string(object.get("model"), "model")?;
    let source_messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProxyError::InvalidRequest(
                "Chat Completions request must contain a messages array".to_string(),
            )
        })?;

    let mut system_parts = Vec::new();
    let mut messages = Vec::new();

    for (index, message) in source_messages.iter().enumerate() {
        let message = message.as_object().ok_or_else(|| {
            ProxyError::InvalidRequest(format!("messages[{index}] must be an object"))
        })?;
        let role =
            required_non_empty_string(message.get("role"), &format!("messages[{index}].role"))?;

        match role {
            "system" | "developer" => {
                system_parts.extend(system_text_parts(message.get("content"), index)?);
            }
            "user" => {
                let blocks = chat_content_to_anthropic_blocks(
                    message.get("content"),
                    index,
                    ContentRole::User,
                )?;
                push_message_blocks(&mut messages, "user", blocks);
            }
            "assistant" => {
                let mut blocks = Vec::new();

                // Chat Completions has no standard carrier for Anthropic's signed
                // thinking blocks.  Preserve reasoning as ordinary assistant text;
                // replaying it as an unsigned `thinking` block would make Anthropic
                // reject the next tool-result turn.
                if let Some(reasoning) = message
                    .get("reasoning_content")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    blocks.push(json!({ "type": "text", "text": reasoning }));
                }

                blocks.extend(chat_content_to_anthropic_blocks(
                    message.get("content"),
                    index,
                    ContentRole::Assistant,
                )?);
                blocks.extend(assistant_tool_calls_to_anthropic(message, index)?);
                push_message_blocks(&mut messages, "assistant", blocks);
            }
            "tool" => {
                let block = tool_message_to_anthropic(message, index)?;
                push_tool_result_block(&mut messages, block);
            }
            other => {
                return Err(ProxyError::InvalidRequest(format!(
                    "unsupported Chat Completions message role: {other}"
                )));
            }
        }
    }

    drop_empty_messages(&mut messages);
    ensure_leading_user_message(&mut messages);
    if messages.is_empty() {
        return Err(ProxyError::InvalidRequest(
            "cannot convert Chat Completions request: empty messages".to_string(),
        ));
    }

    let mut result = json!({
        "model": model,
        "messages": messages,
        "max_tokens": output_token_limit(object, default_max_tokens)
    });

    if !system_parts.is_empty() {
        result["system"] = json!(system_parts.join("\n\n"));
    }

    for field in ["temperature", "top_p", "stream"] {
        if let Some(value) = object.get(field) {
            result[field] = value.clone();
        }
    }
    if let Some(stop_sequences) = map_stop_sequences(object.get("stop"))? {
        result["stop_sequences"] = stop_sequences;
    }

    if object.get("n").and_then(Value::as_u64).unwrap_or(1) != 1 {
        return Err(ProxyError::InvalidRequest(
            "Anthropic Messages supports exactly one completion (n must be 1)".to_string(),
        ));
    }

    let tools = map_chat_tools(object.get("tools"))?;
    if !tools.is_empty() {
        result["tools"] = json!(tools);
    }

    let mapped_tool_choice = map_chat_tool_choice(object.get("tool_choice"), !tools.is_empty())?;
    let forced_tool = mapped_tool_choice.as_ref().is_some_and(|choice| {
        matches!(
            choice.get("type").and_then(Value::as_str),
            Some("any" | "tool")
        )
    });
    if let Some(choice) = mapped_tool_choice {
        result["tool_choice"] = choice;
    }
    if object.get("parallel_tool_calls").and_then(Value::as_bool) == Some(false)
        && !tools.is_empty()
    {
        if result.get("tool_choice").is_none() {
            result["tool_choice"] = json!({ "type": "auto" });
        }
        result["tool_choice"]["disable_parallel_tool_use"] = json!(true);
    }

    map_reasoning_effort(object, &mut result, forced_tool)?;
    Ok(result)
}

/// Convert a non-streaming Anthropic Messages response into Chat Completions.
pub fn anthropic_response_to_chat_completion(body: Value) -> Result<Value, ProxyError> {
    let object = body.as_object().ok_or_else(|| {
        ProxyError::TransformError("Anthropic response must be a JSON object".to_string())
    })?;
    let content = object
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProxyError::TransformError("Anthropic response has no content array".to_string())
        })?;

    let mut text = String::new();
    let mut reasoning = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, block) in content.iter().enumerate() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        match block_type {
            "text" => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                }
            }
            "thinking" => {
                if let Some(value) = block.get("thinking").and_then(Value::as_str) {
                    if !value.is_empty() {
                        reasoning.push(value.to_string());
                    }
                }
            }
            // There is no safe Chat Completions representation for encrypted
            // thinking. It is intentionally not exposed as visible text.
            "redacted_thinking" => {}
            "tool_use" => {
                let id = required_response_string(block.get("id"), "tool_use.id")?;
                let name = required_response_string(block.get("name"), "tool_use.name")?;
                let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": canonical_json_string(&input)
                    }
                }));
            }
            other if is_hosted_anthropic_block(other) => {
                return Err(ProxyError::InvalidRequest(format!(
                    "unsupported Anthropic hosted-tool response block: {other}"
                )));
            }
            other => {
                return Err(ProxyError::TransformError(format!(
                    "unsupported Anthropic content block at index {index}: {other}"
                )));
            }
        }
    }

    let mut message = Map::new();
    message.insert("role".to_string(), json!("assistant"));
    message.insert(
        "content".to_string(),
        if text.is_empty() {
            Value::Null
        } else {
            json!(text)
        },
    );
    if !reasoning.is_empty() {
        message.insert(
            "reasoning_content".to_string(),
            json!(reasoning.join("\n\n")),
        );
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), json!(tool_calls));
    }

    let stop_reason = object.get("stop_reason").and_then(Value::as_str);
    let finish_reason = map_anthropic_stop_reason_to_chat(stop_reason)
        .or_else(|| (!tool_calls.is_empty()).then_some("tool_calls"));

    Ok(json!({
        "id": chat_completion_id(object.get("id").and_then(Value::as_str)),
        "object": "chat.completion",
        "created": 0,
        "model": object.get("model").and_then(Value::as_str).unwrap_or(""),
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "logprobs": null,
            "finish_reason": finish_reason
        }],
        "usage": chat_usage_from_anthropic(object.get("usage"))
    }))
}

pub(crate) fn map_anthropic_stop_reason_to_chat(stop_reason: Option<&str>) -> Option<&'static str> {
    match stop_reason {
        Some("end_turn" | "stop_sequence" | "pause_turn") => Some("stop"),
        Some("max_tokens" | "model_context_window_exceeded") => Some("length"),
        Some("tool_use") => Some("tool_calls"),
        Some("refusal") => Some("content_filter"),
        Some(other) => {
            log::warn!("[Agent Gateway] Unknown Anthropic stop_reason: {other}");
            Some("stop")
        }
        None => None,
    }
}

pub(crate) fn chat_usage_from_anthropic(usage: Option<&Value>) -> Value {
    let fresh_input = usage
        .and_then(|value| value.get("input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .and_then(|value| value.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = usage
        .and_then(|value| value.get("cache_read_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation = usage
        .and_then(|value| value.get("cache_creation_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let prompt_tokens = fresh_input
        .saturating_add(cache_read)
        .saturating_add(cache_creation);

    let mut result = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": output,
        "total_tokens": prompt_tokens.saturating_add(output)
    });
    if cache_read > 0 || cache_creation > 0 {
        result["prompt_tokens_details"] = json!({
            "cached_tokens": cache_read,
            "cache_write_tokens": cache_creation
        });
    }
    // Compatibility aliases used by CC Switch's usage parser and several
    // OpenAI-compatible clients.
    if cache_read > 0 {
        result["cache_read_input_tokens"] = json!(cache_read);
    }
    if cache_creation > 0 {
        result["cache_creation_input_tokens"] = json!(cache_creation);
    }
    result
}

pub(crate) fn chat_completion_id(id: Option<&str>) -> String {
    let id = id.filter(|value| !value.is_empty()).unwrap_or("ccswitch");
    if id.starts_with("chatcmpl-") || id.starts_with("chatcmpl_") {
        id.to_string()
    } else {
        format!("chatcmpl-{id}")
    }
}

#[derive(Clone, Copy)]
enum ContentRole {
    User,
    Assistant,
    ToolResult,
}

fn required_non_empty_string<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<&'a str, ProxyError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::InvalidRequest(format!("{field} must be a non-empty string")))
}

fn required_response_string<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<&'a str, ProxyError> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::TransformError(format!("{field} must be a non-empty string")))
}

fn system_text_parts(
    content: Option<&Value>,
    message_index: usize,
) -> Result<Vec<String>, ProxyError> {
    match content {
        Some(Value::String(text)) => Ok((!text.is_empty())
            .then(|| text.to_string())
            .into_iter()
            .collect()),
        Some(Value::Array(parts)) => {
            let mut result = Vec::new();
            for (part_index, part) in parts.iter().enumerate() {
                let part_type = part.get("type").and_then(Value::as_str).unwrap_or("text");
                if !matches!(part_type, "text" | "input_text") {
                    return Err(ProxyError::InvalidRequest(format!(
                        "unsupported system/developer content type at messages[{message_index}].content[{part_index}]: {part_type}"
                    )));
                }
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    result.push(text.to_string());
                }
            }
            Ok(result)
        }
        None | Some(Value::Null) => Ok(Vec::new()),
        _ => Err(ProxyError::InvalidRequest(format!(
            "messages[{message_index}].content must be text or an array"
        ))),
    }
}

fn chat_content_to_anthropic_blocks(
    content: Option<&Value>,
    message_index: usize,
    role: ContentRole,
) -> Result<Vec<Value>, ProxyError> {
    match content {
        Some(Value::String(text)) => Ok((!text.is_empty())
            .then(|| json!({ "type": "text", "text": text }))
            .into_iter()
            .collect()),
        Some(Value::Array(parts)) => {
            let mut blocks = Vec::new();
            for (part_index, part) in parts.iter().enumerate() {
                let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
                match part_type {
                    "text" | "input_text" | "output_text" => {
                        if let Some(text) = part
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            blocks.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    "image_url" | "input_image" if !matches!(role, ContentRole::Assistant) => {
                        blocks.push(chat_image_to_anthropic(part, message_index, part_index)?);
                    }
                    "refusal" if matches!(role, ContentRole::Assistant) => {
                        if let Some(text) = part
                            .get("refusal")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            blocks.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    other => {
                        return Err(ProxyError::InvalidRequest(format!(
                            "unsupported content type at messages[{message_index}].content[{part_index}]: {other}"
                        )));
                    }
                }
            }
            Ok(blocks)
        }
        None | Some(Value::Null) => Ok(Vec::new()),
        _ => Err(ProxyError::InvalidRequest(format!(
            "messages[{message_index}].content must be text, null, or an array"
        ))),
    }
}

fn chat_image_to_anthropic(
    part: &Value,
    message_index: usize,
    part_index: usize,
) -> Result<Value, ProxyError> {
    let image_url = part
        .get("image_url")
        .or_else(|| part.get("url"))
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("url").and_then(Value::as_str))
        })
        .ok_or_else(|| {
            ProxyError::InvalidRequest(format!(
                "messages[{message_index}].content[{part_index}] has no image URL"
            ))
        })?;

    if let Some(data_uri) = image_url.strip_prefix("data:") {
        let (metadata, data) = data_uri
            .split_once(',')
            .ok_or_else(|| ProxyError::InvalidRequest("invalid image data URI".to_string()))?;
        let mut metadata_parts = metadata.split(';');
        let media_type = metadata_parts.next().unwrap_or("image/png");
        if !metadata_parts.any(|part| part.eq_ignore_ascii_case("base64")) || data.is_empty() {
            return Err(ProxyError::InvalidRequest(
                "image data URI must contain base64 data".to_string(),
            ));
        }
        return Ok(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data
            }
        }));
    }

    if image_url.starts_with("https://") || image_url.starts_with("http://") {
        return Ok(json!({
            "type": "image",
            "source": { "type": "url", "url": image_url }
        }));
    }

    Err(ProxyError::InvalidRequest(
        "image_url must be an http(s) URL or a base64 data URI".to_string(),
    ))
}

fn assistant_tool_calls_to_anthropic(
    message: &Map<String, Value>,
    message_index: usize,
) -> Result<Vec<Value>, ProxyError> {
    let Some(tool_calls) = message.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let tool_calls = tool_calls.as_array().ok_or_else(|| {
        ProxyError::InvalidRequest(format!(
            "messages[{message_index}].tool_calls must be an array"
        ))
    })?;

    let mut blocks = Vec::new();
    for (tool_index, tool_call) in tool_calls.iter().enumerate() {
        let call_type = tool_call
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("function");
        if call_type != "function" {
            return Err(ProxyError::InvalidRequest(format!(
                "unsupported assistant tool call type: {call_type}"
            )));
        }
        let id = required_non_empty_string(
            tool_call.get("id"),
            &format!("messages[{message_index}].tool_calls[{tool_index}].id"),
        )?;
        let function = tool_call
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProxyError::InvalidRequest(format!(
                    "messages[{message_index}].tool_calls[{tool_index}].function must be an object"
                ))
            })?;
        let name = required_non_empty_string(
            function.get("name"),
            &format!("messages[{message_index}].tool_calls[{tool_index}].function.name"),
        )?;
        let input = parse_tool_arguments(
            function.get("arguments"),
            &format!("messages[{message_index}].tool_calls[{tool_index}].function.arguments"),
        )?;
        blocks.push(json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input
        }));
    }
    Ok(blocks)
}

fn parse_tool_arguments(arguments: Option<&Value>, field: &str) -> Result<Value, ProxyError> {
    let parsed = match arguments {
        Some(Value::String(value)) if value.trim().is_empty() => json!({}),
        Some(Value::String(value)) => serde_json::from_str(value).map_err(|error| {
            ProxyError::InvalidRequest(format!("{field} is not valid JSON: {error}"))
        })?,
        Some(Value::Object(_)) => arguments.cloned().unwrap_or_else(|| json!({})),
        None | Some(Value::Null) => json!({}),
        _ => {
            return Err(ProxyError::InvalidRequest(format!(
                "{field} must be a JSON object or encoded JSON object"
            )));
        }
    };

    if !parsed.is_object() {
        return Err(ProxyError::InvalidRequest(format!(
            "{field} must encode a JSON object"
        )));
    }
    Ok(parsed)
}

fn tool_message_to_anthropic(
    message: &Map<String, Value>,
    message_index: usize,
) -> Result<Value, ProxyError> {
    let tool_use_id = required_non_empty_string(
        message.get("tool_call_id"),
        &format!("messages[{message_index}].tool_call_id"),
    )?;
    let content = message.get("content");
    let converted = match content {
        Some(Value::String(text)) => json!(text),
        None | Some(Value::Null) => json!(""),
        Some(Value::Array(_)) => json!(chat_content_to_anthropic_blocks(
            content,
            message_index,
            ContentRole::ToolResult,
        )?),
        Some(value) => json!(canonical_json_string(value)),
    };

    let mut block = json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": converted
    });
    if let Some(is_error) = message.get("is_error").and_then(Value::as_bool) {
        block["is_error"] = json!(is_error);
    }
    Ok(block)
}

fn push_message_blocks(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some(role) {
            if let Some(content) = last.get_mut("content").and_then(Value::as_array_mut) {
                content.extend(blocks);
                return;
            }
        }
    }
    messages.push(json!({ "role": role, "content": blocks }));
}

fn push_tool_result_block(messages: &mut Vec<Value>, block: Value) {
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some("user") {
            if let Some(content) = last.get_mut("content").and_then(Value::as_array_mut) {
                let insert_at = content
                    .iter()
                    .position(|item| {
                        item.get("type").and_then(Value::as_str) != Some("tool_result")
                    })
                    .unwrap_or(content.len());
                content.insert(insert_at, block);
                return;
            }
        }
    }
    messages.push(json!({ "role": "user", "content": [block] }));
}

fn drop_empty_messages(messages: &mut Vec<Value>) {
    messages.retain(|message| {
        message
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|content| !content.is_empty())
    });
}

fn ensure_leading_user_message(messages: &mut Vec<Value>) {
    if !messages.is_empty()
        && messages
            .first()
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            != Some("user")
    {
        messages.insert(
            0,
            json!({
                "role": "user",
                "content": [{ "type": "text", "text": DEFAULT_CONTINUATION_MESSAGE }]
            }),
        );
    }
}

fn output_token_limit(object: &Map<String, Value>, default_max_tokens: u64) -> u64 {
    object
        .get("max_completion_tokens")
        .or_else(|| object.get("max_tokens"))
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .unwrap_or(default_max_tokens.max(1))
}

fn map_stop_sequences(stop: Option<&Value>) -> Result<Option<Value>, ProxyError> {
    match stop {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(json!([value]))),
        Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
            Ok(Some(json!(values)))
        }
        _ => Err(ProxyError::InvalidRequest(
            "stop must be a string or an array of strings".to_string(),
        )),
    }
}

fn map_chat_tools(tools: Option<&Value>) -> Result<Vec<Value>, ProxyError> {
    let Some(tools) = tools.filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let tools = tools
        .as_array()
        .ok_or_else(|| ProxyError::InvalidRequest("tools must be an array".to_string()))?;

    let mut result = Vec::with_capacity(tools.len());
    for (index, tool) in tools.iter().enumerate() {
        let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or("");
        if tool_type != "function" {
            let label = if tool_type.is_empty() {
                "unknown"
            } else {
                tool_type
            };
            return Err(ProxyError::InvalidRequest(format!(
                "unsupported hosted tool type at tools[{index}]: {label}; Agent Gateway currently supports function tools only"
            )));
        }
        let function = tool
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProxyError::InvalidRequest(format!("tools[{index}].function must be an object"))
            })?;
        let name = required_non_empty_string(
            function.get("name"),
            &format!("tools[{index}].function.name"),
        )?;
        let schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        if !schema.is_object() {
            return Err(ProxyError::InvalidRequest(format!(
                "tools[{index}].function.parameters must be a JSON schema object"
            )));
        }

        let mut mapped = json!({ "name": name, "input_schema": schema });
        if let Some(description) = function.get("description").and_then(Value::as_str) {
            mapped["description"] = json!(description);
        }
        if let Some(strict) = function.get("strict").and_then(Value::as_bool) {
            mapped["strict"] = json!(strict);
        }
        result.push(mapped);
    }
    Ok(result)
}

fn map_chat_tool_choice(
    tool_choice: Option<&Value>,
    has_tools: bool,
) -> Result<Option<Value>, ProxyError> {
    let Some(tool_choice) = tool_choice else {
        return Ok(None);
    };
    let mapped = match tool_choice {
        Value::Null => return Ok(None),
        Value::String(value) => match value.as_str() {
            "auto" => Some(json!({ "type": "auto" })),
            "required" => Some(json!({ "type": "any" })),
            "none" => Some(json!({ "type": "none" })),
            other => {
                return Err(ProxyError::InvalidRequest(format!(
                    "unsupported tool_choice: {other}"
                )));
            }
        },
        Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("function") => {
            let name = object
                .get("function")
                .and_then(|value| value.get("name"))
                .or_else(|| object.get("name"));
            let name = required_non_empty_string(name, "tool_choice.function.name")?;
            Some(json!({ "type": "tool", "name": name }))
        }
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => Some(json!({ "type": "auto" })),
            Some("required" | "any") => Some(json!({ "type": "any" })),
            Some("none") => Some(json!({ "type": "none" })),
            Some(other) => {
                return Err(ProxyError::InvalidRequest(format!(
                    "unsupported hosted tool_choice type: {other}"
                )));
            }
            None => {
                return Err(ProxyError::InvalidRequest(
                    "tool_choice object must contain type".to_string(),
                ));
            }
        },
        _ => {
            return Err(ProxyError::InvalidRequest(
                "tool_choice must be a string or object".to_string(),
            ));
        }
    };

    if !has_tools
        && mapped
            .as_ref()
            .is_some_and(|choice| choice.get("type").and_then(Value::as_str) != Some("none"))
    {
        return Err(ProxyError::InvalidRequest(
            "tool_choice requires at least one function tool".to_string(),
        ));
    }
    Ok(if has_tools { mapped } else { None })
}

fn map_reasoning_effort(
    request: &Map<String, Value>,
    result: &mut Value,
    forced_tool: bool,
) -> Result<(), ProxyError> {
    let effort = request
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .or_else(|| {
            request
                .get("reasoning")
                .and_then(|reasoning| reasoning.get("effort"))
                .and_then(Value::as_str)
        });
    let Some(effort) = effort else {
        return Ok(());
    };

    let mapped = match effort.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" => {
            result["thinking"] = json!({ "type": "disabled" });
            return Ok(());
        }
        "minimal" | "low" => "low",
        "medium" => "medium",
        "high" => "high",
        "xhigh" | "max" => "max",
        other => {
            return Err(ProxyError::InvalidRequest(format!(
                "unsupported reasoning_effort: {other}"
            )));
        }
    };

    // Anthropic rejects forced tool selection while thinking is enabled. Keep
    // the client's explicit tool constraint instead of silently weakening it.
    if forced_tool {
        result["thinking"] = json!({ "type": "disabled" });
        return Ok(());
    }
    result["thinking"] = json!({ "type": "adaptive" });
    result["output_config"] = json!({ "effort": mapped });
    result
        .as_object_mut()
        .expect("result is an object")
        .remove("temperature");
    result
        .as_object_mut()
        .expect("result is an object")
        .remove("top_p");
    Ok(())
}

fn is_hosted_anthropic_block(block_type: &str) -> bool {
    block_type == "server_tool_use"
        || block_type.ends_with("_tool_result")
        || matches!(
            block_type,
            "web_search_result" | "web_fetch_result" | "code_execution_result"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_system_developer_images_tools_and_tool_results() {
        let request = json!({
            "model": "claude-opus-4-8",
            "messages": [
                {"role":"system","content":"System rules"},
                {"role":"developer","content":[{"type":"text","text":"Developer rules"}]},
                {"role":"user","content":[
                    {"type":"text","text":"Inspect this"},
                    {"type":"image_url","image_url":{"url":"data:image/png;base64,abc"}}
                ]},
                {"role":"assistant","reasoning_content":"Need the tool.","content":null,"tool_calls":[{
                    "id":"call_1","type":"function","function":{"name":"inspect","arguments":"{\"path\":\"a.rs\"}"}
                }]},
                {"role":"tool","tool_call_id":"call_1","content":"ok"}
            ],
            "tools":[{"type":"function","function":{
                "name":"inspect","description":"Inspect a file","parameters":{"type":"object"},"strict":true
            }}],
            "tool_choice":{"type":"function","function":{"name":"inspect"}},
            "parallel_tool_calls":false,
            "max_completion_tokens":4096,
            "stream":true
        });

        let result = chat_completions_request_to_anthropic(request, 8192).unwrap();
        assert_eq!(result["system"], "System rules\n\nDeveloper rules");
        assert_eq!(result["max_tokens"], 4096);
        assert_eq!(result["stream"], true);
        assert_eq!(
            result["messages"][0]["content"][1]["source"]["type"],
            "base64"
        );
        assert_eq!(
            result["messages"][1]["content"][0]["text"],
            "Need the tool."
        );
        assert_eq!(result["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(result["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(result["messages"][2]["content"][0]["tool_use_id"], "call_1");
        assert_eq!(result["tools"][0]["name"], "inspect");
        assert_eq!(result["tools"][0]["strict"], true);
        assert_eq!(result["tool_choice"]["type"], "tool");
        assert_eq!(result["tool_choice"]["disable_parallel_tool_use"], true);
    }

    #[test]
    fn converts_remote_image_and_defaults_max_tokens() {
        let result = chat_completions_request_to_anthropic(
            json!({
                "model":"claude-fable-5",
                "messages":[{"role":"user","content":[{
                    "type":"image_url","image_url":"https://example.com/a.png"
                }]}]
            }),
            1234,
        )
        .unwrap();

        assert_eq!(result["max_tokens"], 1234);
        assert_eq!(result["messages"][0]["content"][0]["source"]["type"], "url");
    }

    #[test]
    fn rejects_hosted_tools_instead_of_silently_dropping_them() {
        let error = chat_completions_request_to_anthropic(
            json!({
                "model":"claude-opus-4-8",
                "messages":[{"role":"user","content":"hi"}],
                "tools":[{"type":"web_search"}]
            }),
            1024,
        )
        .unwrap_err();
        assert!(matches!(error, ProxyError::InvalidRequest(_)));
        assert!(error.to_string().contains("hosted tool"));
    }

    #[test]
    fn rejects_invalid_function_arguments() {
        let error = chat_completions_request_to_anthropic(json!({
            "model":"claude-opus-4-8",
            "messages":[
                {"role":"user","content":"hi"},
                {"role":"assistant","tool_calls":[{
                    "id":"call_1","type":"function","function":{"name":"x","arguments":"not-json"}
                }]}
            ]
        }), 1024).unwrap_err();
        assert!(matches!(error, ProxyError::InvalidRequest(_)));
    }

    #[test]
    fn maps_reasoning_effort_and_forced_tool_safely() {
        let adaptive = chat_completions_request_to_anthropic(
            json!({
                "model":"claude-opus-4-8",
                "messages":[{"role":"user","content":"hi"}],
                "reasoning_effort":"xhigh",
                "temperature":0.2
            }),
            1024,
        )
        .unwrap();
        assert_eq!(adaptive["thinking"]["type"], "adaptive");
        assert_eq!(adaptive["output_config"]["effort"], "max");
        assert!(adaptive.get("temperature").is_none());

        let forced = chat_completions_request_to_anthropic(json!({
            "model":"claude-opus-4-8",
            "messages":[{"role":"user","content":"hi"}],
            "reasoning_effort":"high",
            "tools":[{"type":"function","function":{"name":"x","parameters":{"type":"object"}}}],
            "tool_choice":"required"
        }), 1024).unwrap();
        assert_eq!(forced["thinking"]["type"], "disabled");
        assert_eq!(forced["tool_choice"]["type"], "any");
    }

    #[test]
    fn converts_anthropic_response_with_reasoning_tool_calls_and_cache_usage() {
        let result = anthropic_response_to_chat_completion(json!({
            "id":"msg_123",
            "type":"message",
            "model":"claude-opus-4-8",
            "content":[
                {"type":"thinking","thinking":"Need weather","signature":"sig"},
                {"type":"text","text":"Checking."},
                {"type":"tool_use","id":"call_1","name":"weather","input":{"city":"Tokyo"}}
            ],
            "stop_reason":"tool_use",
            "usage":{
                "input_tokens":10,"cache_read_input_tokens":20,
                "cache_creation_input_tokens":3,"output_tokens":5
            }
        }))
        .unwrap();

        assert_eq!(result["id"], "chatcmpl-msg_123");
        assert_eq!(result["choices"][0]["message"]["content"], "Checking.");
        assert_eq!(
            result["choices"][0]["message"]["reasoning_content"],
            "Need weather"
        );
        assert_eq!(
            result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"Tokyo\"}"
        );
        assert_eq!(result["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(result["usage"]["prompt_tokens"], 33);
        assert_eq!(result["usage"]["total_tokens"], 38);
        assert_eq!(
            result["usage"]["prompt_tokens_details"]["cached_tokens"],
            20
        );
    }

    #[test]
    fn maps_finish_reasons() {
        assert_eq!(
            map_anthropic_stop_reason_to_chat(Some("end_turn")),
            Some("stop")
        );
        assert_eq!(
            map_anthropic_stop_reason_to_chat(Some("max_tokens")),
            Some("length")
        );
        assert_eq!(
            map_anthropic_stop_reason_to_chat(Some("tool_use")),
            Some("tool_calls")
        );
        assert_eq!(
            map_anthropic_stop_reason_to_chat(Some("refusal")),
            Some("content_filter")
        );
    }

    #[test]
    fn rejects_hosted_tool_response_blocks() {
        let error = anthropic_response_to_chat_completion(json!({
            "content":[{"type":"server_tool_use","id":"srv_1","name":"web_search"}]
        }))
        .unwrap_err();
        assert!(matches!(error, ProxyError::InvalidRequest(_)));
    }
}
