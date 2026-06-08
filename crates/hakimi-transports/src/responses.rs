//! OpenAI Responses API transport (`/v1/responses`).
//!
//! This transport speaks the newer OpenAI Responses API format used by Codex
//! and newer OpenAI models. It differs from Chat Completions in input/output
//! structure and streaming event types.

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::Stream;
use hakimi_common::{
    ApiMode, FinishReason, HakimiError, Message, MessageRole, NormalizedResponse, Result, ToolCall,
    ToolDefinition, Usage,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use std::pin::Pin;
use tracing::{debug, warn};

use crate::error::classify_error;
use crate::nous_rate_guard;
use crate::params::RequestParams;
use crate::rate_limit::{RateLimitState, RateLimitTracker};
use crate::streaming::StreamEvent;
use crate::trait_def::ProviderTransport;

// ---------------------------------------------------------------------------
// Transport struct
// ---------------------------------------------------------------------------

/// An OpenAI Responses API transport (`/v1/responses`).
///
/// Works with the newer Responses API format that uses typed input/output items,
/// `instructions` instead of system messages, and different streaming event types.
pub struct ResponsesTransport {
    base_url: String,
    api_key: String,
    client: Client,
    rate_limits: RateLimitTracker,
}

impl ResponsesTransport {
    pub fn new(base_url: String, api_key: String, client: Client) -> Self {
        Self {
            base_url,
            api_key,
            client,
            rate_limits: RateLimitTracker::new(),
        }
    }

    /// Return the most recently observed provider rate-limit headers.
    pub fn rate_limits(&self) -> Option<RateLimitState> {
        self.rate_limits.snapshot()
    }

    /// Build the full request URL.
    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/v1/responses", base)
    }

    /// Extract the system message content as instructions (if present).
    fn extract_instructions(messages: &[Message]) -> Option<String> {
        for msg in messages {
            if msg.role == MessageRole::System {
                return msg.content.clone();
            }
        }
        None
    }

    /// Convert messages into Responses API input items (excluding system messages).
    fn convert_messages(messages: &[Message]) -> Vec<JsonValue> {
        let mut items = Vec::new();

        for msg in messages {
            match msg.role {
                MessageRole::System => {
                    // System messages become instructions, not input items.
                    // We skip them here.
                }
                MessageRole::User => {
                    let content = msg.content.as_deref().unwrap_or("");
                    items.push(json!({
                        "role": "user",
                        "content": content,
                    }));
                }
                MessageRole::Assistant => {
                    // If the assistant has tool_calls, emit function_call items
                    // for each tool call before the text content.
                    if let Some(ref tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            items.push(json!({
                                "type": "function_call",
                                "name": tc.name,
                                "arguments": tc.arguments,
                                "call_id": tc.id,
                            }));
                        }
                    }
                    // If the assistant has text content, emit a message item.
                    if let Some(ref content) = msg.content
                        && !content.is_empty()
                    {
                        items.push(json!({
                            "role": "assistant",
                            "content": content,
                        }));
                    }
                }
                MessageRole::Tool => {
                    let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                    let output = msg.content.as_deref().unwrap_or("");
                    items.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": output,
                    }));
                }
            }
        }

        items
    }

    /// Convert our internal [`ToolDefinition`] slice into Responses API function format.
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<JsonValue> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
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
        let instructions = Self::extract_instructions(messages);
        let input = Self::convert_messages(messages);

        let mut body = json!({
            "model": model,
            "input": input,
        });

        if let Some(ref instr) = instructions {
            body["instructions"] = json!(instr);
        }

        if !tools.is_empty() {
            body["tools"] = json!(Self::convert_tools(tools));
        }

        if let Some(temp) = params.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max) = params.max_tokens {
            body["max_output_tokens"] = json!(max);
        }
        if stream {
            body["stream"] = json!(true);
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
        self.build_request_base(model, messages, tools, params, params.stream)
    }

    /// Parse a Responses API response into a [`NormalizedResponse`].
    fn parse_response(resp: &ResponsesOutput) -> Result<NormalizedResponse> {
        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for item in &resp.output {
            match item {
                OutputItem::Message { content: parts, .. } => {
                    for part in parts {
                        match part {
                            ContentPart::OutputText { text } => {
                                content.push_str(text);
                            }
                            ContentPart::InputText { text } => {
                                content.push_str(text);
                            }
                        }
                    }
                }
                OutputItem::FunctionCall {
                    name,
                    arguments,
                    call_id,
                } => {
                    tool_calls.push(ToolCall {
                        id: call_id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                        index: None,
                    });
                }
                OutputItem::FunctionCallOutput { .. } => {
                    // FunctionCallOutput items are not model outputs.
                }
            }
        }

        let content = if content.is_empty() {
            None
        } else {
            Some(content)
        };

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        let finish_reason = match resp.status.as_str() {
            "completed" => {
                if tool_calls.is_some() {
                    Some(FinishReason::ToolCalls)
                } else {
                    Some(FinishReason::Stop)
                }
            }
            "failed" => Some(FinishReason::Error),
            _ => None,
        };

        let usage = resp.usage.as_ref().map(|u| Usage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.input_tokens + u.output_tokens,
            cached_tokens: 0,
            reasoning_tokens: 0,
        });

        Ok(NormalizedResponse {
            content,
            tool_calls,
            finish_reason,
            usage,
            reasoning: None,
        })
    }

    /// Execute a streaming responses request.
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

        if let Some(message) = nous_rate_guard::active_limit_message(&self.base_url) {
            warn!(message = %message, "blocking Nous request while shared rate-limit guard is active");
            return Err(crate::error::TransportError::Api {
                status: 429,
                reason: "NousRateLimitGuard".to_string(),
                retryable: false,
                body: message,
            });
        }

        debug!(url = %url, model = model, "sending streaming responses API request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                warn!(error = %e, "HTTP streaming request failed");
                crate::error::TransportError::Http(format!("HTTP request failed: {e}"))
            })?;

        let status = response.status();
        let previous_rate_limits = self.rate_limits.snapshot();
        let current_rate_limits = self
            .rate_limits
            .update_from_headers(response.headers(), "openai-responses");
        if status.is_success() {
            nous_rate_guard::clear_success(&self.base_url);
        }
        if !status.is_success() {
            let response_text = response.text().await.unwrap_or_default();
            let code = status.as_u16();
            let (reason, retryable) = classify_error(code, &response_text);
            if code == 429
                && let Some(message) = nous_rate_guard::record_genuine_limit(
                    &self.base_url,
                    current_rate_limits.as_ref(),
                    previous_rate_limits.as_ref(),
                )
            {
                warn!(message = %message, "recorded shared Nous rate-limit guard");
                return Err(crate::error::TransportError::Api {
                    status: code,
                    reason: "NousRateLimitGuard".to_string(),
                    retryable: false,
                    body: message,
                });
            }
            warn!(
                status = code,
                ?reason,
                retryable,
                body = %response_text,
                "API returned error for streaming request"
            );
            return Err(crate::error::TransportError::Api {
                status: code,
                reason: format!("{reason:?}"),
                retryable,
                body: response_text,
            });
        }

        let byte_stream = response.bytes_stream();
        let sse_stream = ResponsesSseEventStream::new(Box::pin(byte_stream));
        Ok(Box::pin(sse_stream))
    }
}

// ---------------------------------------------------------------------------
// ProviderTransport implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl ProviderTransport for ResponsesTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::CodexResponses
    }

    fn provider_name(&self) -> &str {
        "openai-responses"
    }

    fn rate_limits(&self) -> Option<RateLimitState> {
        self.rate_limits.snapshot()
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

        if let Some(message) = nous_rate_guard::active_limit_message(&self.base_url) {
            warn!(message = %message, "blocking Nous request while shared rate-limit guard is active");
            return Err(HakimiError::Other(message));
        }

        debug!(url = %url, model = model, "sending responses API request");

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                warn!(error = %e, "HTTP request failed");
                HakimiError::Transport(format!("HTTP request failed: {e}"))
            })?;

        let status = response.status();
        let previous_rate_limits = self.rate_limits.snapshot();
        let current_rate_limits = self
            .rate_limits
            .update_from_headers(response.headers(), "openai-responses");
        if status.is_success() {
            nous_rate_guard::clear_success(&self.base_url);
        }
        let response_text = response
            .text()
            .await
            .map_err(|e| HakimiError::Transport(format!("failed to read response body: {e}")))?;

        if !status.is_success() {
            let code = status.as_u16();
            let (reason, retryable) = classify_error(code, &response_text);
            if code == 429
                && let Some(message) = nous_rate_guard::record_genuine_limit(
                    &self.base_url,
                    current_rate_limits.as_ref(),
                    previous_rate_limits.as_ref(),
                )
            {
                warn!(message = %message, "recorded shared Nous rate-limit guard");
                return Err(HakimiError::Other(message));
            }
            warn!(
                status = code,
                ?reason,
                retryable,
                body = %response_text,
                "API returned error"
            );
            return Err(HakimiError::Transport(format!(
                "API error {code} ({reason:?}, retryable={retryable}): {response_text}"
            )));
        }

        let parsed: ResponsesOutput = serde_json::from_str(&response_text).map_err(|e| {
            warn!(error = %e, "failed to parse response JSON");
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
        ResponsesTransport::execute_streaming(self, model, messages, tools, params)
            .await
            .map_err(|e| {
                let message = e.to_string();
                if nous_rate_guard::is_guard_message(&message) {
                    HakimiError::Other(message)
                } else {
                    HakimiError::Transport(message)
                }
            })
    }
}

// ---------------------------------------------------------------------------
// Wire-format types (Responses API)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ResponsesOutput {
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    status: String,
    output: Vec<OutputItem>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OutputItem {
    #[serde(rename = "message")]
    Message {
        content: Vec<ContentPart>,
        #[allow(dead_code)]
        role: String,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        name: String,
        arguments: String,
        call_id: String,
    },
    #[serde(rename = "function_call_output")]
    FunctionCallOutput {
        #[allow(dead_code)]
        call_id: String,
        #[allow(dead_code)]
        output: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "input_text")]
    InputText { text: String },
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// ---------------------------------------------------------------------------
// Responses API SSE event stream
// ---------------------------------------------------------------------------

/// A specialized SSE event stream for the Responses API.
///
/// The Responses API uses typed SSE events like `response.output_text.delta`,
/// `response.function_call_arguments.delta`, and `response.completed`.
struct ResponsesSseEventStream {
    inner: Pin<Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>>,
    buffer: crate::streaming::SseFullBuffer,
    done: bool,
    pending: Vec<StreamEvent>,
    // Track current function call index for streaming tool calls.
    current_tool_index: usize,
}

impl ResponsesSseEventStream {
    fn new(
        inner: Pin<
            Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
        >,
    ) -> Self {
        Self {
            inner,
            buffer: crate::streaming::SseFullBuffer::new(),
            done: false,
            pending: Vec::new(),
            current_tool_index: 0,
        }
    }

    fn process_event(
        event_type: &str,
        payload: &str,
        current_tool_index: &mut usize,
    ) -> Vec<StreamEvent> {
        let parsed: std::result::Result<JsonValue, _> = serde_json::from_str(payload);
        let Ok(val) = parsed else {
            return vec![];
        };

        let mut events = Vec::new();

        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = val["delta"].as_str()
                    && !delta.is_empty()
                {
                    events.push(StreamEvent::ContentDelta(delta.to_string()));
                }
            }
            "response.function_call_arguments.delta" => {
                let delta = val["delta"].as_str().unwrap_or("");
                events.push(StreamEvent::ToolCallDelta {
                    index: *current_tool_index,
                    id: val["item_id"].as_str().map(String::from),
                    name: None,
                    arguments_delta: delta.to_string(),
                });
            }
            "response.output_item.added" => {
                // Check if this is a function_call item to track its index.
                if let Some(item_type) = val["item"]["type"].as_str()
                    && item_type == "function_call"
                {
                    let id = val["item"]["id"].as_str().map(String::from);
                    let name = val["item"]["name"].as_str().map(String::from);
                    events.push(StreamEvent::ToolCallDelta {
                        index: *current_tool_index,
                        id,
                        name,
                        arguments_delta: String::new(),
                    });
                }
            }
            "response.output_item.done" => {
                // When a function_call item is done, advance the tool index.
                if let Some(item_type) = val["item"]["type"].as_str()
                    && item_type == "function_call"
                {
                    *current_tool_index += 1;
                }
            }
            "response.completed" => {
                // Extract usage from the completed response if present.
                if let Some(usage) = val.get("response").and_then(|r| r.get("usage")) {
                    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0) as u32;
                    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0) as u32;
                    if input_tokens > 0 || output_tokens > 0 {
                        events.push(StreamEvent::Usage {
                            prompt_tokens: input_tokens,
                            completion_tokens: output_tokens,
                        });
                    }
                }
                events.push(StreamEvent::Done);
            }
            "response.incomplete" => {
                // Treat incomplete Responses API streams like an output length
                // stop so the agent can request a continuation instead of
                // returning a partial final answer.
                if let Some(usage) = val.get("response").and_then(|r| r.get("usage")) {
                    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0) as u32;
                    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0) as u32;
                    if input_tokens > 0 || output_tokens > 0 {
                        events.push(StreamEvent::Usage {
                            prompt_tokens: input_tokens,
                            completion_tokens: output_tokens,
                        });
                    }
                }
                events.push(StreamEvent::Finished("length".to_string()));
                events.push(StreamEvent::Done);
            }
            _ => {
                // Ignore other event types: response.created, response.in_progress, etc.
            }
        }

        events
    }

    fn poll_inner(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<std::result::Result<(), String>>> {
        loop {
            match self.inner.poll_next_unpin(cx) {
                std::task::Poll::Ready(Some(Ok(chunk))) => {
                    let pairs = self.buffer.feed(&chunk);

                    for (event_type, payload) in pairs {
                        let et = event_type.as_deref().unwrap_or("");
                        let events =
                            Self::process_event(et, &payload, &mut self.current_tool_index);
                        self.pending.extend(events);

                        // Check if we got a Done event.
                        if self.pending.iter().any(|e| matches!(e, StreamEvent::Done)) {
                            self.done = true;
                            break;
                        }
                    }

                    if !self.pending.is_empty() {
                        return std::task::Poll::Ready(Some(Ok(())));
                    }
                }
                std::task::Poll::Ready(Some(Err(e))) => {
                    return std::task::Poll::Ready(Some(Err(format!("SSE stream error: {e}"))));
                }
                std::task::Poll::Ready(None) => {
                    self.done = true;
                    return std::task::Poll::Ready(None);
                }
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
    }
}

impl Stream for ResponsesSseEventStream {
    type Item = std::result::Result<StreamEvent, String>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // Yield pending events first.
        if !self.pending.is_empty() {
            let event = self.pending.remove(0);
            return std::task::Poll::Ready(Some(Ok(event)));
        }

        if self.done {
            return std::task::Poll::Ready(None);
        }

        // Poll inner stream to fill pending buffer.
        match self.poll_inner(cx) {
            std::task::Poll::Ready(Some(Ok(()))) => {
                if !self.pending.is_empty() {
                    let event = self.pending.remove(0);
                    std::task::Poll::Ready(Some(Ok(event)))
                } else {
                    std::task::Poll::Ready(None)
                }
            }
            std::task::Poll::Ready(Some(Err(e))) => std::task::Poll::Ready(Some(Err(e))),
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Constructor tests --

    #[test]
    fn test_responses_transport_new() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );
        assert_eq!(transport.base_url, "https://api.openai.com");
        assert_eq!(transport.api_key, "sk-test");
    }

    #[test]
    fn test_api_mode_codex_responses() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );
        assert_eq!(transport.api_mode(), ApiMode::CodexResponses);
    }

    #[test]
    fn test_provider_name() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );
        assert_eq!(transport.provider_name(), "openai-responses");
    }

    // -- Request building tests --

    #[test]
    fn test_build_request_body() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );

        let messages = vec![Message::user("Hello")];
        let params = RequestParams::default();
        let body = transport.build_request("gpt-4o", &messages, &[], &params);

        assert_eq!(body["model"], "gpt-4o");
        assert!(body["input"].is_array());
        assert_eq!(body["input"].as_array().unwrap().len(), 1);
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "Hello");
        // No tools or instructions when not provided.
        assert!(body.get("tools").is_none());
        assert!(body.get("instructions").is_none());
    }

    #[test]
    fn test_build_request_with_tools() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );

        let messages = vec![Message::user("Search for something")];
        let tools = vec![ToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            parameters: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
            toolset: "web".to_string(),
        }];
        let params = RequestParams::default();
        let body = transport.build_request("gpt-4o", &messages, &tools, &params);

        assert!(body["tools"].is_array());
        let tools_arr = body["tools"].as_array().unwrap();
        assert_eq!(tools_arr.len(), 1);
        assert_eq!(tools_arr[0]["type"], "function");
        assert_eq!(tools_arr[0]["name"], "search");
        assert_eq!(tools_arr[0]["description"], "Search the web");
    }

    #[test]
    fn test_build_request_with_system_instructions() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );

        let messages = vec![
            Message::system("You are a helpful assistant."),
            Message::user("Hello"),
        ];
        let params = RequestParams::default();
        let body = transport.build_request("gpt-4o", &messages, &[], &params);

        // System message should become instructions.
        assert_eq!(body["instructions"], "You are a helpful assistant.");
        // Input should only contain the user message.
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    // -- Response parsing tests --

    #[test]
    fn test_parse_response_completed() {
        let resp = ResponsesOutput {
            id: "resp_123".to_string(),
            status: "completed".to_string(),
            output: vec![OutputItem::Message {
                content: vec![ContentPart::OutputText {
                    text: "Hello!".to_string(),
                }],
                role: "assistant".to_string(),
            }],
            usage: Some(ResponsesUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        };

        let result = ResponsesTransport::parse_response(&resp).unwrap();
        assert_eq!(result.content.as_deref(), Some("Hello!"));
        assert!(result.tool_calls.is_none());
        assert_eq!(result.finish_reason, Some(FinishReason::Stop));
        assert!(result.usage.is_some());
        assert_eq!(result.usage.as_ref().unwrap().prompt_tokens, 10);
        assert_eq!(result.usage.as_ref().unwrap().completion_tokens, 5);
    }

    #[test]
    fn test_parse_response_with_function_call() {
        let resp = ResponsesOutput {
            id: "resp_456".to_string(),
            status: "completed".to_string(),
            output: vec![OutputItem::FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path":"/tmp/test.txt"}"#.to_string(),
                call_id: "call_789".to_string(),
            }],
            usage: None,
        };

        let result = ResponsesTransport::parse_response(&resp).unwrap();
        assert!(result.content.is_none());
        assert!(result.tool_calls.is_some());
        let calls = result.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_789");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, r#"{"path":"/tmp/test.txt"}"#);
        assert_eq!(result.finish_reason, Some(FinishReason::ToolCalls));
    }

    #[test]
    fn test_parse_response_with_text() {
        let resp = ResponsesOutput {
            id: "resp_abc".to_string(),
            status: "completed".to_string(),
            output: vec![
                OutputItem::FunctionCall {
                    name: "bash".to_string(),
                    arguments: "{}".to_string(),
                    call_id: "call_1".to_string(),
                },
                OutputItem::Message {
                    content: vec![
                        ContentPart::OutputText {
                            text: "Part 1 ".to_string(),
                        },
                        ContentPart::OutputText {
                            text: "Part 2".to_string(),
                        },
                    ],
                    role: "assistant".to_string(),
                },
            ],
            usage: Some(ResponsesUsage {
                input_tokens: 20,
                output_tokens: 10,
            }),
        };

        let result = ResponsesTransport::parse_response(&resp).unwrap();
        assert_eq!(result.content.as_deref(), Some("Part 1 Part 2"));
        assert!(result.tool_calls.is_some());
        assert_eq!(result.tool_calls.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_response_empty_output() {
        let resp = ResponsesOutput {
            id: "resp_empty".to_string(),
            status: "completed".to_string(),
            output: vec![],
            usage: None,
        };

        let result = ResponsesTransport::parse_response(&resp).unwrap();
        assert!(result.content.is_none());
        assert!(result.tool_calls.is_none());
        assert_eq!(result.finish_reason, Some(FinishReason::Stop));
    }

    // -- SSE event parsing tests --

    #[test]
    fn test_sse_event_text_delta() {
        let json_str = r#"{"type":"response.output_text.delta","item_id":"item_1","output_index":0,"content_index":0,"delta":"Hello world"}"#;
        let mut idx = 0;
        let events = ResponsesSseEventStream::process_event(
            "response.output_text.delta",
            json_str,
            &mut idx,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ContentDelta(s) => assert_eq!(s, "Hello world"),
            _ => panic!("expected ContentDelta"),
        }
    }

    #[test]
    fn test_sse_event_function_call_delta() {
        let json_str = r#"{"type":"response.function_call_arguments.delta","item_id":"call_1","output_index":0,"delta":"{\"path\":"}"#;
        let mut idx = 0;
        let events = ResponsesSseEventStream::process_event(
            "response.function_call_arguments.delta",
            json_str,
            &mut idx,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index,
                id,
                arguments_delta,
                ..
            } => {
                assert_eq!(*index, 0);
                assert_eq!(id.as_deref(), Some("call_1"));
                assert_eq!(arguments_delta, "{\"path\":");
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_sse_event_completed() {
        let json_str = r#"{"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[],"usage":{"input_tokens":10,"output_tokens":5}}}"#;
        let mut idx = 0;
        let events =
            ResponsesSseEventStream::process_event("response.completed", json_str, &mut idx);
        assert_eq!(events.len(), 2); // Usage + Done
        match &events[0] {
            StreamEvent::Usage {
                prompt_tokens,
                completion_tokens,
            } => {
                assert_eq!(*prompt_tokens, 10);
                assert_eq!(*completion_tokens, 5);
            }
            _ => panic!("expected Usage"),
        }
        assert!(matches!(events[1], StreamEvent::Done));
    }

    #[test]
    fn test_sse_event_output_item_added_function_call() {
        let json_str = r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"call_xyz","type":"function_call","name":"read_file","arguments":""}}"#;
        let mut idx = 0;
        let events = ResponsesSseEventStream::process_event(
            "response.output_item.added",
            json_str,
            &mut idx,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                assert_eq!(*index, 0);
                assert_eq!(id.as_deref(), Some("call_xyz"));
                assert_eq!(name.as_deref(), Some("read_file"));
                assert!(arguments_delta.is_empty());
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn test_sse_event_output_item_done_advances_index() {
        let mut idx = 0;

        // First function call item added.
        let json_str = r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"call_1","type":"function_call","name":"bash","arguments":""}}"#;
        ResponsesSseEventStream::process_event("response.output_item.added", json_str, &mut idx);
        assert_eq!(idx, 0);

        // Function call arguments delta.
        let json_str = r#"{"type":"response.function_call_arguments.delta","item_id":"call_1","output_index":0,"delta":"{}"}"#;
        ResponsesSseEventStream::process_event(
            "response.function_call_arguments.delta",
            json_str,
            &mut idx,
        );
        assert_eq!(idx, 0);

        // Item done — should advance the index.
        let json_str = r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call"}}"#;
        ResponsesSseEventStream::process_event("response.output_item.done", json_str, &mut idx);
        assert_eq!(idx, 1);
    }

    #[test]
    fn test_sse_event_ignored_types() {
        let mut idx = 0;

        // response.created should be ignored.
        let events = ResponsesSseEventStream::process_event(
            "response.created",
            r#"{"type":"response.created","response":{}}"#,
            &mut idx,
        );
        assert!(events.is_empty());

        // response.in_progress should be ignored.
        let events = ResponsesSseEventStream::process_event(
            "response.in_progress",
            r#"{"type":"response.in_progress"}"#,
            &mut idx,
        );
        assert!(events.is_empty());
    }

    #[test]
    fn test_sse_event_incomplete() {
        let json_str = r#"{"type":"response.incomplete","response":{"id":"resp_1","status":"incomplete","output":[],"usage":{"input_tokens":10,"output_tokens":3}}}"#;
        let mut idx = 0;
        let events =
            ResponsesSseEventStream::process_event("response.incomplete", json_str, &mut idx);
        assert_eq!(events.len(), 3); // Usage + Finished(length) + Done
        assert!(matches!(events[0], StreamEvent::Usage { .. }));
        assert!(matches!(events[1], StreamEvent::Finished(ref reason) if reason == "length"));
        assert!(matches!(events[2], StreamEvent::Done));
    }

    // -- Message conversion tests --

    #[test]
    fn test_convert_messages_system_to_instructions() {
        let messages = vec![
            Message::system("Be helpful"),
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];

        let items = ResponsesTransport::convert_messages(&messages);
        // System message should be excluded from input items.
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[1]["role"], "assistant");
    }

    #[test]
    fn test_convert_messages_user_assistant() {
        let messages = vec![
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems programming language."),
        ];

        let items = ResponsesTransport::convert_messages(&messages);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[0]["content"], "What is Rust?");
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(
            items[1]["content"],
            "Rust is a systems programming language."
        );
    }

    #[test]
    fn test_convert_messages_with_tool_output() {
        let messages = vec![
            Message::user("Read the file"),
            Message {
                role: MessageRole::Assistant,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"/tmp/test.txt"}"#.to_string(),
                    index: None,
                }]),
                tool_call_id: None,
                name: None,
                reasoning: None,
                reasoning_content: None,
                images: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            },
            Message::tool_result("call_1", "read_file", "file contents here"),
        ];

        let items = ResponsesTransport::convert_messages(&messages);
        assert_eq!(items.len(), 3);
        // User message.
        assert_eq!(items[0]["role"], "user");
        // Function call item (no role, has type).
        assert_eq!(items[1]["type"], "function_call");
        assert_eq!(items[1]["name"], "read_file");
        assert_eq!(items[1]["call_id"], "call_1");
        // Function call output.
        assert_eq!(items[2]["type"], "function_call_output");
        assert_eq!(items[2]["call_id"], "call_1");
        assert_eq!(items[2]["output"], "file contents here");
    }

    #[test]
    fn test_convert_messages_assistant_with_tool_calls_and_content() {
        let messages = vec![Message {
            role: MessageRole::Assistant,
            content: Some("Let me search for that.".to_string()),
            tool_calls: Some(vec![ToolCall {
                id: "call_2".to_string(),
                name: "search".to_string(),
                arguments: r#"{"query":"test"}"#.to_string(),
                index: None,
            }]),
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            images: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        }];

        let items = ResponsesTransport::convert_messages(&messages);
        assert_eq!(items.len(), 2);
        // First: function_call item.
        assert_eq!(items[0]["type"], "function_call");
        assert_eq!(items[0]["name"], "search");
        // Second: assistant message with text.
        assert_eq!(items[1]["role"], "assistant");
        assert_eq!(items[1]["content"], "Let me search for that.");
    }

    // -- Tool conversion tests --

    #[test]
    fn test_convert_tools() {
        let tools = vec![
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    },
                    "required": ["path"]
                }),
                toolset: "file".to_string(),
            },
            ToolDefinition {
                name: "bash".to_string(),
                description: "Execute a command".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    }
                }),
                toolset: "shell".to_string(),
            },
        ];

        let converted = ResponsesTransport::convert_tools(&tools);
        assert_eq!(converted.len(), 2);

        assert_eq!(converted[0]["type"], "function");
        assert_eq!(converted[0]["name"], "read_file");
        assert_eq!(converted[0]["description"], "Read a file");
        assert!(converted[0]["parameters"].is_object());

        assert_eq!(converted[1]["type"], "function");
        assert_eq!(converted[1]["name"], "bash");
        assert_eq!(converted[1]["description"], "Execute a command");
    }

    // -- Endpoint tests --

    #[test]
    fn test_endpoint_strips_trailing_slash() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com/v1/".to_string(),
            "sk-test".to_string(),
            client,
        );
        assert_eq!(
            transport.endpoint(),
            "https://api.openai.com/v1/v1/responses"
        );
    }

    #[test]
    fn test_endpoint_no_trailing_slash() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );
        assert_eq!(transport.endpoint(), "https://api.openai.com/v1/responses");
    }

    // -- Instructions extraction tests --

    #[test]
    fn test_extract_instructions_present() {
        let messages = vec![
            Message::system("You are a coding assistant."),
            Message::user("Hello"),
        ];
        let instructions = ResponsesTransport::extract_instructions(&messages);
        assert_eq!(instructions.as_deref(), Some("You are a coding assistant."));
    }

    #[test]
    fn test_extract_instructions_absent() {
        let messages = vec![Message::user("Hello")];
        let instructions = ResponsesTransport::extract_instructions(&messages);
        assert!(instructions.is_none());
    }

    // -- Build request with params tests --

    #[test]
    fn test_build_request_with_temperature_and_max_tokens() {
        let client = Client::new();
        let transport = ResponsesTransport::new(
            "https://api.openai.com".to_string(),
            "sk-test".to_string(),
            client,
        );

        let messages = vec![Message::user("Hello")];
        let params = RequestParams {
            temperature: Some(0.7),
            max_tokens: Some(1024),
            top_p: None,
            stop: None,
            stream: false,
        };
        let body = transport.build_request("gpt-4o", &messages, &[], &params);

        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["max_output_tokens"], 1024);
    }

    // -- Config api_mode tests --

    #[test]
    fn test_config_api_mode_deserialize() {
        let yaml = r#"
model:
  default: "gpt-4o"
  provider: "openai"
  api_mode: "responses"
"#;
        let config: hakimi_config::HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model.api_mode, "responses");
    }

    #[test]
    fn test_config_api_mode_default_empty() {
        let config = hakimi_config::ModelConfig::default();
        assert!(config.api_mode.is_empty());
    }

    #[test]
    fn test_config_api_mode_all_variants() {
        // Test each valid variant.
        for mode in &[
            "chat_completions",
            "responses",
            "anthropic_messages",
            "openai",
            "codex",
            "anthropic",
        ] {
            let yaml = format!(
                r#"
model:
  default: "gpt-4o"
  api_mode: "{}"
"#,
                mode
            );
            let config: hakimi_config::HakimiConfig = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(config.model.api_mode, *mode);
        }
    }

    #[test]
    fn test_config_api_mode_empty_by_default() {
        let yaml = r#"
model:
  default: "gpt-4o"
"#;
        let config: hakimi_config::HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.model.api_mode.is_empty());
    }
}
