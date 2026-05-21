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
use crate::streaming::{SseEventStream, StreamEvent};
use crate::trait_def::ProviderTransport;
use futures::stream::Stream;
use std::pin::Pin;

/// A Google Gemini GenerateContent API transport.
///
/// Implements the Gemini-specific wire format for
/// `/v1beta/models/{model}:generateContent` and the streaming variant
/// `/v1beta/models/{model}:streamGenerateContent`.
///
/// Key differences from OpenAI/Anthropic formats:
/// - Uses `contents` array with `role` ("user"/"model") and `parts` array
/// - System prompt is a separate `systemInstruction` top-level field
/// - Tool calls: `functionCall` parts in model messages
/// - Tool results: `functionResponse` parts in user messages
/// - Auth via `?key=API_KEY` query parameter
pub struct GeminiTransport {
    base_url: String,
    api_key: String,
    client: Client,
}

impl GeminiTransport {
    pub fn new(base_url: String, api_key: String, client: Client) -> Self {
        Self {
            base_url,
            api_key,
            client,
        }
    }

    /// Build the full request URL for non-streaming requests.
    fn endpoint(&self, model: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            base, model, self.api_key
        )
    }

    /// Build the full request URL for streaming requests.
    fn streaming_endpoint(&self, model: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            base, model, self.api_key
        )
    }

    /// Extract the system prompt from messages (if present) and return the remaining messages.
    fn split_system_prompt(messages: &[Message]) -> (Option<String>, Vec<&Message>) {
        let mut system: Option<String> = None;
        let mut rest = Vec::with_capacity(messages.len());

        for msg in messages {
            if msg.role == MessageRole::System {
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

    /// Convert our internal [`Message`] slice into Gemini JSON wire format.
    ///
    /// Key transformations:
    /// - `System` messages are excluded (handled by `split_system_prompt`).
    /// - `User` messages become `{"role": "user", "parts": [{"text": "..."}]}`.
    /// - `Assistant` messages become `{"role": "model", "parts": [...]}` with
    ///   text parts and `functionCall` parts for tool calls.
    /// - `Tool` messages become `{"role": "user", "parts": [{"functionResponse": {...}}]}`.
    /// - Adjacent messages with the same role are merged as required by Gemini.
    fn convert_messages(messages: &[&Message]) -> Vec<JsonValue> {
        let mut result: Vec<JsonValue> = Vec::new();

        for msg in messages {
            match msg.role {
                MessageRole::System => {
                    continue;
                }
                MessageRole::User => {
                    let obj = json!({
                        "role": "user",
                        "parts": [{"text": msg.content.as_deref().unwrap_or("")}]
                    });
                    result.push(obj);
                }
                MessageRole::Assistant => {
                    let mut parts: Vec<JsonValue> = Vec::new();

                    // Text content part (if any).
                    if let Some(ref text) = msg.content
                        && !text.is_empty()
                    {
                        parts.push(json!({"text": text}));
                    }

                    // functionCall parts from tool_calls.
                    if let Some(ref tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let args: JsonValue =
                                serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
                            parts.push(json!({
                                "functionCall": {
                                    "name": tc.name,
                                    "args": args
                                }
                            }));
                        }
                    }

                    if parts.is_empty() {
                        parts.push(json!({"text": ""}));
                    }

                    let obj = json!({
                        "role": "model",
                        "parts": parts
                    });
                    result.push(obj);
                }
                MessageRole::Tool => {
                    // Tool results in Gemini are sent as a user message with
                    // functionResponse parts.
                    let function_name = msg.name.as_deref().unwrap_or("");
                    let content_text = msg.content.as_deref().unwrap_or("");

                    // Try to parse content as JSON; fall back to wrapping in a string.
                    let response_value: JsonValue = serde_json::from_str(content_text)
                        .unwrap_or_else(|_| json!({"result": content_text}));

                    let obj = json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": function_name,
                                "response": response_value
                            }
                        }]
                    });
                    result.push(obj);
                }
            }
        }

        // Gemini requires that no two adjacent messages have the same role.
        merge_adjacent_same_role(result)
    }

    /// Convert our internal [`ToolDefinition`] slice into Gemini tool format.
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<JsonValue> {
        if tools.is_empty() {
            return vec![];
        }

        let declarations: Vec<JsonValue> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                })
            })
            .collect();

        vec![json!({
            "functionDeclarations": declarations
        })]
    }

    /// Build the full JSON request body.
    fn build_request_base(
        &self,
        _model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
        _stream: bool,
    ) -> JsonValue {
        let (system, remaining) = Self::split_system_prompt(messages);
        let gemini_messages = Self::convert_messages(&remaining);

        let mut body = json!({
            "contents": gemini_messages,
        });

        if let Some(ref sys) = system
            && !sys.is_empty()
        {
            body["systemInstruction"] = json!({
                "parts": [{"text": sys}]
            });
        }

        if !tools.is_empty() {
            body["tools"] = json!(Self::convert_tools(tools));
        }

        // Build generationConfig from params.
        let mut gen_config = json!({});
        if let Some(temp) = params.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if let Some(max) = params.max_tokens {
            gen_config["maxOutputTokens"] = json!(max);
        }
        if let Some(top_p) = params.top_p {
            gen_config["topP"] = json!(top_p);
        }
        if let Some(ref stop) = params.stop {
            gen_config["stopSequences"] = json!(stop);
        }

        if gen_config.as_object().is_some_and(|o| !o.is_empty()) {
            body["generationConfig"] = gen_config;
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

    /// Parse a Gemini GenerateContent response into a [`NormalizedResponse`].
    fn parse_response(resp: &GeminiResponse) -> Result<NormalizedResponse> {
        let candidate = resp.candidates.first().ok_or_else(|| {
            HakimiError::Transport("response contained no candidates".to_string())
        })?;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for (idx, part) in candidate.content.parts.iter().enumerate() {
            if let Some(ref text) = part.text
                && !text.is_empty()
            {
                text_parts.push(text.clone());
            }

            if let Some(ref fc) = part.function_call {
                let name = fc.name.clone();
                let args = serde_json::to_string(&fc.args).unwrap_or_else(|_| "{}".to_string());
                tool_calls.push(ToolCall {
                    id: format!("{}-{}", name, idx),
                    name,
                    arguments: args,
                    index: None,
                });
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

        let finish_reason = match candidate.finish_reason.as_deref() {
            Some("STOP") => Some(FinishReason::Stop),
            Some("MAX_TOKENS") => Some(FinishReason::Length),
            Some("SAFETY") => Some(FinishReason::ContentFilter),
            Some("RECITATION") => Some(FinishReason::ContentFilter),
            Some("OTHER") => Some(FinishReason::Error),
            _ => None,
        };

        // If there are tool calls and no explicit finish reason, mark as ToolCalls.
        let finish_reason = if tool_calls.is_some() && finish_reason.is_none() {
            Some(FinishReason::ToolCalls)
        } else {
            finish_reason
        };

        let usage = resp.usage_metadata.as_ref().map(|u| Usage {
            prompt_tokens: u.prompt_token_count.unwrap_or(0),
            completion_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count.unwrap_or(0),
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

    /// Execute a streaming Gemini GenerateContent request.
    ///
    /// Returns a `Stream` of [`StreamEvent`]s as they arrive from the provider.
    /// The stream ends when the provider signals completion.
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
        let url = self.streaming_endpoint(model);

        debug!(url = %url, model = model, "sending streaming Gemini generateContent request");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
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
                "Gemini API returned error for streaming request"
            );
            return Err(crate::error::TransportError::Api {
                status: code,
                reason: format!("{reason:?}"),
                retryable,
                body: response_text,
            });
        }

        let byte_stream = response.bytes_stream();
        let sse_stream = SseEventStream::gemini(Box::pin(byte_stream));
        Ok(Box::pin(sse_stream))
    }
}

/// Merge adjacent messages with the same role, as required by the Gemini API.
///
/// When consecutive messages share a role, their parts are concatenated.
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
            // Both messages have the same role. Merge parts arrays.
            let existing_parts = parts_to_array(last["parts"].clone());
            let new_parts = parts_to_array(msg["parts"].clone());

            let mut combined = existing_parts;
            combined.extend(new_parts);
            last["parts"] = JsonValue::Array(combined);
        } else {
            merged.push(msg);
        }
    }

    merged
}

/// Normalize parts to a Vec of part objects.
fn parts_to_array(parts: JsonValue) -> Vec<JsonValue> {
    match parts {
        JsonValue::Array(arr) => arr,
        other => vec![other],
    }
}

#[async_trait]
impl ProviderTransport for GeminiTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::GeminiGenerateContent
    }

    fn provider_name(&self) -> &str {
        "gemini"
    }

    async fn execute(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> Result<NormalizedResponse> {
        let body = self.build_request(model, messages, tools, params);
        let url = self.endpoint(model);

        debug!(url = %url, model = model, "sending Gemini generateContent request");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
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
                "Gemini API returned error"
            );
            return Err(HakimiError::Transport(format!(
                "API error {code} ({reason:?}, retryable={retryable}): {response_text}"
            )));
        }

        let parsed: GeminiResponse = serde_json::from_str(&response_text).map_err(|e| {
            warn!(error = %e, "failed to parse Gemini response JSON");
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
        GeminiTransport::execute_streaming(self, model, messages, tools, params)
            .await
            .map_err(|e| HakimiError::Transport(e.to_string()))
    }
}

// ── Wire-format types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    content: GeminiContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    function_call: Option<GeminiFunctionCall>,
    #[serde(default)]
    #[allow(dead_code)]
    function_response: Option<GeminiFunctionResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: JsonValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionResponse {
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    response: JsonValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    #[serde(default)]
    prompt_token_count: Option<u32>,
    #[serde(default)]
    candidates_token_count: Option<u32>,
    #[serde(default)]
    total_token_count: Option<u32>,
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Unit tests for message conversion ──────────────────────────────────

    #[test]
    fn test_convert_user_message() {
        let msg = Message::user("Hello, Gemini!");
        let messages = GeminiTransport::convert_messages(&[&msg]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["parts"][0]["text"], "Hello, Gemini!");
    }

    #[test]
    fn test_convert_assistant_message_with_text() {
        let msg = Message::assistant("I can help.");
        let messages = GeminiTransport::convert_messages(&[&msg]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "model");
        assert_eq!(messages[0]["parts"][0]["text"], "I can help.");
    }

    #[test]
    fn test_convert_assistant_message_with_tool_calls() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: Some("Let me check.".to_string()),
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
            timestamp: None,
            token_count: None,
            finish_reason: None,
        };
        let messages = GeminiTransport::convert_messages(&[&msg]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "model");
        assert_eq!(messages[0]["parts"][0]["text"], "Let me check.");
        assert_eq!(messages[0]["parts"][1]["functionCall"]["name"], "read_file");
        assert_eq!(
            messages[0]["parts"][1]["functionCall"]["args"]["path"],
            "/tmp/test.txt"
        );
    }

    #[test]
    fn test_convert_tool_result_message() {
        let msg = Message::tool_result("call_1", "read_file", "file contents here");
        let messages = GeminiTransport::convert_messages(&[&msg]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(
            messages[0]["parts"][0]["functionResponse"]["name"],
            "read_file"
        );
        assert_eq!(
            messages[0]["parts"][0]["functionResponse"]["response"]["result"],
            "file contents here"
        );
    }

    #[test]
    fn test_convert_tool_result_with_json_content() {
        let msg = Message::tool_result("call_1", "search", r#"{"results": ["a", "b"]}"#);
        let messages = GeminiTransport::convert_messages(&[&msg]);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0]["parts"][0]["functionResponse"]["response"]["results"][0],
            "a"
        );
    }

    #[test]
    fn test_system_messages_excluded() {
        let sys = Message::system("You are helpful.");
        let user = Message::user("Hi");
        let messages = GeminiTransport::convert_messages(&[&sys, &user]);
        // System message should be filtered out.
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn test_merge_adjacent_same_role() {
        let user1 = Message::user("First");
        let user2 = Message::user("Second");
        let messages = GeminiTransport::convert_messages(&[&user1, &user2]);
        // Should be merged into a single user message with 2 parts.
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["parts"].as_array().unwrap().len(), 2);
    }

    // ── Unit tests for tool conversion ─────────────────────────────────────

    #[test]
    fn test_convert_tools_empty() {
        let tools: Vec<ToolDefinition> = vec![];
        let result = GeminiTransport::convert_tools(&tools);
        assert!(result.is_empty());
    }

    #[test]
    fn test_convert_tools_single() {
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from disk".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        }];
        let result = GeminiTransport::convert_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["functionDeclarations"][0]["name"], "read_file");
        assert_eq!(
            result[0]["functionDeclarations"][0]["description"],
            "Read a file from disk"
        );
    }

    // ── Unit tests for system prompt extraction ────────────────────────────

    #[test]
    fn test_split_system_prompt_single() {
        let messages = vec![Message::system("Be helpful"), Message::user("Hello")];
        let (system, rest) = GeminiTransport::split_system_prompt(&messages);
        assert_eq!(system.as_deref(), Some("Be helpful"));
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].role, MessageRole::User);
    }

    #[test]
    fn test_split_system_prompt_multiple() {
        let messages = vec![
            Message::system("Be helpful"),
            Message::system("Be concise"),
            Message::user("Hello"),
        ];
        let (system, rest) = GeminiTransport::split_system_prompt(&messages);
        assert_eq!(system.as_deref(), Some("Be helpful\n\nBe concise"));
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn test_split_system_prompt_none() {
        let messages = vec![Message::user("Hello")];
        let (system, rest) = GeminiTransport::split_system_prompt(&messages);
        assert!(system.is_none());
        assert_eq!(rest.len(), 1);
    }

    // ── Unit tests for response parsing ────────────────────────────────────

    #[test]
    fn test_parse_text_response() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("Hello, world!".to_string()),
                        function_call: None,
                        function_response: None,
                    }],
                },
                finish_reason: Some("STOP".to_string()),
            }],
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: Some(10),
                candidates_token_count: Some(5),
                total_token_count: Some(15),
            }),
        };

        let result = GeminiTransport::parse_response(&resp).unwrap();
        assert_eq!(result.content.as_deref(), Some("Hello, world!"));
        assert!(result.tool_calls.is_none());
        assert_eq!(result.finish_reason, Some(FinishReason::Stop));
        let usage = result.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_parse_tool_call_response() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: None,
                        function_call: Some(GeminiFunctionCall {
                            name: "read_file".to_string(),
                            args: json!({"path": "/tmp/test.txt"}),
                        }),
                        function_response: None,
                    }],
                },
                finish_reason: None,
            }],
            usage_metadata: None,
        };

        let result = GeminiTransport::parse_response(&resp).unwrap();
        assert!(result.content.is_none());
        let tool_calls = result.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "read_file");
        // When tool calls are present and no explicit finish reason, it should be ToolCalls.
        assert_eq!(result.finish_reason, Some(FinishReason::ToolCalls));
    }

    #[test]
    fn test_parse_mixed_response() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![
                        GeminiPart {
                            text: Some("Let me read that file.".to_string()),
                            function_call: None,
                            function_response: None,
                        },
                        GeminiPart {
                            text: None,
                            function_call: Some(GeminiFunctionCall {
                                name: "read_file".to_string(),
                                args: json!({"path": "/tmp/test.txt"}),
                            }),
                            function_response: None,
                        },
                    ],
                },
                finish_reason: Some("STOP".to_string()),
            }],
            usage_metadata: None,
        };

        let result = GeminiTransport::parse_response(&resp).unwrap();
        assert_eq!(result.content.as_deref(), Some("Let me read that file."));
        let tool_calls = result.tool_calls.unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "read_file");
        // finish_reason is STOP but there are tool calls — STOP takes precedence
        // (the transport doesn't override explicit finish reasons).
        assert_eq!(result.finish_reason, Some(FinishReason::Stop));
    }

    #[test]
    fn test_parse_max_tokens_finish_reason() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("truncated".to_string()),
                        function_call: None,
                        function_response: None,
                    }],
                },
                finish_reason: Some("MAX_TOKENS".to_string()),
            }],
            usage_metadata: None,
        };

        let result = GeminiTransport::parse_response(&resp).unwrap();
        assert_eq!(result.finish_reason, Some(FinishReason::Length));
    }

    #[test]
    fn test_parse_safety_finish_reason() {
        let resp = GeminiResponse {
            candidates: vec![GeminiCandidate {
                content: GeminiContent { parts: vec![] },
                finish_reason: Some("SAFETY".to_string()),
            }],
            usage_metadata: None,
        };

        let result = GeminiTransport::parse_response(&resp).unwrap();
        assert_eq!(result.finish_reason, Some(FinishReason::ContentFilter));
    }

    #[test]
    fn test_parse_no_candidates() {
        let resp = GeminiResponse {
            candidates: vec![],
            usage_metadata: None,
        };

        let result = GeminiTransport::parse_response(&resp);
        assert!(result.is_err());
    }

    // ── Unit tests for request building ────────────────────────────────────

    #[test]
    fn test_build_request_with_system_prompt() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com".to_string(),
            "test-key".to_string(),
            Client::new(),
        );

        let messages = vec![
            Message::system("You are a coding assistant."),
            Message::user("Write hello world."),
        ];
        let params = RequestParams::default();
        let body = transport.build_request("gemini-pro", &messages, &[], &params);

        assert_eq!(body["contents"].as_array().unwrap().len(), 1);
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "You are a coding assistant."
        );
    }

    #[test]
    fn test_build_request_with_generation_config() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com".to_string(),
            "test-key".to_string(),
            Client::new(),
        );

        let messages = vec![Message::user("Hello")];
        let params = RequestParams {
            temperature: Some(0.7),
            max_tokens: Some(4096),
            top_p: Some(0.9),
            stop: Some(vec!["STOP".to_string()]),
            stream: false,
        };
        let body = transport.build_request("gemini-pro", &messages, &[], &params);

        let gen_cfg = &body["generationConfig"];
        assert_eq!(gen_cfg["temperature"], 0.7);
        assert_eq!(gen_cfg["maxOutputTokens"], 4096);
        assert_eq!(gen_cfg["topP"], 0.9);
        assert_eq!(gen_cfg["stopSequences"][0], "STOP");
    }

    #[test]
    fn test_build_request_with_tools() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com".to_string(),
            "test-key".to_string(),
            Client::new(),
        );

        let messages = vec![Message::user("Read a file")];
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }];
        let params = RequestParams::default();
        let body = transport.build_request("gemini-pro", &messages, &tools, &params);

        assert!(body.get("tools").is_some());
        let declarations = &body["tools"][0]["functionDeclarations"];
        assert_eq!(declarations[0]["name"], "read_file");
    }

    #[test]
    fn test_build_request_no_system_prompt() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com".to_string(),
            "test-key".to_string(),
            Client::new(),
        );

        let messages = vec![Message::user("Hello")];
        let params = RequestParams::default();
        let body = transport.build_request("gemini-pro", &messages, &[], &params);

        assert!(body.get("systemInstruction").is_none());
    }

    // ── Unit tests for URL building ────────────────────────────────────────

    #[test]
    fn test_endpoint_url() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com".to_string(),
            "test-api-key".to_string(),
            Client::new(),
        );
        let url = transport.endpoint("gemini-pro");
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent?key=test-api-key"
        );
    }

    #[test]
    fn test_streaming_endpoint_url() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com".to_string(),
            "test-api-key".to_string(),
            Client::new(),
        );
        let url = transport.streaming_endpoint("gemini-pro");
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:streamGenerateContent?alt=sse&key=test-api-key"
        );
    }

    #[test]
    fn test_endpoint_trailing_slash() {
        let transport = GeminiTransport::new(
            "https://generativelanguage.googleapis.com/".to_string(),
            "key".to_string(),
            Client::new(),
        );
        let url = transport.endpoint("gemini-pro");
        assert!(
            !url.contains("//v1beta"),
            "should not have double slash: {url}"
        );
    }

    // ── Unit tests for transport metadata ──────────────────────────────────

    #[test]
    fn test_api_mode() {
        let transport = GeminiTransport::new(
            "https://example.com".to_string(),
            "key".to_string(),
            Client::new(),
        );
        assert_eq!(transport.api_mode(), ApiMode::GeminiGenerateContent);
    }

    #[test]
    fn test_provider_name() {
        let transport = GeminiTransport::new(
            "https://example.com".to_string(),
            "key".to_string(),
            Client::new(),
        );
        assert_eq!(transport.provider_name(), "gemini");
    }

    // ── Unit tests for JSON deserialization ────────────────────────────────

    #[test]
    fn test_deserialize_gemini_response() {
        let json_str = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "Hello!"}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 2,
                "totalTokenCount": 7
            }
        }"#;

        let resp: GeminiResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(
            resp.candidates[0].content.parts[0].text.as_deref(),
            Some("Hello!")
        );
        assert_eq!(resp.candidates[0].finish_reason.as_deref(), Some("STOP"));
        let usage = resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, Some(5));
        assert_eq!(usage.candidates_token_count, Some(2));
        assert_eq!(usage.total_token_count, Some(7));
    }

    #[test]
    fn test_deserialize_gemini_response_with_function_call() {
        let json_str = r#"{
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "bash",
                            "args": {"command": "ls -la"}
                        }
                    }]
                }
            }]
        }"#;

        let resp: GeminiResponse = serde_json::from_str(json_str).unwrap();
        let part = &resp.candidates[0].content.parts[0];
        let fc = part.function_call.as_ref().unwrap();
        assert_eq!(fc.name, "bash");
        assert_eq!(fc.args["command"], "ls -la");
    }

    #[test]
    fn test_deserialize_gemini_response_empty() {
        let json_str = r#"{"candidates": []}"#;
        let resp: GeminiResponse = serde_json::from_str(json_str).unwrap();
        assert!(resp.candidates.is_empty());
        assert!(resp.usage_metadata.is_none());
    }

    #[test]
    fn test_deserialize_gemini_response_no_usage() {
        let json_str = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "Hi"}]
                }
            }]
        }"#;

        let resp: GeminiResponse = serde_json::from_str(json_str).unwrap();
        assert!(resp.usage_metadata.is_none());
    }

    // ── Integration-style test for merge_adjacent_same_role ────────────────

    #[test]
    fn test_merge_adjacent_user_messages() {
        let user1 = Message::user("First question");
        let user2 = Message::user("Second question");
        let assistant = Message::assistant("Answer");
        let user3 = Message::user("Follow-up");

        let messages = GeminiTransport::convert_messages(&[&user1, &user2, &assistant, &user3]);
        // user1 + user2 should be merged; assistant separate; user3 separate.
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["parts"].as_array().unwrap().len(), 2);
        assert_eq!(messages[1]["role"], "model");
        assert_eq!(messages[2]["role"], "user");
    }

    #[test]
    fn test_merge_adjacent_model_messages() {
        let model1 = Message::assistant("Thinking...");
        let model2 = Message::assistant("Here's the answer.");
        let messages = GeminiTransport::convert_messages(&[&model1, &model2]);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "model");
        assert_eq!(messages[0]["parts"].as_array().unwrap().len(), 2);
    }
}
