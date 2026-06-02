//! AWS Bedrock Converse API transport.
//!
//! This is the Rust-native counterpart to Hermes' `agent/transports/bedrock.py`
//! and `agent/bedrock_adapter.py`: it maps Hakimi's normalized messages/tools
//! into Bedrock Converse JSON, signs the HTTPS request with AWS Signature V4,
//! and normalizes the response back into `NormalizedResponse`.

use async_trait::async_trait;
use chrono::{Datelike, Timelike, Utc};
use futures::stream::Stream;
use hakimi_common::{
    ApiMode, FinishReason, HakimiError, ImageContent, Message, MessageRole, NormalizedResponse,
    Result, ToolCall, ToolDefinition, Usage,
};
use reqwest::Client;
use ring::{digest, hmac};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use std::pin::Pin;
use tracing::{debug, warn};

use crate::params::RequestParams;
use crate::rate_limit::RateLimitState;
use crate::streaming::StreamEvent;
use crate::trait_def::ProviderTransport;

const BEDROCK_SERVICE: &str = "bedrock";
const DEFAULT_REGION: &str = "us-east-1";
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// AWS credentials used by the Bedrock transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl AwsCredentials {
    fn from_env() -> Result<Self> {
        let access_key_id = read_env("AWS_ACCESS_KEY_ID").ok_or_else(|| {
            HakimiError::Transport(
                "Bedrock requires AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY; AWS profile/IMDS credential-chain support is a follow-up gap".to_string(),
            )
        })?;
        let secret_access_key = read_env("AWS_SECRET_ACCESS_KEY").ok_or_else(|| {
            HakimiError::Transport(
                "Bedrock requires AWS_SECRET_ACCESS_KEY with AWS_ACCESS_KEY_ID".to_string(),
            )
        })?;
        let session_token = read_env("AWS_SESSION_TOKEN");
        Ok(Self {
            access_key_id,
            secret_access_key,
            session_token,
        })
    }
}

/// Transport for AWS Bedrock Runtime Converse.
pub struct BedrockConverseTransport {
    region: String,
    client: Client,
    credentials: AwsCredentials,
    base_url: Option<String>,
}

impl BedrockConverseTransport {
    pub fn from_env(
        region: Option<String>,
        base_url: Option<String>,
        client: Client,
    ) -> Result<Self> {
        let region = normalize_region(region);
        let credentials = AwsCredentials::from_env()?;
        Ok(Self::new(region, credentials, base_url, client))
    }

    pub fn new(
        region: String,
        credentials: AwsCredentials,
        base_url: Option<String>,
        client: Client,
    ) -> Self {
        Self {
            region: normalize_region(Some(region)),
            client,
            credentials,
            base_url: base_url.and_then(normalize_base_url),
        }
    }

    fn endpoint(&self, model: &str) -> String {
        let encoded_model = aws_uri_encode(model, false);
        if let Some(base_url) = self.base_url.as_deref() {
            format!(
                "{}/model/{encoded_model}/converse",
                base_url.trim_end_matches('/')
            )
        } else {
            format!(
                "https://bedrock-runtime.{}.amazonaws.com/model/{encoded_model}/converse",
                self.region
            )
        }
    }

    fn build_request(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> JsonValue {
        build_converse_request(model, messages, tools, params)
    }

    fn parse_response(resp: &BedrockConverseResponse) -> NormalizedResponse {
        parse_converse_response(resp)
    }

    fn signed_headers(
        &self,
        url: &str,
        body: &[u8],
        now: chrono::DateTime<Utc>,
    ) -> Result<SignedHeaders> {
        sign_bedrock_request(url, body, &self.region, &self.credentials, now)
    }
}

#[async_trait]
impl ProviderTransport for BedrockConverseTransport {
    fn api_mode(&self) -> ApiMode {
        ApiMode::BedrockConverse
    }

    fn provider_name(&self) -> &str {
        "bedrock"
    }

    fn rate_limits(&self) -> Option<RateLimitState> {
        None
    }

    async fn execute(
        &self,
        model: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        params: &RequestParams,
    ) -> Result<NormalizedResponse> {
        let body = self.build_request(model, messages, tools, params);
        let body_bytes = serde_json::to_vec(&body).map_err(|err| {
            HakimiError::Transport(format!("failed to encode Bedrock request: {err}"))
        })?;
        let url = self.endpoint(model);
        let signed = self.signed_headers(&url, &body_bytes, Utc::now())?;

        debug!(url = %url, model = model, region = %self.region, "sending Bedrock Converse request");

        let mut request = self
            .client
            .post(&url)
            .header("Authorization", signed.authorization)
            .header("Content-Type", "application/json")
            .header("Host", signed.host)
            .header("X-Amz-Date", signed.amz_date)
            .header("X-Amz-Content-Sha256", signed.payload_hash)
            .body(body_bytes);

        if let Some(token) = signed.session_token {
            request = request.header("X-Amz-Security-Token", token);
        }

        let response = request.send().await.map_err(|err| {
            warn!(error = %err, "Bedrock HTTP request failed");
            HakimiError::Transport(format!("Bedrock HTTP request failed: {err}"))
        })?;

        let status = response.status();
        let response_text = response.text().await.map_err(|err| {
            HakimiError::Transport(format!("failed to read Bedrock response body: {err}"))
        })?;

        if !status.is_success() {
            let code = status.as_u16();
            warn!(status = code, body = %response_text, "Bedrock Converse returned error");
            return Err(HakimiError::Transport(format!(
                "Bedrock Converse API error {code}: {response_text}"
            )));
        }

        let parsed: BedrockConverseResponse =
            serde_json::from_str(&response_text).map_err(|err| {
                warn!(error = %err, "failed to parse Bedrock response JSON");
                HakimiError::Transport(format!("failed to parse Bedrock response: {err}"))
            })?;
        Ok(Self::parse_response(&parsed))
    }

    async fn execute_streaming(
        &self,
        _model: &str,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _params: &RequestParams,
    ) -> Result<Pin<Box<dyn Stream<Item = std::result::Result<StreamEvent, String>> + Send>>> {
        Err(HakimiError::Transport(
            "Bedrock Converse streaming is not implemented yet; set display.streaming=false or use non-streaming mode".to_string(),
        ))
    }
}

fn normalize_region(region: Option<String>) -> String {
    region
        .or_else(|| read_env("AWS_REGION"))
        .or_else(|| read_env("AWS_DEFAULT_REGION"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REGION.to_string())
}

fn normalize_base_url(value: String) -> Option<String> {
    let trimmed = value.trim().trim_end_matches('/').to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn read_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn build_converse_request(
    model: &str,
    messages: &[Message],
    tools: &[ToolDefinition],
    params: &RequestParams,
) -> JsonValue {
    let (system, converse_messages) = convert_messages_to_converse(messages);
    let mut body = json!({
        "messages": converse_messages,
        "inferenceConfig": {
            "maxTokens": params.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        },
    });

    if let Some(system) = system {
        body["system"] = JsonValue::Array(system);
    }
    if let Some(temperature) = params.temperature {
        body["inferenceConfig"]["temperature"] = json!(temperature);
    }
    if let Some(top_p) = params.top_p {
        body["inferenceConfig"]["topP"] = json!(top_p);
    }
    if let Some(stop) = params.stop.as_ref()
        && !stop.is_empty()
    {
        body["inferenceConfig"]["stopSequences"] = json!(stop);
    }
    if !tools.is_empty() && model_supports_tool_use(model) {
        let converted = convert_tools_to_converse(tools);
        if !converted.is_empty() {
            body["toolConfig"] = json!({ "tools": converted });
        }
    }

    body
}

fn convert_tools_to_converse(tools: &[ToolDefinition]) -> Vec<JsonValue> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "toolSpec": {
                    "name": tool.name,
                    "description": tool.description,
                    "inputSchema": {
                        "json": tool.parameters,
                    },
                }
            })
        })
        .collect()
}

fn convert_messages_to_converse(messages: &[Message]) -> (Option<Vec<JsonValue>>, Vec<JsonValue>) {
    let mut system_blocks = Vec::new();
    let mut converse_messages: Vec<JsonValue> = Vec::new();

    for message in messages {
        match message.role {
            MessageRole::System => {
                if let Some(content) = non_empty_text_block(message.content.as_deref()) {
                    system_blocks.push(content);
                }
            }
            MessageRole::User => {
                let content_blocks = convert_content_to_converse(message);
                push_or_merge_message(&mut converse_messages, "user", content_blocks);
            }
            MessageRole::Assistant => {
                let mut content_blocks = Vec::new();
                if let Some(text) = message.content.as_deref()
                    && !text.trim().is_empty()
                {
                    content_blocks.push(json!({ "text": text }));
                }
                if let Some(tool_calls) = message.tool_calls.as_ref() {
                    for tool_call in tool_calls {
                        let input = serde_json::from_str::<JsonValue>(&tool_call.arguments)
                            .unwrap_or_else(|_| json!({}));
                        content_blocks.push(json!({
                            "toolUse": {
                                "toolUseId": tool_call.id,
                                "name": tool_call.name,
                                "input": input,
                            }
                        }));
                    }
                }
                if content_blocks.is_empty() {
                    content_blocks.push(json!({ "text": " " }));
                }
                push_or_merge_message(&mut converse_messages, "assistant", content_blocks);
            }
            MessageRole::Tool => {
                let content = message.content.as_deref().unwrap_or("");
                let tool_call_id = message.tool_call_id.as_deref().unwrap_or("");
                let block = json!({
                    "toolResult": {
                        "toolUseId": tool_call_id,
                        "content": [{ "text": if content.trim().is_empty() { " " } else { content } }],
                    }
                });
                push_or_merge_message(&mut converse_messages, "user", vec![block]);
            }
        }
    }

    if converse_messages
        .first()
        .and_then(|msg| msg.get("role"))
        .and_then(JsonValue::as_str)
        .is_some_and(|role| role != "user")
    {
        converse_messages.insert(0, json!({ "role": "user", "content": [{ "text": " " }] }));
    }
    if converse_messages
        .last()
        .and_then(|msg| msg.get("role"))
        .and_then(JsonValue::as_str)
        .is_some_and(|role| role != "user")
    {
        converse_messages.push(json!({ "role": "user", "content": [{ "text": " " }] }));
    }

    let system = (!system_blocks.is_empty()).then_some(system_blocks);
    (system, converse_messages)
}

fn non_empty_text_block(content: Option<&str>) -> Option<JsonValue> {
    content
        .filter(|text| !text.trim().is_empty())
        .map(|text| json!({ "text": text }))
}

fn convert_content_to_converse(message: &Message) -> Vec<JsonValue> {
    let mut blocks = Vec::new();
    if let Some(text) = message.content.as_deref() {
        blocks.push(json!({ "text": if text.trim().is_empty() { " " } else { text } }));
    }
    if let Some(images) = message.images.as_ref() {
        for image in images {
            blocks.push(image_to_converse(image));
        }
    }
    if blocks.is_empty() {
        blocks.push(json!({ "text": " " }));
    }
    blocks
}

fn image_to_converse(image: &ImageContent) -> JsonValue {
    let format = image
        .mime_type
        .split('/')
        .nth(1)
        .unwrap_or("jpeg")
        .split(';')
        .next()
        .unwrap_or("jpeg");
    json!({
        "image": {
            "format": format,
            "source": {
                "bytes": image.data,
            },
        }
    })
}

fn push_or_merge_message(
    messages: &mut Vec<JsonValue>,
    role: &str,
    content_blocks: Vec<JsonValue>,
) {
    if let Some(last) = messages.last_mut()
        && last
            .get("role")
            .and_then(JsonValue::as_str)
            .is_some_and(|last_role| last_role == role)
        && let Some(existing) = last.get_mut("content").and_then(JsonValue::as_array_mut)
    {
        existing.extend(content_blocks);
        return;
    }
    messages.push(json!({ "role": role, "content": content_blocks }));
}

fn model_supports_tool_use(model_id: &str) -> bool {
    let model = model_id.to_ascii_lowercase();
    ![
        "deepseek.r1",
        "deepseek-r1",
        "stability.",
        "cohere.embed",
        "amazon.titan-embed",
    ]
    .iter()
    .any(|pattern| model.contains(pattern))
}

fn parse_converse_response(resp: &BedrockConverseResponse) -> NormalizedResponse {
    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut tool_calls = Vec::new();

    if let Some(message) = resp.output.message.as_ref() {
        for block in &message.content {
            if let Some(text) = block.text.as_ref()
                && !text.is_empty()
            {
                text_parts.push(text.clone());
            }
            if let Some(reasoning) = block.reasoning_content.as_ref()
                && let Some(text) = reasoning.text.as_ref()
                && !text.is_empty()
            {
                reasoning_parts.push(text.clone());
            }
            if let Some(tool_use) = block.tool_use.as_ref() {
                let arguments =
                    serde_json::to_string(&tool_use.input).unwrap_or_else(|_| "{}".to_string());
                tool_calls.push(ToolCall {
                    id: tool_use.tool_use_id.clone(),
                    name: tool_use.name.clone(),
                    arguments,
                    index: None,
                });
            }
        }
    }

    let content = (!text_parts.is_empty()).then(|| text_parts.join("\n"));
    let tool_calls = (!tool_calls.is_empty()).then_some(tool_calls);
    let reasoning = (!reasoning_parts.is_empty()).then(|| reasoning_parts.join("\n\n"));
    let finish_reason = map_stop_reason(resp.stop_reason.as_deref(), tool_calls.is_some());
    let usage = resp.usage.as_ref().map(|usage| Usage {
        prompt_tokens: usage.input_tokens.unwrap_or(0),
        completion_tokens: usage.output_tokens.unwrap_or(0),
        total_tokens: usage
            .total_tokens
            .unwrap_or_else(|| usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0)),
        cached_tokens: usage.cache_read_input_tokens.unwrap_or(0),
        reasoning_tokens: 0,
    });

    NormalizedResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
        reasoning,
    }
}

fn map_stop_reason(stop_reason: Option<&str>, has_tool_calls: bool) -> Option<FinishReason> {
    let mapped = match stop_reason {
        Some("end_turn" | "stop_sequence") => FinishReason::Stop,
        Some("tool_use") => FinishReason::ToolCalls,
        Some("max_tokens") => FinishReason::Length,
        Some("content_filtered" | "guardrail_intervened") => FinishReason::ContentFilter,
        Some(_) => FinishReason::Stop,
        None if has_tool_calls => FinishReason::ToolCalls,
        None => return None,
    };
    if has_tool_calls && mapped == FinishReason::Stop {
        Some(FinishReason::ToolCalls)
    } else {
        Some(mapped)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockConverseResponse {
    #[serde(default)]
    output: BedrockOutput,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<BedrockUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct BedrockOutput {
    #[serde(default)]
    message: Option<BedrockMessage>,
}

#[derive(Debug, Deserialize)]
struct BedrockMessage {
    #[serde(default)]
    content: Vec<BedrockContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockContentBlock {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    reasoning_content: Option<BedrockReasoningContent>,
    #[serde(default)]
    tool_use: Option<BedrockToolUse>,
}

#[derive(Debug, Deserialize)]
struct BedrockReasoningContent {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockToolUse {
    tool_use_id: String,
    name: String,
    #[serde(default)]
    input: JsonValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SignedHeaders {
    authorization: String,
    host: String,
    amz_date: String,
    payload_hash: String,
    session_token: Option<String>,
}

fn sign_bedrock_request(
    url: &str,
    body: &[u8],
    region: &str,
    credentials: &AwsCredentials,
    now: chrono::DateTime<Utc>,
) -> Result<SignedHeaders> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| HakimiError::Transport(format!("invalid Bedrock URL `{url}`: {err}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| HakimiError::Transport(format!("Bedrock URL has no host: {url}")))?
        .to_string();
    let host_header = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    };
    let canonical_uri = if parsed.path().is_empty() {
        "/".to_string()
    } else {
        parsed
            .path_segments()
            .map(|segments| {
                format!(
                    "/{}",
                    segments
                        .map(|segment| aws_uri_encode(segment, true))
                        .collect::<Vec<_>>()
                        .join("/")
                )
            })
            .unwrap_or_else(|| parsed.path().to_string())
    };
    let canonical_query = canonical_query_string(&parsed);
    let amz_date = aws_amz_date(now);
    let short_date = aws_short_date(now);
    let payload_hash = sha256_hex(body);
    let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
    let canonical_headers = format!(
        "content-type:application/json\nhost:{host_header}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
    );
    let canonical_request = format!(
        "POST\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let credential_scope = format!("{short_date}/{region}/{BEDROCK_SERVICE}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signing_key = sigv4_signing_key(&credentials.secret_access_key, &short_date, region);
    let signature =
        hex_lower(hmac_sha256(signing_key.as_ref(), string_to_sign.as_bytes()).as_ref());
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        credentials.access_key_id
    );
    Ok(SignedHeaders {
        authorization,
        host: host_header,
        amz_date,
        payload_hash,
        session_token: credentials.session_token.clone(),
    })
}

fn canonical_query_string(url: &reqwest::Url) -> String {
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| (aws_uri_encode(&key, true), aws_uri_encode(&value, true)))
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn sigv4_signing_key(secret: &str, short_date: &str, region: &str) -> hmac::Tag {
    let date_key = hmac_sha256(format!("AWS4{secret}").as_bytes(), short_date.as_bytes());
    let region_key = hmac_sha256(date_key.as_ref(), region.as_bytes());
    let service_key = hmac_sha256(region_key.as_ref(), BEDROCK_SERVICE.as_bytes());
    hmac_sha256(service_key.as_ref(), b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> hmac::Tag {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, data)
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = digest::digest(&digest::SHA256, data);
    hex_lower(digest.as_ref())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn aws_amz_date(now: chrono::DateTime<Utc>) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn aws_short_date(now: chrono::DateTime<Utc>) -> String {
    format!("{:04}{:02}{:02}", now.year(), now.month(), now.day())
}

fn aws_uri_encode(value: &str, encode_slash: bool) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~');
        if keep || (!encode_slash && ch == '/') {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_definition() -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            toolset: "core".to_string(),
        }
    }

    #[test]
    fn bedrock_request_maps_system_tools_and_alternation() {
        let assistant = Message {
            role: MessageRole::Assistant,
            content: Some("I will inspect it.".to_string()),
            images: None,
            tool_calls: Some(vec![ToolCall {
                id: "toolu_1".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path":"README.md"}"#.to_string(),
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
        let messages = vec![
            Message::system("You are Hakimi."),
            Message::user("Read the readme."),
            assistant,
            Message::tool_result("toolu_1", "read_file", "hello"),
        ];
        let body = build_converse_request(
            "anthropic.claude-3-5-sonnet-20240620-v1:0",
            &messages,
            &[tool_definition()],
            &RequestParams {
                max_tokens: Some(128),
                temperature: Some(0.2),
                top_p: Some(0.9),
                stop: Some(vec!["END".to_string()]),
                stream: false,
            },
        );

        assert_eq!(body["system"][0]["text"], "You are Hakimi.");
        assert_eq!(body["inferenceConfig"]["maxTokens"], 128);
        assert_eq!(body["inferenceConfig"]["temperature"], 0.2);
        assert_eq!(body["inferenceConfig"]["topP"], 0.9);
        assert_eq!(body["inferenceConfig"]["stopSequences"][0], "END");
        assert_eq!(
            body["toolConfig"]["tools"][0]["toolSpec"]["name"],
            "read_file"
        );
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(
            body["messages"][1]["content"][1]["toolUse"]["input"]["path"],
            "README.md"
        );
        assert_eq!(body["messages"][2]["role"], "user");
        assert_eq!(
            body["messages"][2]["content"][0]["toolResult"]["toolUseId"],
            "toolu_1"
        );
    }

    #[test]
    fn bedrock_request_strips_tools_for_known_non_tool_models() {
        let body = build_converse_request(
            "deepseek.r1-v1:0",
            &[Message::user("hi")],
            &[tool_definition()],
            &RequestParams::default(),
        );
        assert!(body.get("toolConfig").is_none());
    }

    #[test]
    fn bedrock_response_maps_text_tool_reasoning_usage_and_stop_reason() {
        let raw = json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [
                        { "reasoningContent": { "text": "thinking" } },
                        { "text": "Use this." },
                        { "toolUse": { "toolUseId": "toolu_1", "name": "read_file", "input": { "path": "README.md" } } }
                    ]
                }
            },
            "stopReason": "tool_use",
            "usage": { "inputTokens": 12, "outputTokens": 7, "totalTokens": 19, "cacheReadInputTokens": 3 }
        });
        let parsed: BedrockConverseResponse = serde_json::from_value(raw).expect("parse");
        let response = parse_converse_response(&parsed);

        assert_eq!(response.content.as_deref(), Some("Use this."));
        assert_eq!(response.reasoning.as_deref(), Some("thinking"));
        assert_eq!(response.finish_reason, Some(FinishReason::ToolCalls));
        let calls = response.tool_calls.expect("tool calls");
        assert_eq!(calls[0].id, "toolu_1");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, r#"{"path":"README.md"}"#);
        let usage = response.usage.expect("usage");
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 19);
        assert_eq!(usage.cached_tokens, 3);
    }

    #[test]
    fn bedrock_sigv4_signing_sets_expected_headers() {
        let credentials = AwsCredentials {
            access_key_id: "AKIDEXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
            session_token: Some("session".to_string()),
        };
        let now = chrono::DateTime::parse_from_rfc3339("2015-08-30T12:36:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let signed = sign_bedrock_request(
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-3-sonnet-20240229-v1%3A0/converse",
            br#"{"messages":[]}"#,
            "us-east-1",
            &credentials,
            now,
        )
        .expect("signed");

        assert_eq!(signed.host, "bedrock-runtime.us-east-1.amazonaws.com");
        assert_eq!(signed.amz_date, "20150830T123600Z");
        assert_eq!(signed.session_token.as_deref(), Some("session"));
        assert!(signed.authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/bedrock/aws4_request"
        ));
        assert!(
            signed
                .authorization
                .contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date")
        );
        assert_eq!(signed.payload_hash, sha256_hex(br#"{"messages":[]}"#));
    }
}
