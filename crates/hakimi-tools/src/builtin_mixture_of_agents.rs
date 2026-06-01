use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, redact_sensitive_text};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use tracing::{debug, warn};

use crate::Tool;

const DEFAULT_OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const DEFAULT_REFERENCE_TEMPERATURE: f64 = 0.6;
const DEFAULT_AGGREGATOR_TEMPERATURE: f64 = 0.4;
const DEFAULT_MAX_TOKENS: u32 = 32_000;
const MAX_REFERENCE_MODELS: usize = 8;
const MIN_PROMPT_CHARS: usize = 1;

const DEFAULT_REFERENCE_MODELS: &[&str] = &[
    "anthropic/claude-opus-4.6",
    "google/gemini-2.5-pro",
    "openai/gpt-5.4-pro",
    "deepseek/deepseek-v3.2",
];

const DEFAULT_AGGREGATOR_MODEL: &str = "anthropic/claude-opus-4.6";

const AGGREGATOR_SYSTEM_PROMPT: &str = "You have been provided with a set of responses from various models to the latest user query. Your task is to synthesize these responses into a single, high-quality response. Critically evaluate the information provided, recognizing that some of it may be biased or incorrect. Do not simply replicate the given answers; provide a refined, accurate, and comprehensive reply.\n\nResponses from models:";

/// Built-in Mixture-of-Agents tool backed by OpenRouter chat completions.
pub struct MixtureOfAgentsTool;

#[async_trait]
impl Tool for MixtureOfAgentsTool {
    fn name(&self) -> &str {
        "mixture_of_agents"
    }

    fn toolset(&self) -> &str {
        "moa"
    }

    fn description(&self) -> &str {
        "Route a genuinely hard reasoning, math, coding, or analysis problem through multiple reference LLMs and aggregate their answers. Uses OpenRouter via OPENROUTER_API_KEY and should be used sparingly because it makes multiple model calls."
    }

    fn emoji(&self) -> &str {
        "\u{1f9e0}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "user_prompt": {
                    "type": "string",
                    "description": "The complex query or problem to solve using multiple model perspectives."
                },
                "reference_models": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional OpenRouter model IDs for the reference layer. Defaults to Hakimi's frontier-model set.",
                    "maxItems": MAX_REFERENCE_MODELS
                },
                "aggregator_model": {
                    "type": "string",
                    "description": "Optional OpenRouter model ID for synthesis. Defaults to the strongest configured aggregator."
                },
                "max_tokens": {
                    "type": "integer",
                    "minimum": 256,
                    "maximum": DEFAULT_MAX_TOKENS,
                    "description": "Maximum completion tokens for each model call. Default: 32000."
                }
            },
            "required": ["user_prompt"]
        })
    }

    fn check_available(&self) -> bool {
        openrouter_api_key().is_some()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(64 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let user_prompt = args
            .get("user_prompt")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| s.len() >= MIN_PROMPT_CHARS)
            .ok_or_else(|| HakimiError::Tool("missing required parameter: user_prompt".into()))?;

        let api_key = openrouter_api_key().ok_or_else(|| {
            HakimiError::Tool(
                "OPENROUTER_API_KEY environment variable not set; mixture_of_agents requires OpenRouter."
                    .into(),
            )
        })?;
        let base_url = openrouter_base_url();
        let reference_models = parse_reference_models(args)?;
        let aggregator_model = args
            .get("aggregator_model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_AGGREGATOR_MODEL)
            .to_string();
        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(DEFAULT_MAX_TOKENS)
            .clamp(256, DEFAULT_MAX_TOKENS);

        let started = std::time::Instant::now();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| HakimiError::Tool(format!("failed to create MoA HTTP client: {e}")))?;

        debug!(
            references = reference_models.len(),
            aggregator = %aggregator_model,
            "starting mixture_of_agents"
        );

        let mut tasks = Vec::with_capacity(reference_models.len());
        for model in reference_models.clone() {
            let client = client.clone();
            let api_key = api_key.clone();
            let base_url = base_url.clone();
            let prompt = user_prompt.to_string();
            tasks.push(tokio::spawn(async move {
                run_reference_model(&client, &base_url, &api_key, &model, &prompt, max_tokens).await
            }));
        }

        let mut successful = Vec::new();
        let mut failed = Vec::new();
        for task in tasks {
            match task.await {
                Ok(Ok(response)) => successful.push(response),
                Ok(Err(failure)) => failed.push(failure),
                Err(err) => failed.push(ModelFailure {
                    model: "unknown".to_string(),
                    error: format!("join failure: {err}"),
                }),
            }
        }

        if successful.is_empty() {
            return Ok(json!({
                "success": false,
                "response": "MoA processing failed because every reference model failed.",
                "models_used": {
                    "reference_models": reference_models,
                    "aggregator_model": aggregator_model,
                },
                "failed_models": failed,
            })
            .to_string());
        }

        let aggregator_prompt = construct_aggregator_prompt(
            AGGREGATOR_SYSTEM_PROMPT,
            successful
                .iter()
                .map(|response| response.content.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
        );
        let final_response = run_chat_completion(
            &client,
            &base_url,
            &api_key,
            ChatRequest {
                model: &aggregator_model,
                messages: vec![
                    json!({"role": "system", "content": aggregator_prompt}),
                    json!({"role": "user", "content": user_prompt}),
                ],
                temperature: DEFAULT_AGGREGATOR_TEMPERATURE,
                max_tokens,
            },
        )
        .await?;

        Ok(json!({
            "success": true,
            "response": final_response.content,
            "models_used": {
                "reference_models": reference_models,
                "aggregator_model": aggregator_model,
            },
            "reference_success_count": successful.len(),
            "failed_models": failed,
            "processing_time_ms": started.elapsed().as_millis(),
        })
        .to_string())
    }
}

fn openrouter_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn openrouter_base_url() -> String {
    std::env::var("OPENROUTER_BASE_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_OPENROUTER_BASE_URL.to_string())
}

fn parse_reference_models(args: &JsonValue) -> Result<Vec<String>> {
    let models = args
        .get("reference_models")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .take(MAX_REFERENCE_MODELS)
                .collect::<Vec<_>>()
        })
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_REFERENCE_MODELS
                .iter()
                .map(|s| s.to_string())
                .collect()
        });

    if models.is_empty() {
        return Err(HakimiError::Tool(
            "reference_models must contain at least one model".into(),
        ));
    }
    Ok(models)
}

fn construct_aggregator_prompt(system_prompt: &str, responses: &[&str]) -> String {
    let enumerated = responses
        .iter()
        .enumerate()
        .map(|(index, response)| format!("{}. {}", index + 1, response.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{system_prompt}\n\n{enumerated}")
}

async fn run_reference_model(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    user_prompt: &str,
    max_tokens: u32,
) -> std::result::Result<ModelResponse, ModelFailure> {
    match run_chat_completion(
        client,
        base_url,
        api_key,
        ChatRequest {
            model,
            messages: vec![json!({"role": "user", "content": user_prompt})],
            temperature: DEFAULT_REFERENCE_TEMPERATURE,
            max_tokens,
        },
    )
    .await
    {
        Ok(response) => Ok(response),
        Err(err) => {
            let message = redact_sensitive_text(&err.to_string());
            warn!(model = %model, error = %message, "MoA reference model failed");
            Err(ModelFailure {
                model: model.to_string(),
                error: message,
            })
        }
    }
}

struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<JsonValue>,
    temperature: f64,
    max_tokens: u32,
}

async fn run_chat_completion(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: ChatRequest<'_>,
) -> Result<ModelResponse> {
    let url = chat_completions_endpoint(base_url);
    let body = json!({
        "model": request.model,
        "messages": request.messages,
        "temperature": request.temperature,
        "max_tokens": request.max_tokens,
        "reasoning": {
            "enabled": true,
            "effort": "xhigh"
        }
    });

    let response = client
        .post(&url)
        .bearer_auth(api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("MoA API request failed: {e}")))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read MoA API response: {e}")))?;
    if !status.is_success() {
        return Err(HakimiError::Tool(format!(
            "MoA API returned status {status}: {}",
            redact_sensitive_text(&text)
        )));
    }

    let parsed: ChatCompletionResponse = serde_json::from_str(&text)
        .map_err(|e| HakimiError::Tool(format!("failed to parse MoA response: {e}")))?;
    let choice = parsed
        .choices
        .first()
        .ok_or_else(|| HakimiError::Tool("MoA response contained no choices".into()))?;
    let content = extract_content_or_reasoning(&choice.message).ok_or_else(|| {
        HakimiError::Tool(format!(
            "MoA model '{}' returned empty content",
            request.model
        ))
    })?;

    Ok(ModelResponse { content })
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
    }
}

fn extract_content_or_reasoning(message: &ChatCompletionMessage) -> Option<String> {
    message
        .content
        .as_deref()
        .or(message.reasoning_content.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, serde::Serialize)]
struct ModelResponse {
    content: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ModelFailure {
    model: String,
    error: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn tool_metadata_matches_moa_surface() {
        let tool = MixtureOfAgentsTool;
        assert_eq!(tool.name(), "mixture_of_agents");
        assert_eq!(tool.toolset(), "moa");
        assert!(tool.description().contains("OpenRouter"));
        assert_eq!(tool.emoji(), "\u{1f9e0}");
    }

    #[test]
    fn schema_requires_user_prompt() {
        let tool = MixtureOfAgentsTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("user_prompt")));
        assert_eq!(
            schema["properties"]["reference_models"]["maxItems"],
            MAX_REFERENCE_MODELS
        );
    }

    #[test]
    fn aggregator_prompt_enumerates_responses() {
        let prompt = construct_aggregator_prompt("system", &["alpha", " beta "]);
        assert_eq!(prompt, "system\n\n1. alpha\n2. beta");
    }

    #[test]
    fn parse_reference_models_uses_defaults_when_missing() {
        let models = parse_reference_models(&json!({})).unwrap();
        assert_eq!(models.len(), DEFAULT_REFERENCE_MODELS.len());
        assert_eq!(models[0], DEFAULT_REFERENCE_MODELS[0]);
    }

    #[test]
    fn parse_reference_models_caps_custom_list() {
        let models = parse_reference_models(&json!({
            "reference_models": [
                "m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8", "m9"
            ]
        }))
        .unwrap();
        assert_eq!(models.len(), MAX_REFERENCE_MODELS);
        assert_eq!(models[7], "m8");
    }

    #[test]
    fn extracts_reasoning_when_content_empty() {
        let message = ChatCompletionMessage {
            content: None,
            reasoning_content: Some("hidden answer".to_string()),
        };
        assert_eq!(
            extract_content_or_reasoning(&message).as_deref(),
            Some("hidden answer")
        );
    }

    #[test]
    fn chat_endpoint_does_not_duplicate_v1() {
        assert_eq!(
            chat_completions_endpoint("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("https://openrouter.ai/api"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
    }

    #[test]
    fn check_available_tracks_openrouter_key() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("OPENROUTER_API_KEY");
        }
        let tool = MixtureOfAgentsTool;
        assert!(!tool.check_available());
        unsafe {
            std::env::set_var("OPENROUTER_API_KEY", "test-key");
        }
        assert!(tool.check_available());
        unsafe {
            std::env::remove_var("OPENROUTER_API_KEY");
        }
    }

    #[tokio::test]
    async fn execute_requires_user_prompt_before_api_key() {
        let tool = MixtureOfAgentsTool;
        let result = tool.execute(&json!({}), &ToolContext::default()).await;
        assert!(
            result
                .expect_err("missing user_prompt should fail")
                .to_string()
                .contains("user_prompt")
        );
    }

    #[tokio::test]
    async fn execute_reports_missing_openrouter_key() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("OPENROUTER_API_KEY");
        }
        let tool = MixtureOfAgentsTool;
        let result = tool
            .execute(
                &json!({"user_prompt": "solve this"}),
                &ToolContext::default(),
            )
            .await;
        assert!(
            result
                .expect_err("missing key should fail")
                .to_string()
                .contains("OPENROUTER_API_KEY")
        );
    }
}
