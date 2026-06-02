//! Server-initiated MCP sampling support.

use anyhow::{Context, Result as AnyhowResult};
use async_trait::async_trait;
use hakimi_common::{
    FinishReason, Message, MessageRole, NormalizedResponse, ToolCall, ToolDefinition,
};
use hakimi_transports::{ProviderTransport, RequestParams};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::protocol::{
    ContentBlock, CreateMessageParams, CreateMessageResult, JsonRpcError, JsonRpcServerRequest,
    JsonRpcServerResponse, McpToolDefinition, SamplingMessage, SamplingResultContent,
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
        let tools = sampling_tools_to_hakimi(params.tools.as_deref());
        let request_params = RequestParams {
            temperature: params.temperature.map(f64::from),
            max_tokens: Some(params.max_tokens),
            ..Default::default()
        };

        let response = self
            .transport
            .execute(&self.model, &messages, &tools, &request_params)
            .await
            .map_err(|e| {
                JsonRpcError::internal(format!(
                    "sampling/createMessage failed for MCP server '{}': {e}",
                    self.server_name
                ))
            })?;

        let result = create_message_result(&self.model, &response);

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
        append_sampling_message(message, &mut messages)?;
    }

    Ok(messages)
}

fn append_sampling_message(
    message: &SamplingMessage,
    messages: &mut Vec<Message>,
) -> std::result::Result<(), JsonRpcError> {
    let items = if let Some(items) = message.content.as_array() {
        items.iter().collect::<Vec<_>>()
    } else {
        vec![&message.content]
    };

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();

    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                tool_calls.push(extract_tool_use_call(item, tool_calls.len()).map_err(|e| {
                    JsonRpcError::invalid_params(format!("unsupported sampling content: {e}"))
                })?);
            }
            Some("tool_result") => {
                tool_results.push(extract_tool_result_message(item).map_err(|e| {
                    JsonRpcError::invalid_params(format!("unsupported sampling content: {e}"))
                })?);
            }
            Some("text") | None => {
                let text = extract_sampling_text(item).map_err(|e| {
                    JsonRpcError::invalid_params(format!("unsupported sampling content: {e}"))
                })?;
                if !text.is_empty() {
                    text_parts.push(text);
                }
            }
            Some(kind) => {
                return Err(JsonRpcError::invalid_params(format!(
                    "unsupported sampling content: only text, tool_use, and tool_result blocks are supported (got '{kind}')"
                )));
            }
        }
    }

    messages.extend(tool_results);

    let text = text_parts.join("\n");
    if !tool_calls.is_empty() {
        messages.push(Message {
            role: MessageRole::Assistant,
            content: (!text.is_empty()).then_some(text),
            images: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        });
    } else if !text.is_empty() {
        messages.push(text_message_for_role(&message.role, text)?);
    }

    Ok(())
}

fn text_message_for_role(
    role: &str,
    content: String,
) -> std::result::Result<Message, JsonRpcError> {
    Ok(match role {
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
    })
}

fn extract_tool_use_call(value: &Value, index: usize) -> AnyhowResult<ToolCall> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("call_{index}"));
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .context("tool_use block missing name")?
        .to_string();
    let input = value.get("input").cloned().unwrap_or_else(|| json!({}));
    let arguments = serde_json::to_string(&input).context("serialize tool_use input")?;

    Ok(ToolCall {
        id,
        name,
        arguments,
        index: Some(index as u32),
    })
}

fn extract_tool_result_message(value: &Value) -> AnyhowResult<Message> {
    let tool_call_id = value
        .get("toolUseId")
        .or_else(|| value.get("tool_use_id"))
        .and_then(Value::as_str)
        .context("tool_result block missing toolUseId")?;
    let content = value
        .get("content")
        .map(extract_sampling_text)
        .transpose()?
        .unwrap_or_default();

    Ok(Message::tool_result(tool_call_id, "", content))
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
            if item.get("type").and_then(Value::as_str) == Some("text")
                && let Some(text) = item.get("text").and_then(Value::as_str)
            {
                parts.push(text);
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

fn sampling_tools_to_hakimi(tools: Option<&[McpToolDefinition]>) -> Vec<ToolDefinition> {
    tools
        .unwrap_or_default()
        .iter()
        .map(|tool| ToolDefinition {
            name: tool.name.clone(),
            description: tool.description.clone().unwrap_or_default(),
            parameters: tool.input_schema.clone(),
            toolset: "mcp_sampling".to_string(),
        })
        .collect()
}

fn create_message_result(model: &str, response: &NormalizedResponse) -> CreateMessageResult {
    let content = if response.has_tool_calls() {
        let mut blocks = Vec::new();
        if let Some(text) = response.content.as_deref().filter(|text| !text.is_empty()) {
            blocks.push(ContentBlock::Text {
                text: text.to_string(),
            });
        }
        if let Some(tool_calls) = &response.tool_calls {
            for call in tool_calls {
                blocks.push(ContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: parse_tool_call_arguments(&call.arguments),
                });
            }
        }
        SamplingResultContent::Blocks(blocks)
    } else {
        SamplingResultContent::Block(ContentBlock::Text {
            text: response.content_or_empty().to_string(),
        })
    };

    CreateMessageResult {
        role: "assistant".to_string(),
        content,
        model: model.to_string(),
        stop_reason: mcp_stop_reason_for_response(response),
    }
}

fn parse_tool_call_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({ "_raw": arguments }))
}

fn mcp_stop_reason_for_response(response: &NormalizedResponse) -> Option<String> {
    if response.has_tool_calls() {
        return Some("toolUse".to_string());
    }
    response.finish_reason.as_ref().map(mcp_stop_reason)
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
    fn converts_sampling_tool_blocks_to_hakimi_messages() {
        let params = CreateMessageParams {
            system_prompt: None,
            messages: vec![
                SamplingMessage {
                    role: "assistant".to_string(),
                    content: json!([
                        {"type": "text", "text": "I need a file."},
                        {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "README.md"}}
                    ]),
                },
                SamplingMessage {
                    role: "user".to_string(),
                    content: json!([
                        {"type": "tool_result", "toolUseId": "call_1", "content": [{"type": "text", "text": "contents"}]}
                    ]),
                },
            ],
            max_tokens: 32,
            temperature: None,
            model_preferences: None,
            tools: None,
        };

        let messages = sampling_messages_to_hakimi(None, &params).expect("convert messages");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::Assistant);
        assert_eq!(messages[0].content.as_deref(), Some("I need a file."));
        let call = &messages[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(call.id, "call_1");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments, r#"{"path":"README.md"}"#);
        assert_eq!(messages[1].role, MessageRole::Tool);
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(messages[1].content.as_deref(), Some("contents"));
    }

    #[test]
    fn converts_sampling_tools_to_hakimi_definitions() {
        let tools = vec![crate::protocol::McpToolDefinition {
            name: "search".to_string(),
            description: Some("Search documents".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }),
        }];

        let converted = sampling_tools_to_hakimi(Some(&tools));

        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "search");
        assert_eq!(converted[0].description, "Search documents");
        assert_eq!(converted[0].toolset, "mcp_sampling");
        assert_eq!(
            converted[0].parameters["properties"]["query"]["type"],
            "string"
        );
    }

    #[test]
    fn builds_tool_use_sampling_result_from_model_tool_calls() {
        let response = NormalizedResponse {
            content: Some("I will call a tool.".to_string()),
            tool_calls: Some(vec![ToolCall {
                id: "call_7".to_string(),
                name: "search".to_string(),
                arguments: r#"{"query":"mcp"}"#.to_string(),
                index: Some(0),
            }]),
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
            reasoning: None,
        };

        let result = create_message_result("test-model", &response);
        let value = serde_json::to_value(result).unwrap();

        assert_eq!(value["role"], "assistant");
        assert_eq!(value["model"], "test-model");
        assert_eq!(value["stopReason"], "toolUse");
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][1]["type"], "tool_use");
        assert_eq!(value["content"][1]["id"], "call_7");
        assert_eq!(value["content"][1]["name"], "search");
        assert_eq!(value["content"][1]["input"]["query"], "mcp");
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
