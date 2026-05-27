use async_trait::async_trait;
use hakimi_common::{
    ApiMode, FinishReason, HakimiError, Message, NormalizedResponse, Result, ToolCall,
    ToolDefinition, Usage,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use tracing::{debug, warn};

use crate::error::classify_error;
use crate::params::RequestParams;
use crate::rate_limit::{RateLimitState, RateLimitTracker};
use crate::streaming::{SseEventStream, StreamEvent};
use crate::trait_def::ProviderTransport;
use futures::stream::Stream;
use std::pin::Pin;

/// An OpenAI-compatible Chat Completions transport.
///
/// Works with any provider that exposes `/v1/chat/completions` (OpenAI, Together,
/// Groq, Fireworks, local vLLM / Ollama, etc.).
pub struct ChatCompletionsTransport {
    base_url: String,
    api_key: String,
    client: Client,
    rate_limits: RateLimitTracker,
}

impl ChatCompletionsTransport {
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
        // Avoid doubling /v1 if the base_url already includes it
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }

    /// Convert our internal [`Message`] slice into the OpenAI JSON wire format.
    fn convert_messages(messages: &[Message]) -> Vec<JsonValue> {
        messages
            .iter()
            .map(|m| {
                let mut obj = json!({
                    "role": m.role,
                });

                // Content — may be absent for assistant messages with tool_calls.
                if let Some(ref content) = m.content {
                    if let Some(ref images) = m.images {
                        if !images.is_empty() {
                            let mut content_array = Vec::new();
                            content_array.push(json!({
                                "type": "text",
                                "text": content
                            }));
                            for img in images {
                                let url = format!("data:{};base64,{}", img.mime_type, img.data);
                                content_array.push(json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": url
                                    }
                                }));
                            }
                            obj["content"] = json!(content_array);
                        } else {
                            obj["content"] = json!(content);
                        }
                    } else {
                        obj["content"] = json!(content);
                    }
                }

                // Tool calls (assistant messages).
                if let Some(ref tool_calls) = m.tool_calls {
                    let calls: Vec<JsonValue> = tool_calls
                        .iter()
                        .map(|tc| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments,
                                }
                            })
                        })
                        .collect();
                    obj["tool_calls"] = json!(calls);
                }

                // Tool result messages.
                if let Some(ref tool_call_id) = m.tool_call_id {
                    obj["tool_call_id"] = json!(tool_call_id);
                }
                if let Some(ref name) = m.name {
                    obj["name"] = json!(name);
                }

                // Reasoning content — must be passed back for reasoning models
                // (DeepSeek R1, QwQ, etc.) or the API returns 400.
                if let Some(ref reasoning_content) = m.reasoning_content {
                    obj["reasoning_content"] = json!(reasoning_content);
                }

                obj
            })
            .collect()
    }

    /// Convert our internal [`ToolDefinition`] slice into OpenAI function-calling format.
    fn convert_tools(tools: &[ToolDefinition]) -> Vec<JsonValue> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
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
        let mut body = json!({
            "model": model,
            "messages": Self::convert_messages(messages),
        });

        if !tools.is_empty() {
            body["tools"] = json!(Self::convert_tools(tools));
        }

        if let Some(temp) = params.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max) = params.max_tokens {
            body["max_tokens"] = json!(max);
        }
        if let Some(top_p) = params.top_p {
            body["top_p"] = json!(top_p);
        }
        if let Some(ref stop) = params.stop {
            body["stop"] = json!(stop);
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

    /// Parse an OpenAI Chat Completions response into a [`NormalizedResponse`].
    fn parse_response(resp: &ChatCompletionResponse) -> Result<NormalizedResponse> {
        let choice = resp
            .choices
            .first()
            .ok_or_else(|| HakimiError::Transport("response contained no choices".to_string()))?;

        let content = choice.message.content.clone();

        let tool_calls = choice.message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                    index: None,
                })
                .collect()
        });

        let finish_reason = match choice.finish_reason.as_deref() {
            Some("stop") => Some(FinishReason::Stop),
            Some("tool_calls") => Some(FinishReason::ToolCalls),
            Some("length") => Some(FinishReason::Length),
            Some("content_filter") => Some(FinishReason::ContentFilter),
            _ => None,
        };

        let usage = resp.usage.as_ref().map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            cached_tokens: u
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)
                .unwrap_or(0),
            reasoning_tokens: u
                .completion_tokens_details
                .as_ref()
                .and_then(|d| d.reasoning_tokens)
                .unwrap_or(0),
        });

        Ok(NormalizedResponse {
            content,
            tool_calls,
            finish_reason,
            usage,
            reasoning: choice.message.reasoning_content.clone(),
        })
    }

    /// Execute a streaming chat completions request.
    ///
    /// Returns a `Stream` of [`StreamEvent`]s as they arrive from the provider.
    /// The stream ends when the provider sends `data: [DONE]` or closes the connection.
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

        debug!(url = %url, model = model, "sending streaming chat completions request");

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
        self.rate_limits
            .update_from_headers(response.headers(), "openai-compatible");
        if !status.is_success() {
            let response_text = response.text().await.unwrap_or_default();
            let code = status.as_u16();
            let (reason, retryable) = classify_error(code, &response_text);
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
        let sse_stream = SseEventStream::openai(Box::pin(byte_stream));
        Ok(Box::pin(sse_stream))
    }
}

#[async_trait]
impl ProviderTransport for ChatCompletionsTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::ChatCompletions
    }

    fn provider_name(&self) -> &str {
        "openai-compatible"
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

        debug!(url = %url, model = model, "sending chat completions request");

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
        self.rate_limits
            .update_from_headers(response.headers(), "openai-compatible");
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
                "API returned error"
            );
            return Err(HakimiError::Transport(format!(
                "API error {code} ({reason:?}, retryable={retryable}): {response_text}"
            )));
        }

        let parsed: ChatCompletionResponse = serde_json::from_str(&response_text).map_err(|e| {
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
        ChatCompletionsTransport::execute_streaming(self, model, messages, tools, params)
            .await
            .map_err(|e| HakimiError::Transport(e.to_string()))
    }
}

// ── Wire-format types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
    #[serde(default)]
    usage: Option<ChatCompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatCompletionToolCall>>,
    /// Reasoning content from reasoning models (DeepSeek R1, QwQ, etc.).
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionToolCall {
    id: String,
    function: ChatCompletionFunction,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u32>,
}
