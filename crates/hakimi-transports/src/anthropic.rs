use async_trait::async_trait;
use hakimi_common::{
    ApiMode, FinishReason, HakimiError, Message, MessageRole, NormalizedResponse, Result, ToolCall,
    ToolDefinition, Usage,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use tracing::{debug, warn};

use crate::error::classify_error;
use crate::params::RequestParams;
use crate::prompt_caching::{CACHE_BETA_HEADER_VALUE, CacheLayout, apply_caching};
use crate::streaming::{SseEventStream, StreamEvent};
use crate::trait_def::ProviderTransport;
use futures::stream::Stream;
use std::pin::Pin;

/// An Anthropic Messages API transport.
///
/// Implements the Anthropic-specific wire format for `/v1/messages`,
/// which differs from OpenAI's Chat Completions in several ways:
/// - System prompt is a top-level field, not a message role
/// - Only `user` and `assistant` roles are valid in the messages array
/// - Tool calls are inline content blocks (`type: "tool_use"`)
/// - Tool results are sent as `user` messages with `type: "tool_result"` content blocks
pub struct AnthropicTransport {
    base_url: String,
    api_key: String,
    client: Client,
    enable_caching: bool,
    cache_layout: CacheLayout,
}

impl AnthropicTransport {
    pub fn new(base_url: String, api_key: String, client: Client) -> Self {
        Self {
            base_url,
            api_key,
            client,
            enable_caching: false,
            cache_layout: CacheLayout::SystemAnd3,
        }
    }

    /// Enable prompt caching with the given layout strategy.
    pub fn with_caching(mut self, layout: CacheLayout) -> Self {
        self.enable_caching = true;
        self.cache_layout = layout;
        self
    }

    /// Returns `true` if prompt caching is enabled.
    pub fn is_caching_enabled(&self) -> bool {
        self.enable_caching
    }

    /// Returns the `anthropic-beta` header value if caching is enabled.
    pub fn beta_header(&self) -> Option<&'static str> {
        if self.enable_caching {
            Some(CACHE_BETA_HEADER_VALUE)
        } else {
            None
        }
    }

    /// Build the full request URL.
    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}/messages", base)
        } else {
            format!("{}/v1/messages", base)
        }
    }

    /// Extract the system prompt from messages (if present) and return the remaining messages.
    fn split_system_prompt(messages: &[Message]) -> (Option<String>, Vec<&Message>) {
        let mut system: Option<String> = None;
        let mut rest = Vec::with_capacity(messages.len());

        for msg in messages {
            if msg.role == MessageRole::System {
                // Concatenate multiple system messages if present.
                match system {
                    Some(ref mut existing) => {
                        existing.push_str("\n\n");
                        existing.push_str(msg.content.as_deref().unwrap_or(""));
                    }
                    None => {
                        system = Some(msg.content.as_deref().unwrap_or("").to_string());
                    }
                }
            } else {
                rest.push(msg);
            }
        }

        (system, rest)
    }

    /// Convert our internal [`Message`] slice into Anthropic JSON wire format.
    ///
    /// Key transformations:
    /// - `System` messages are excluded (handled by `split_system_prompt`).
    /// - `Tool` messages become `{"role": "user", "content": [{"type": "tool_result", ...}]}`.
    /// - `Assistant` messages with `tool_calls` emit content blocks with `type: "tool_use"`.
    /// - Adjacent messages with the same role are merged as required by Anthropic.
    fn convert_messages(messages: &[&Message]) -> Vec<JsonValue> {
        let mut result: Vec<JsonValue> = Vec::new();

        for msg in messages {
            match msg.role {
                MessageRole::System => {
                    // Should have been filtered out already, skip.
                    continue;
                }
                MessageRole::User => {
                    let mut content_blocks: Vec<JsonValue> = Vec::new();
                    if let Some(ref text) = msg.content {
                        if !text.is_empty() {
                            content_blocks.push(json!({
                                "type": "text",
                                "text": text
                            }));
                        }
                    }
                    if let Some(ref images) = msg.images {
                        for img in images {
                            content_blocks.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": img.mime_type,
                                    "data": img.data
                                }
                            }));
                        }
                    }
                    if content_blocks.is_empty() {
                        content_blocks.push(json!({
                            "type": "text",
                            "text": ""
                        }));
                    }
                    let obj = json!({
                        "role": "user",
                        "content": content_blocks
                    });
                    result.push(obj);
                }
                MessageRole::Assistant => {
                    let mut content_blocks: Vec<JsonValue> = Vec::new();

                    // Text content block (if any).
                    if let Some(ref text) = msg.content
                        && !text.is_empty()
                    {
                        content_blocks.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }

                    // Tool use blocks from tool_calls.
                    if let Some(ref tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            // Anthropic expects the input as a JSON object, not a string.
                            let input: JsonValue =
                                serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
                            content_blocks.push(json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": input
                            }));
                        }
                    }

                    if content_blocks.is_empty() {
                        // Empty assistant message — emit with minimal content.
                        content_blocks.push(json!({
                            "type": "text",
                            "text": ""
                        }));
                    }

                    let obj = json!({
                        "role": "assistant",
                        "content": content_blocks
                    });
                    result.push(obj);
                }
                MessageRole::Tool => {
                    // Tool results in Anthropic are sent as a user message with
                    // tool_result content blocks.
                    let tool_use_id = msg.tool_call_id.as_deref().unwrap_or("");
                    let content_text = msg.content.as_deref().unwrap_or("");

                    let tool_result_block = json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content_text
                    });

                    let obj = json!({
                        "role": "user",
                        "content": [tool_result_block]
                    });
                    result.push(obj);
                }
            }
        }

        // Anthropic requires that no two adjacent messages have the same role.
        // Merge consecutive user or assistant messages.
        merge_adjacent_same_role(result)
    }

    /// Convert our internal [`ToolDefinition`] slice into Anthropic tool format.
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<JsonValue> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect()
    }

    /// Build the full JSON request body.
    fn build_request_base(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
        stream: bool,
    ) -> JsonValue {
        let (system, remaining) = Self::split_system_prompt(messages);
        let anthropic_messages = Self::convert_messages(&remaining);

        let max_tokens = params.max_tokens.unwrap_or(8192);

        let mut body = json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": anthropic_messages,
        });

        if let Some(ref sys) = system {
            body["system"] = json!(sys);
        }

        if !tools.is_empty() {
            body["tools"] = json!(Self::convert_tools(tools));
        }

        if let Some(temp) = params.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(top_p) = params.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(ref stop) = params.stop {
            body["stop_sequences"] = json!(stop);
        }

        if stream {
            body["stream"] = json!(true);
        }

        if self.enable_caching {
            apply_caching(&mut body, self.cache_layout);
        }

        body
    }

    /// Build the full JSON request body (non-streaming).
    fn build_request(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> JsonValue {
        self.build_request_base(model, messages, tools, params, false)
    }

    /// Parse an Anthropic Messages response into a [`NormalizedResponse`].
    fn parse_response(resp: &AnthropicResponse) -> Result<NormalizedResponse> {
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in &resp.content {
            match block.block_type.as_str() {
                "text" => {
                    if let Some(ref text) = block.text {
                        text_parts.push(text.clone());
                    }
                }
                "tool_use" => {
                    let id = block.id.clone().unwrap_or_default();
                    let name = block.name.clone().unwrap_or_default();
                    let input = block.input.clone().unwrap_or(JsonValue::Null);
                    let arguments =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                        index: None,
                    });
                }
                other => {
                    warn!(
                        block_type = other,
                        "unexpected content block type from Anthropic"
                    );
                }
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        let finish_reason = match resp.stop_reason.as_deref() {
            Some("end_turn") => Some(FinishReason::Stop),
            Some("tool_use") => Some(FinishReason::ToolCalls),
            Some("max_tokens") => Some(FinishReason::Length),
            Some("stop_sequence") => Some(FinishReason::Stop),
            _ => None,
        };

        let usage = Some(Usage {
            prompt_tokens: resp.usage.input_tokens,
            completion_tokens: resp.usage.output_tokens,
            total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
            cached_tokens: resp.usage.cache_read_input_tokens.unwrap_or(0),
            reasoning_tokens: resp.usage.cache_creation_input_tokens.unwrap_or(0),
        });

        Ok(NormalizedResponse {
            content,
            tool_calls,
            finish_reason,
            usage,
            reasoning: None,
        })
    }

    /// Execute a streaming Anthropic Messages request.
    ///
    /// Returns a `Stream` of [`StreamEvent`]s as they arrive from the provider.
    /// The stream ends when the provider sends a `message_stop` event or closes the connection.
    pub async fn execute_streaming(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> crate::error::TransportResult<
        Pin<Box<dyn Stream<Item = std::result::Result<StreamEvent, String>> + Send>>,
    > {
        let body = self.build_request_base(model, messages, tools, params, true);
        let url = self.endpoint();

        debug!(url = %url, model = model, "sending streaming Anthropic messages request");

        let mut request = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json");

        if self.enable_caching {
            request = request.header("anthropic-beta", CACHE_BETA_HEADER_VALUE);
        }

        let response = request.json(&body).send().await.map_err(|e| {
            warn!(error = %e, "HTTP streaming request failed");
            crate::error::TransportError::Http(format!("HTTP request failed: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            let response_text = response.text().await.unwrap_or_default();
            let code = status.as_u16();
            let (reason, retryable) = classify_error(code, &response_text);
            warn!(
                status = code,
                ?reason,
                retryable,
                body = %response_text,
                "Anthropic API returned error for streaming request"
            );
            return Err(crate::error::TransportError::Api {
                status: code,
                reason: format!("{reason:?}"),
                retryable,
                body: response_text,
            });
        }

        let byte_stream = response.bytes_stream();
        let sse_stream = SseEventStream::anthropic(Box::pin(byte_stream));
        Ok(Box::pin(sse_stream))
    }
}

/// Merge adjacent messages with the same role, as required by the Anthropic API.
///
/// When consecutive messages share a role, their content is concatenated.
/// For assistant messages with structured content blocks, the blocks are preserved.
fn merge_adjacent_same_role(messages: Vec<JsonValue>) -> Vec<JsonValue> {
    if messages.is_empty() {
        return messages;
    }

    let mut merged: Vec<JsonValue> = Vec::with_capacity(messages.len());

    for msg in messages {
        let should_merge = if let Some(last) = merged.last() {
            last["role"] == msg["role"]
        } else {
            false
        };

        if should_merge {
            let last = merged.last_mut().unwrap();
            // Both messages have the same role. We need to merge content.
            // Convert both to arrays of content blocks and concatenate.
            let existing_content = last["content"].clone();
            let new_content = msg["content"].clone();

            let existing_blocks = content_to_blocks(existing_content);
            let new_blocks = content_to_blocks(new_content);

            let mut combined = existing_blocks;
            combined.extend(new_blocks);
            last["content"] = JsonValue::Array(combined);
        } else {
            merged.push(msg);
        }
    }

    merged
}

/// Normalize content to a vec of content blocks.
fn content_to_blocks(content: JsonValue) -> Vec<JsonValue> {
    match content {
        JsonValue::String(s) => {
            vec![json!({"type": "text", "text": s})]
        }
        JsonValue::Array(arr) => arr,
        other => {
            vec![json!({"type": "text", "text": other.to_string()})]
        }
    }
}

#[async_trait]
impl ProviderTransport for AnthropicTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::AnthropicMessages
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }

    async fn execute(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> Result<NormalizedResponse> {
        let body = self.build_request(model, messages, tools, params);
        let url = self.endpoint();

        debug!(url = %url, model = model, "sending Anthropic messages request");

        let mut request = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json");

        if self.enable_caching {
            request = request.header("anthropic-beta", CACHE_BETA_HEADER_VALUE);
        }

        let response = request.json(&body).send().await.map_err(|e| {
            warn!(error = %e, "HTTP request failed");
            HakimiError::Transport(format!("HTTP request failed: {e}"))
        })?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|e| HakimiError::Transport(format!("failed to read response body: {e}")))?;

        if !status.is_success() {
            let code = status.as_u16();
            let (reason, retryable) = classify_error(code, &response_text);
            warn!(
                status = code,
                ?reason,
                retryable,
                body = %response_text,
                "Anthropic API returned error"
            );
            return Err(HakimiError::Transport(format!(
                "API error {code} ({reason:?}, retryable={retryable}): {response_text}"
            )));
        }

        let parsed: AnthropicResponse = serde_json::from_str(&response_text).map_err(|e| {
            warn!(error = %e, "failed to parse Anthropic response JSON");
            HakimiError::Transport(format!("failed to parse response: {e}"))
        })?;

        Self::parse_response(&parsed)
    }

    async fn execute_streaming(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamEvent, String>> + Send>>> {
        AnthropicTransport::execute_streaming(self, model, messages, tools, params)
            .await
            .map_err(|e| HakimiError::Transport(e.to_string()))
    }
}

// ── Wire-format types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_system_prompt_none() {
        let messages = [Message::user("hello"), Message::assistant("hi")];
        let (system, remaining) = AnthropicTransport::split_system_prompt(&messages);
        assert!(system.is_none());
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn test_split_system_prompt_single() {
        let messages = vec![Message::system("You are helpful."), Message::user("hello")];
        let (system, remaining) = AnthropicTransport::split_system_prompt(&messages);
        assert_eq!(system.as_deref(), Some("You are helpful."));
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        }];

        let result = AnthropicTransport::convert_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["name"], "read_file");
        assert_eq!(result[0]["description"], "Read a file");
        assert_eq!(result[0]["input_schema"]["type"], "object");
    }

    #[test]
    fn test_convert_messages_user_assistant() {
        let messages = [Message::user("hello"), Message::assistant("hi")];
        let refs: Vec<&Message> = messages.iter().collect();
        let result = AnthropicTransport::convert_messages(&refs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["content"], "hello");
        assert_eq!(result[1]["role"], "assistant");
        // Assistant content should be an array of blocks
        assert!(result[1]["content"].is_array());
    }

    #[test]
    fn test_convert_messages_tool_result() {
        let msg = Message::tool_result("toolu_123", "read_file", "file contents");
        let messages = [msg];
        let refs: Vec<&Message> = messages.iter().collect();
        let result = AnthropicTransport::convert_messages(&refs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        let content = result[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "toolu_123");
        assert_eq!(content[0]["content"], "file contents");
    }

    #[test]
    fn test_parse_response_text_only() {
        let resp = AnthropicResponse {
            content: vec![AnthropicContentBlock {
                block_type: "text".to_string(),
                text: Some("Hello!".to_string()),
                id: None,
                name: None,
                input: None,
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            },
        };

        let result = AnthropicTransport::parse_response(&resp).unwrap();
        assert_eq!(result.content.as_deref(), Some("Hello!"));
        assert!(result.tool_calls.is_none());
        assert_eq!(result.finish_reason, Some(FinishReason::Stop));
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_parse_response_tool_use() {
        let resp = AnthropicResponse {
            content: vec![
                AnthropicContentBlock {
                    block_type: "text".to_string(),
                    text: Some("Let me check that.".to_string()),
                    id: None,
                    name: None,
                    input: None,
                },
                AnthropicContentBlock {
                    block_type: "tool_use".to_string(),
                    text: None,
                    id: Some("toolu_abc".to_string()),
                    name: Some("read_file".to_string()),
                    input: Some(json!({"path": "/tmp/test.txt"})),
                },
            ],
            stop_reason: Some("tool_use".to_string()),
            usage: AnthropicUsage {
                input_tokens: 50,
                output_tokens: 30,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            },
        };

        let result = AnthropicTransport::parse_response(&resp).unwrap();
        assert_eq!(result.content.as_deref(), Some("Let me check that."));
        let calls = result.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "toolu_abc");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(result.finish_reason, Some(FinishReason::ToolCalls));
    }

    #[test]
    fn test_merge_adjacent_same_role() {
        let messages = vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "user", "content": "world"}),
            json!({"role": "assistant", "content": "hi"}),
        ];
        let merged = merge_adjacent_same_role(messages);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["role"], "user");
        let content = merged[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
    }

    // ── Caching integration tests ───────────────────────────────────────

    #[test]
    fn test_beta_header_included_when_caching_enabled() {
        let transport = AnthropicTransport::new(
            "https://api.anthropic.com".to_string(),
            "test-key".to_string(),
            reqwest::Client::new(),
        )
        .with_caching(CacheLayout::SystemAnd3);

        assert!(transport.is_caching_enabled());
        assert_eq!(transport.beta_header(), Some(CACHE_BETA_HEADER_VALUE));
    }

    #[test]
    fn test_beta_header_excluded_when_caching_disabled() {
        let transport = AnthropicTransport::new(
            "https://api.anthropic.com".to_string(),
            "test-key".to_string(),
            reqwest::Client::new(),
        );

        assert!(!transport.is_caching_enabled());
        assert_eq!(transport.beta_header(), None);
    }

    #[test]
    fn test_cache_usage_parsing() {
        let resp = AnthropicResponse {
            content: vec![AnthropicContentBlock {
                block_type: "text".to_string(),
                text: Some("Hi".to_string()),
                id: None,
                name: None,
                input: None,
            }],
            stop_reason: Some("end_turn".to_string()),
            usage: AnthropicUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: Some(80),
                cache_read_input_tokens: Some(60),
            },
        };

        let result = AnthropicTransport::parse_response(&resp).unwrap();
        let usage = result.usage.unwrap();

        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.cached_tokens, 60);
        assert_eq!(usage.reasoning_tokens, 80);
    }

    #[test]
    fn test_no_caching_by_default() {
        let transport = AnthropicTransport::new(
            "https://api.anthropic.com".to_string(),
            "test-key".to_string(),
            reqwest::Client::new(),
        );

        assert!(!transport.is_caching_enabled());
        assert_eq!(transport.beta_header(), None);
    }

    #[test]
    fn test_caching_enabled_on_transport() {
        let transport = AnthropicTransport::new(
            "https://api.anthropic.com".to_string(),
            "test-key".to_string(),
            reqwest::Client::new(),
        )
        .with_caching(CacheLayout::PrefixAnd2);

        assert!(transport.is_caching_enabled());
        assert_eq!(transport.beta_header(), Some(CACHE_BETA_HEADER_VALUE));
    }
}
