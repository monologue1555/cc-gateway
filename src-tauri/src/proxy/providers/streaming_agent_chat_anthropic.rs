//! Anthropic Messages SSE → OpenAI Chat Completions SSE conversion.
//!
//! The Agent Gateway uses this module when a generic OpenAI-compatible agent
//! streams from an Anthropic-native provider.  It keeps text, reasoning,
//! function-call argument deltas, finish reasons, and cache-aware usage intact.

use super::transform_agent_chat_anthropic::{
    chat_completion_id, chat_usage_from_anthropic, map_anthropic_stop_reason_to_chat,
};
use crate::proxy::{
    error::ProxyError,
    json_canonical::canonical_json_string,
    sse::{append_utf8_safe, strip_sse_field, take_sse_block},
};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
enum ContentBlockKind {
    Text,
    Thinking,
    Tool { chat_index: u64 },
}

#[derive(Debug)]
struct AnthropicToChatState {
    id: String,
    model: String,
    created: u64,
    role_sent: bool,
    finish_sent: bool,
    usage_sent: bool,
    done: bool,
    include_usage: bool,
    next_tool_index: u64,
    blocks: BTreeMap<u64, ContentBlockKind>,
    usage: Map<String, Value>,
    stop_reason: Option<String>,
}

impl AnthropicToChatState {
    fn new(include_usage: bool) -> Self {
        Self {
            id: chat_completion_id(None),
            model: String::new(),
            created: 0,
            role_sent: false,
            finish_sent: false,
            usage_sent: false,
            done: false,
            include_usage,
            next_tool_index: 0,
            blocks: BTreeMap::new(),
            usage: Map::new(),
            stop_reason: None,
        }
    }

    fn merge_usage(&mut self, usage: &Value) {
        if let Some(object) = usage.as_object() {
            for (key, value) in object {
                if !value.is_null() {
                    self.usage.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn chat_chunk(&self, choices: Value, usage: Option<Value>) -> Bytes {
        let mut chunk = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": choices
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        sse_json(&chunk)
    }

    fn delta_chunk(&self, delta: Value) -> Bytes {
        self.chat_chunk(
            json!([{
                "index": 0,
                "delta": delta,
                "logprobs": null,
                "finish_reason": null
            }]),
            None,
        )
    }

    fn ensure_role(&mut self) -> Vec<Bytes> {
        if self.role_sent {
            return Vec::new();
        }
        self.role_sent = true;
        vec![self.delta_chunk(json!({ "role": "assistant", "content": "" }))]
    }

    fn handle_message_start(&mut self, event: &Value) -> Vec<Bytes> {
        if let Some(message) = event.get("message") {
            self.id = chat_completion_id(message.get("id").and_then(Value::as_str));
            if let Some(model) = message.get("model").and_then(Value::as_str) {
                self.model = model.to_string();
            }
            if let Some(usage) = message.get("usage") {
                self.merge_usage(usage);
            }
        }
        self.ensure_role()
    }

    fn handle_content_block_start(&mut self, event: &Value) -> Result<Vec<Bytes>, ProxyError> {
        let mut output = self.ensure_role();
        let index = event.get("index").and_then(Value::as_u64).ok_or_else(|| {
            ProxyError::TransformError("Anthropic content_block_start has no index".to_string())
        })?;
        let block = event.get("content_block").ok_or_else(|| {
            ProxyError::TransformError(
                "Anthropic content_block_start has no content_block".to_string(),
            )
        })?;
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");

        match block_type {
            "text" => {
                self.blocks.insert(index, ContentBlockKind::Text);
                if let Some(text) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    output.push(self.delta_chunk(json!({ "content": text })));
                }
            }
            "thinking" => {
                self.blocks.insert(index, ContentBlockKind::Thinking);
                if let Some(thinking) = block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    output.push(self.delta_chunk(json!({ "reasoning_content": thinking })));
                }
            }
            "redacted_thinking" => {
                // Encrypted thinking has no safe Chat Completions representation.
                self.blocks.insert(index, ContentBlockKind::Thinking);
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ProxyError::TransformError("Anthropic tool_use block has no id".to_string())
                    })?;
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        ProxyError::TransformError(
                            "Anthropic tool_use block has no name".to_string(),
                        )
                    })?;
                let chat_index = self.next_tool_index;
                self.next_tool_index += 1;
                self.blocks
                    .insert(index, ContentBlockKind::Tool { chat_index });

                let initial_arguments = block
                    .get("input")
                    .and_then(Value::as_object)
                    .filter(|input| !input.is_empty())
                    .map(|input| canonical_json_string(&Value::Object(input.clone())))
                    .unwrap_or_default();
                output.push(self.delta_chunk(json!({
                    "tool_calls": [{
                        "index": chat_index,
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": initial_arguments }
                    }]
                })));
            }
            other if is_hosted_anthropic_block(other) => {
                return Err(ProxyError::InvalidRequest(format!(
                    "unsupported Anthropic hosted-tool stream block: {other}"
                )));
            }
            other => {
                return Err(ProxyError::TransformError(format!(
                    "unsupported Anthropic stream content block: {other}"
                )));
            }
        }
        Ok(output)
    }

    fn handle_content_block_delta(&mut self, event: &Value) -> Result<Vec<Bytes>, ProxyError> {
        let mut output = self.ensure_role();
        let index = event.get("index").and_then(Value::as_u64).ok_or_else(|| {
            ProxyError::TransformError("Anthropic content_block_delta has no index".to_string())
        })?;
        let delta = event.get("delta").ok_or_else(|| {
            ProxyError::TransformError("Anthropic content_block_delta has no delta".to_string())
        })?;
        let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or("");
        let block_kind = self.blocks.get(&index).copied();

        match delta_type {
            "text_delta" => {
                if !matches!(block_kind, Some(ContentBlockKind::Text)) {
                    return Err(ProxyError::TransformError(format!(
                        "text_delta does not match content block {index}"
                    )));
                }
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    output.push(self.delta_chunk(json!({ "content": text })));
                }
            }
            "thinking_delta" => {
                if !matches!(block_kind, Some(ContentBlockKind::Thinking)) {
                    return Err(ProxyError::TransformError(format!(
                        "thinking_delta does not match content block {index}"
                    )));
                }
                if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                    output.push(self.delta_chunk(json!({ "reasoning_content": thinking })));
                }
            }
            "input_json_delta" => {
                let Some(ContentBlockKind::Tool { chat_index }) = block_kind else {
                    return Err(ProxyError::TransformError(format!(
                        "input_json_delta does not match tool block {index}"
                    )));
                };
                if let Some(arguments) = delta.get("partial_json").and_then(Value::as_str) {
                    output.push(self.delta_chunk(json!({
                        "tool_calls": [{
                            "index": chat_index,
                            "function": { "arguments": arguments }
                        }]
                    })));
                }
            }
            // Signatures are provider-private data and cannot be represented in
            // standard Chat Completions chunks.
            "signature_delta" => {}
            other => {
                return Err(ProxyError::TransformError(format!(
                    "unsupported Anthropic stream delta: {other}"
                )));
            }
        }
        Ok(output)
    }

    fn handle_message_delta(&mut self, event: &Value) -> Vec<Bytes> {
        if let Some(usage) = event.get("usage") {
            self.merge_usage(usage);
        }
        if let Some(stop_reason) = event
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            self.stop_reason = Some(stop_reason.to_string());
        }
        self.emit_finish()
    }

    fn emit_finish(&mut self) -> Vec<Bytes> {
        let mut output = self.ensure_role();
        if self.finish_sent {
            return output;
        }
        self.finish_sent = true;
        let finish_reason =
            map_anthropic_stop_reason_to_chat(self.stop_reason.as_deref()).unwrap_or("stop");
        output.push(self.chat_chunk(
            json!([{
                "index": 0,
                "delta": {},
                "logprobs": null,
                "finish_reason": finish_reason
            }]),
            None,
        ));
        output
    }

    fn emit_usage(&mut self) -> Vec<Bytes> {
        if !self.include_usage || self.usage_sent {
            return Vec::new();
        }
        self.usage_sent = true;
        let usage = chat_usage_from_anthropic(Some(&Value::Object(self.usage.clone())));
        vec![self.chat_chunk(json!([]), Some(usage))]
    }

    fn finalize(&mut self) -> Vec<Bytes> {
        if self.done {
            return Vec::new();
        }
        let mut output = self.emit_finish();
        output.extend(self.emit_usage());
        output.push(Bytes::from_static(b"data: [DONE]\n\n"));
        self.done = true;
        output
    }

    fn handle_error(&mut self, event: &Value) -> Vec<Bytes> {
        if self.done {
            return Vec::new();
        }
        let error = event.get("error").unwrap_or(event);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Anthropic streaming error");
        let error_type = error
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("upstream_error");
        self.done = true;
        vec![
            sse_json(&json!({
                "error": {
                    "message": message,
                    "type": error_type,
                    "param": null,
                    "code": error_type
                }
            })),
            Bytes::from_static(b"data: [DONE]\n\n"),
        ]
    }

    fn handle_event(&mut self, event: &Value) -> Result<Vec<Bytes>, ProxyError> {
        if self.done {
            return Ok(Vec::new());
        }
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => Ok(self.handle_message_start(event)),
            "content_block_start" => self.handle_content_block_start(event),
            "content_block_delta" => self.handle_content_block_delta(event),
            "content_block_stop" | "ping" => Ok(Vec::new()),
            "message_delta" => Ok(self.handle_message_delta(event)),
            "message_stop" => Ok(self.finalize()),
            "error" => Ok(self.handle_error(event)),
            other => Err(ProxyError::TransformError(format!(
                "unsupported Anthropic SSE event: {other}"
            ))),
        }
    }
}

/// Convert an Anthropic Messages SSE byte stream into Chat Completions SSE.
///
/// When `include_usage` is true, an OpenAI-compatible usage-only chunk with an
/// empty `choices` array is emitted immediately before `[DONE]`.
pub fn create_chat_completions_sse_stream<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    include_usage: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut state = AnthropicToChatState::new(include_usage);
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            };
            append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

            while let Some(block) = take_sse_block(&mut buffer) {
                match convert_sse_block(&block, &mut state) {
                    Ok(events) => {
                        for event in events {
                            yield Ok(event);
                        }
                    }
                    Err(error) => {
                        yield Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()));
                        return;
                    }
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        if !buffer.trim().is_empty() {
            match convert_sse_block(&buffer, &mut state) {
                Ok(events) => {
                    for event in events {
                        yield Ok(event);
                    }
                }
                Err(error) => {
                    yield Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()));
                    return;
                }
            }
        }
        for event in state.finalize() {
            yield Ok(event);
        }
    }
}

fn convert_sse_block(
    block: &str,
    state: &mut AnthropicToChatState,
) -> Result<Vec<Bytes>, ProxyError> {
    let data = block
        .lines()
        .filter_map(|line| strip_sse_field(line, "data"))
        .collect::<Vec<_>>()
        .join("\n");
    if data.trim().is_empty() {
        return Ok(Vec::new());
    }
    if data.trim() == "[DONE]" {
        return Ok(state.finalize());
    }
    let event: Value = serde_json::from_str(&data).map_err(|error| {
        ProxyError::TransformError(format!("invalid Anthropic SSE JSON: {error}"))
    })?;
    state.handle_event(&event)
}

fn sse_json(value: &Value) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
    ))
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
    use futures::{stream, TryStreamExt};

    async fn convert(
        parts: Vec<&'static str>,
        include_usage: bool,
    ) -> Result<String, std::io::Error> {
        let input = stream::iter(
            parts
                .into_iter()
                .map(|part| Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(part.as_bytes()))),
        );
        let bytes: Vec<Bytes> = create_chat_completions_sse_stream(input, include_usage)
            .try_collect()
            .await?;
        Ok(bytes
            .iter()
            .map(|bytes| String::from_utf8_lossy(bytes))
            .collect::<String>())
    }

    #[tokio::test]
    async fn converts_text_reasoning_tools_finish_and_usage() {
        let output = convert(vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-opus-4-8\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":5}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Plan.\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking.\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"weather\",\"input\":{}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\\\"Tokyo\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ], true).await.unwrap();

        assert!(output.contains("\"id\":\"chatcmpl-msg_1\""));
        assert!(output.contains("\"reasoning_content\":\"Plan.\""));
        assert!(output.contains("\"content\":\"Checking.\""));
        assert!(output.contains("\"id\":\"call_1\""));
        assert!(output.contains("\"name\":\"weather\""));
        assert!(output.contains("\\\"city\\\":\\\"Tokyo\\\""));
        assert!(output.contains("\"finish_reason\":\"tool_calls\""));
        assert!(output.contains("\"prompt_tokens\":15"));
        assert!(output.contains("\"completion_tokens\":7"));
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn handles_fragmented_utf8_and_missing_message_stop() {
        let event = "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你好\"}}\n\n";
        let bytes = event.as_bytes();
        let split = event.find('好').unwrap() + 1;
        let first = Bytes::copy_from_slice(&bytes[..split]);
        let second = Bytes::copy_from_slice(&bytes[split..]);
        let input = stream::iter(vec![
            Ok::<Bytes, std::io::Error>(first),
            Ok::<Bytes, std::io::Error>(second),
        ]);
        let output = create_chat_completions_sse_stream(input, false)
            .try_collect::<Vec<Bytes>>()
            .await
            .unwrap()
            .iter()
            .map(|part| String::from_utf8_lossy(part))
            .collect::<String>();
        assert!(output.contains("你好"));
        assert!(output.contains("\"finish_reason\":\"stop\""));
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn maps_anthropic_error_event_to_openai_error() {
        let output = convert(vec![
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"busy\"}}\n\n"
        ], false).await.unwrap();
        assert!(output.contains("\"message\":\"busy\""));
        assert!(output.contains("\"code\":\"overloaded_error\""));
        assert!(output.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn rejects_hosted_tool_blocks() {
        let error = convert(vec![
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"srv_1\",\"name\":\"web_search\"}}\n\n"
        ], false).await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("hosted-tool"));
    }
}
