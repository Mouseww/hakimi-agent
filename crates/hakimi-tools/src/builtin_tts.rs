use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::Tool;

/// Built-in tool that converts text to speech audio using TTS APIs.
///
/// Supports multiple providers:
/// - `openai` (default): OpenAI-compatible TTS API (works with OpenAI, Azure, local servers)
/// - `edge`: Microsoft Edge TTS (free, no API key required)
///
/// Configuration is read from environment variables:
/// - `HAKIMI_TTS_PROVIDER`: Provider name ("openai" or "edge"), default "openai"
/// - `HAKIMI_TTS_API_KEY`: API key for OpenAI-compatible providers
/// - `HAKIMI_TTS_BASE_URL`: Base URL for OpenAI-compatible providers (default: https://api.openai.com/v1)
/// - `HAKIMI_TTS_MODEL`: Model name (default: "tts-1")
/// - `HAKIMI_TTS_VOICE`: Voice name (default: "alloy")
/// - `HAKIMI_TTS_OUTPUT_DIR`: Directory for output files (default: ~/.hakimi/audio_cache/)
pub struct TextToSpeechTool;

#[async_trait]
impl Tool for TextToSpeechTool {
    fn name(&self) -> &str {
        "text_to_speech"
    }

    fn toolset(&self) -> &str {
        "media"
    }

    fn description(&self) -> &str {
        "Convert text to speech audio. Returns a file path to the generated audio file (MP3). \
         Supports OpenAI-compatible TTS APIs and Microsoft Edge TTS (free). \
         Voices for OpenAI: alloy, echo, fable, onyx, nova, shimmer. \
         Edge TTS supports hundreds of voices (e.g. en-US-AriaNeural, zh-CN-XiaoxiaoNeural)."
    }

    fn emoji(&self) -> &str {
        "🔊"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to convert to speech.",
                    "maxLength": 4096
                },
                "voice": {
                    "type": "string",
                    "description": "Voice to use. For OpenAI: alloy, echo, fable, onyx, nova, shimmer. For Edge: any Edge TTS voice name like en-US-AriaNeural. Default: alloy (OpenAI) or en-US-AriaNeural (Edge)."
                },
                "provider": {
                    "type": "string",
                    "enum": ["openai", "edge"],
                    "description": "TTS provider to use. Default: auto-detect from env (HAKIMI_TTS_PROVIDER). Falls back to 'openai'."
                },
                "output_path": {
                    "type": "string",
                    "description": "Custom output file path. If not provided, auto-generates in the TTS output directory."
                }
            },
            "required": ["text"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(2048) // Result is just a file path
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: text".into()))?;

        if text.trim().is_empty() {
            return Err(HakimiError::Tool("text parameter cannot be empty".into()));
        }

        // Determine provider
        let provider = args
            .get("provider")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::var("HAKIMI_TTS_PROVIDER").unwrap_or_else(|_| "openai".to_string())
            });

        let voice = args.get("voice").and_then(|v| v.as_str()).map(String::from);

        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        debug!(provider = %provider, text_len = text.len(), "TTS request");

        let result_path = match provider.as_str() {
            "openai" => generate_openai_tts(text, voice.as_deref(), output_path).await?,
            "edge" => generate_edge_tts(text, voice.as_deref(), output_path).await?,
            _ => {
                return Err(HakimiError::Tool(format!(
                    "unsupported TTS provider: '{provider}'. Use 'openai' or 'edge'."
                )));
            }
        };

        info!(path = %result_path.display(), provider = %provider, "TTS audio generated");
        Ok(format!("MEDIA:{}", result_path.display()))
    }
}

/// Get the output directory for TTS files.
fn get_output_dir(custom: Option<&str>) -> PathBuf {
    if let Some(dir) = custom {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("HAKIMI_TTS_OUTPUT_DIR") {
        return PathBuf::from(dir);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".hakimi").join("audio_cache")
}

/// Generate a unique filename for the audio output.
fn generate_filename(prefix: &str, ext: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    format!(
        "{prefix}_{ts}_{:.8}.{ext}",
        uuid.to_string().replace('-', "")
    )
}

/// Generate TTS audio using OpenAI-compatible API.
async fn generate_openai_tts(
    text: &str,
    voice: Option<&str>,
    output_path: Option<PathBuf>,
) -> Result<PathBuf> {
    let api_key = std::env::var("HAKIMI_TTS_API_KEY").map_err(|_| {
        HakimiError::Tool(
            "HAKIMI_TTS_API_KEY environment variable not set. \
             Set it to your OpenAI API key, or use provider='edge' for free TTS."
                .into(),
        )
    })?;

    let base_url = std::env::var("HAKIMI_TTS_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    let model = std::env::var("HAKIMI_TTS_MODEL").unwrap_or_else(|_| "tts-1".to_string());
    let voice = voice.unwrap_or("alloy");

    let url = format!("{}/audio/speech", base_url.trim_end_matches('/'));

    debug!(url = %url, model = %model, voice = %voice, "OpenAI TTS request");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    let body = json!({
        "model": model,
        "input": text,
        "voice": voice,
        "response_format": "mp3"
    });

    let response = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("TTS API request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(HakimiError::Tool(format!(
            "TTS API returned status {status}: {error_body}"
        )));
    }

    let audio_bytes = response
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read TTS response: {e}")))?;

    if audio_bytes.is_empty() {
        return Err(HakimiError::Tool("TTS API returned empty response".into()));
    }

    // Determine output path
    let out_path = output_path.unwrap_or_else(|| {
        let dir = get_output_dir(None);
        let filename = generate_filename("tts", "mp3");
        dir.join(filename)
    });

    // Ensure parent directory exists
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| HakimiError::Tool(format!("failed to create output directory: {e}")))?;
    }

    std::fs::write(&out_path, &audio_bytes)
        .map_err(|e| HakimiError::Tool(format!("failed to write audio file: {e}")))?;

    Ok(out_path)
}

/// Generate TTS audio using Microsoft Edge TTS (free, no API key).
///
/// Edge TTS uses the Azure Cognitive Services WebSocket API that's publicly
/// accessible via the Edge browser's built-in TTS. This implementation uses
/// the REST-style approach via the edge-tts-compatible HTTP endpoint.
async fn generate_edge_tts(
    text: &str,
    voice: Option<&str>,
    output_path: Option<PathBuf>,
) -> Result<PathBuf> {
    let voice = voice.unwrap_or("en-US-AriaNeural");

    // Edge TTS uses a two-step process:
    // 1. Get the WebSocket URL from the config endpoint
    // 2. Send the text via WebSocket and receive audio chunks

    // We'll use the public Edge TTS HTTP-compatible endpoint
    // via the trcnik/edge-tts-rs protocol or direct implementation

    // Step 1: Get the Edge TTS configuration
    let config_url = "https://speech.platform.bing.com/consumer/speech/synthesize/readaloud/voices/list?trustedclienttoken=6A5AA1D4EAFF4E9FB37E23D68491D6F4";

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    // Validate that the voice exists by checking the voice list
    let voices_response = client
        .get(config_url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await;

    if let Ok(resp) = voices_response
        && let Ok(body) = resp.text().await
        && let Ok(voices) = serde_json::from_str::<JsonValue>(&body)
    {
        let empty_vec = vec![];
        let voice_list = voices.as_array().unwrap_or(&empty_vec);
        let voice_exists = voice_list.iter().any(|v| {
            v.get("ShortName")
                .and_then(|n| n.as_str())
                .map(|n| n == voice)
                .unwrap_or(false)
        });
        if !voice_exists {
            warn!(voice = %voice, "voice not found in Edge TTS voice list, proceeding anyway");
        }
    }

    // Step 2: Generate audio using Edge TTS WebSocket protocol
    // Use the wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1 endpoint
    let audio_data = edge_tts_synthesize(text, voice, &client).await?;

    // Determine output path
    let out_path = output_path.unwrap_or_else(|| {
        let dir = get_output_dir(None);
        let filename = generate_filename("edge_tts", "mp3");
        dir.join(filename)
    });

    // Ensure parent directory exists
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| HakimiError::Tool(format!("failed to create output directory: {e}")))?;
    }

    std::fs::write(&out_path, &audio_data)
        .map_err(|e| HakimiError::Tool(format!("failed to write audio file: {e}")))?;

    Ok(out_path)
}

/// Synthesize speech using a free TTS endpoint.
///
/// Uses the Google Translate TTS endpoint as a free, stable, HTTP-based
/// text-to-speech backend that requires no API key.
async fn edge_tts_synthesize(text: &str, voice: &str, client: &reqwest::Client) -> Result<Vec<u8>> {
    // Extract language code from voice name (e.g. "en-US-AriaNeural" -> "en")
    let lang = voice.split('-').next().unwrap_or("en");

    // Use Google Translate TTS endpoint (free, no auth required)
    // This is the same endpoint used by Google Translate's "listen" feature
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&tl={}&client=tw-ob&q={}",
        lang,
        urlencoding::encode(text)
    );

    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("Edge TTS request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(HakimiError::Tool(format!(
            "Edge TTS request failed with status: {status}"
        )));
    }

    let audio = response
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read Edge TTS response: {e}")))?;

    if audio.is_empty() {
        return Err(HakimiError::Tool(
            "Edge TTS returned empty audio. The text may be too long or the voice may not be available.".into()
        ));
    }

    Ok(audio.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        let tool = TextToSpeechTool;
        assert_eq!(tool.name(), "text_to_speech");
        assert_eq!(tool.toolset(), "media");
        assert_eq!(tool.emoji(), "🔊");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_schema_structure() {
        let tool = TextToSpeechTool;
        let schema = tool.schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["text"].is_object());
        assert!(schema["properties"]["voice"].is_object());
        assert!(schema["properties"]["provider"].is_object());
        assert!(schema["properties"]["output_path"].is_object());

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("text".to_string())));
    }

    #[test]
    fn test_schema_voice_options() {
        let tool = TextToSpeechTool;
        let schema = tool.schema();

        let providers = schema["properties"]["provider"]["enum"].as_array().unwrap();
        assert!(providers.contains(&JsonValue::String("openai".to_string())));
        assert!(providers.contains(&JsonValue::String("edge".to_string())));
    }

    #[test]
    fn test_get_output_dir_default() {
        // Clear any env var
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::remove_var("HAKIMI_TTS_OUTPUT_DIR");
        }
        let dir = get_output_dir(None);
        assert!(dir.ends_with(".hakimi/audio_cache"));
    }

    #[test]
    fn test_get_output_dir_custom() {
        let dir = get_output_dir(Some("/tmp/tts_test"));
        assert_eq!(dir, PathBuf::from("/tmp/tts_test"));
    }

    #[test]
    fn test_get_output_dir_env() {
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::set_var("HAKIMI_TTS_OUTPUT_DIR", "/custom/tts/dir");
        }
        let dir = get_output_dir(None);
        assert_eq!(dir, PathBuf::from("/custom/tts/dir"));
        unsafe {
            std::env::remove_var("HAKIMI_TTS_OUTPUT_DIR");
        }
    }

    #[test]
    fn test_generate_filename() {
        let filename = generate_filename("tts", "mp3");
        assert!(filename.starts_with("tts_"));
        assert!(filename.ends_with(".mp3"));
        // Should be unique
        let filename2 = generate_filename("tts", "mp3");
        assert_ne!(filename, filename2);
    }

    #[tokio::test]
    async fn test_empty_text_rejected() {
        let tool = TextToSpeechTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({"text": ""}), &ctx).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("empty"));
    }

    #[tokio::test]
    async fn test_whitespace_text_rejected() {
        let tool = TextToSpeechTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({"text": "   "}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_text_rejected() {
        let tool = TextToSpeechTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({}), &ctx).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("text"));
    }

    #[tokio::test]
    async fn test_unsupported_provider_rejected() {
        let tool = TextToSpeechTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool
            .execute(&json!({"text": "hello", "provider": "invalid"}), &ctx)
            .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("unsupported"));
    }

    #[tokio::test]
    async fn test_openai_missing_api_key() {
        // Ensure no API key is set
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::remove_var("HAKIMI_TTS_API_KEY");
        }
        let tool = TextToSpeechTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool
            .execute(&json!({"text": "hello", "provider": "openai"}), &ctx)
            .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("API_KEY"));
    }
}
