//! Server-initiated MCP sampling support.

use anyhow::{Context, Result as AnyhowResult};
use async_trait::async_trait;
use hakimi_common::{FinishReason, Message, MessageRole};
use hakimi_transports::{ProviderTransport, RequestParams};
use serde_json::Value;
use std::sync::Arc;

use crate::protocol::{
    ContentBlock, CreateMessageParams, CreateMessageResult, JsonRpcError, JsonRpcServerRequest,
    JsonRpcServerResponse,
};

/// Handles MCP server-initiated JSON-RPC requests.
#[async_trait]
pub trait McpServerRequestHandler: Send + Sync {
    async fn handle_request(
        &self,
        request: &JsonRpcServerRequest,
    ) -> std::result::Result<JsonRpcServerResponse, JsonRpcError>;
}

/// Sampling handler backed by Hakimi's configured LLM transport.
pub struct TransportSamplingHandler {
    server_name: String,
    model: String,
    transport: Arc<dyn ProviderTransport>,
}

impl TransportSamplingHandler {
    pub fn new(
        server_name: impl Into<String>,
        model: impl Into<String>,
        transport: Arc<dyn ProviderTransport>,
    ) -> Self {
        Self {
            server_name: server_name.into(),
            model: model.into(),
            transport,
        }
    }

    async fn create_message(
        &self,
        request: &JsonRpcServerRequest,
    ) -> std::result::Result<Value, JsonRpcError> {
        let params: CreateMessageParams =
            serde_json::from_value(request.params.clone().ok_or_else(|| {
                JsonRpcError::invalid_params("sampling/createMessage missing params")
            })?)
            .map_err(|e| JsonRpcError::invalid_params(format!("invalid sampling params: {e}")))?;

        let messages = sampling_messages_to_hakimi(params.system_prompt.as_deref(), &params)?;
        let request_params = RequestParams {
            temperature: params.temperature.map(f64::from),
            max_tokens: Some(params.max_tokens),
            ..Default::default()
        };

        let response = self
            .transport
            .execute(&self.model, &messages, &[], &request_params)
            .await
            .map_err(|e| {
                JsonRpcError::internal(format!(
                    "sampling/createMessage failed for MCP server '{}': {e}",
                    self.server_name
                ))
            })?;

        let result = CreateMessageResult {
            role: "assistant".to_string(),
            content: ContentBlock::Text {
                text: response.content_or_empty().to_string(),
            },
            model: self.model.clone(),
            stop_reason: response.finish_reason.as_ref().map(mcp_stop_reason),
        };

        serde_json::to_value(result)
            .map_err(|e| JsonRpcError::internal(format!("serialize sampling result failed: {e}")))
    }
}

#[async_trait]
impl McpServerRequestHandler for TransportSamplingHandler {
    async fn handle_request(
        &self,
        request: &JsonRpcServerRequest,
    ) -> std::result::Result<JsonRpcServerResponse, JsonRpcError> {
        match request.method.as_str() {
            "sampling/createMessage" => {
                let result = self.create_message(request).await?;
                Ok(JsonRpcServerResponse::success(request.id.clone(), result))
            }
            method => Err(JsonRpcError::method_not_found(method)),
        }
    }
}

pub(crate) fn unsupported_server_request_response(
    request: &JsonRpcServerRequest,
) -> JsonRpcServerResponse {
    JsonRpcServerResponse::error(
        request.id.clone(),
        JsonRpcError::method_not_found(&request.method),
    )
}

pub(crate) async fn handle_server_request(
    handler: Option<&Arc<dyn McpServerRequestHandler>>,
    request: JsonRpcServerRequest,
) -> JsonRpcServerResponse {
    if let Some(handler) = handler {
        match handler.handle_request(&request).await {
            Ok(response) => response,
            Err(error) => JsonRpcServerResponse::error(request.id, error),
        }
    } else {
        unsupported_server_request_response(&request)
    }
}

fn sampling_messages_to_hakimi(
    system_prompt: Option<&str>,
    params: &CreateMessageParams,
) -> std::result::Result<Vec<Message>, JsonRpcError> {
    let mut messages = Vec::new();
    if let Some(system_prompt) = system_prompt.filter(|s| !s.trim().is_empty()) {
        messages.push(Message::system(system_prompt));
    }

    for message in &params.messages {
        let content = extract_sampling_text(&message.content).map_err(|e| {
            JsonRpcError::invalid_params(format!("unsupported sampling content: {e}"))
        })?;
        messages.push(match message.role.as_str() {
            "assistant" => Message::assistant(content),
            "system" => Message::system(content),
            "user" => Message::user(content),
            "tool" => Message {
                role: MessageRole::Tool,
                content: Some(content),
                images: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning: None,
                reasoning_content: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            },
            role => {
                return Err(JsonRpcError::invalid_params(format!(
                    "unsupported sampling role '{role}'"
                )));
            }
        });
    }

    Ok(messages)
}

fn extract_sampling_text(value: &Value) -> AnyhowResult<String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_string());
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Ok(text.to_string());
    }
    if let Some(items) = value.as_array() {
        let mut parts = Vec::new();
        for item in items {
            if item.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(text);
                }
            }
        }
        return Ok(parts.join("\n"));
    }
    if value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "text")
    {
        anyhow::bail!("only text content blocks are supported");
    }
    serde_json::to_string(value).context("serialize sampling content")
}

fn mcp_stop_reason(reason: &FinishReason) -> String {
    match reason {
        FinishReason::Stop => "endTurn",
        FinishReason::ToolCalls => "toolUse",
        FinishReason::Length => "maxTokens",
        FinishReason::ContentFilter | FinishReason::Error => "stop",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{JsonRpcServerRequest, SamplingMessage};
    use serde_json::json;

    #[test]
    fn converts_sampling_text_messages() {
        let params = CreateMessageParams {
            system_prompt: Some("be concise".to_string()),
            messages: vec![SamplingMessage {
                role: "user".to_string(),
                content: json!({"type": "text", "text": "hello"}),
            }],
            max_tokens: 32,
            temperature: Some(0.2),
            model_preferences: None,
            tools: None,
        };

        let messages = sampling_messages_to_hakimi(params.system_prompt.as_deref(), &params)
            .expect("convert messages");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(messages[0].content.as_deref(), Some("be concise"));
        assert_eq!(messages[1].role, MessageRole::User);
        assert_eq!(messages[1].content.as_deref(), Some("hello"));
    }

    #[test]
    fn rejects_non_text_sampling_blocks() {
        let params = CreateMessageParams {
            system_prompt: None,
            messages: vec![SamplingMessage {
                role: "user".to_string(),
                content: json!({"type": "image", "data": "...", "mimeType": "image/png"}),
            }],
            max_tokens: 32,
            temperature: None,
            model_preferences: None,
            tools: None,
        };

        let err = sampling_messages_to_hakimi(None, &params).unwrap_err();

        assert!(err.message.contains("unsupported sampling content"));
    }

    #[tokio::test]
    async fn unsupported_request_returns_method_not_found() {
        let request = JsonRpcServerRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(7),
            method: "sampling/createMessage".to_string(),
            params: Some(json!({})),
        };

        let response = handle_server_request(None, request).await;

        assert_eq!(response.id, json!(7));
        let error = response.error.expect("error response");
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("sampling/createMessage"));
    }
}
