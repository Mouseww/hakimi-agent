use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use reqwest::multipart::{Form, Part};
use serde_json::{Value as JsonValue, json};
use std::path::Path;
use tracing::debug;

use crate::Tool;

/// Built-in speech-to-text tool using OpenAI-compatible transcription APIs.
pub struct TranscribeAudioTool;

#[async_trait]
impl Tool for TranscribeAudioTool {
    fn name(&self) -> &str {
        "transcribe_audio"
    }

    fn toolset(&self) -> &str {
        "media"
    }

    fn description(&self) -> &str {
        "Transcribe speech from a local audio file path or remote URL. \
         Uses an OpenAI-compatible audio transcription API and returns the recognized text."
    }

    fn emoji(&self) -> &str {
        "🎙️"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "audio_path": {
                    "type": "string",
                    "description": "Absolute local audio file path or http/https URL to transcribe."
                },
                "provider": {
                    "type": "string",
                    "enum": ["openai"],
                    "description": "Transcription provider. Default: configured voice provider or 'openai'."
                },
                "model": {
                    "type": "string",
                    "description": "Transcription model. Default: configured transcription model or 'whisper-1'."
                },
                "language": {
                    "type": "string",
                    "description": "Optional ISO language hint such as 'en' or 'zh'."
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional prompt to bias punctuation, terminology, or proper nouns."
                },
                "response_format": {
                    "type": "string",
                    "enum": ["text", "json", "verbose_json", "srt", "vtt"],
                    "description": "Output format returned by the transcription API. Default: text."
                }
            },
            "required": ["audio_path"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let audio_path = args
            .get("audio_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: audio_path".into()))?;

        let provider = args
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| ctx.transcription_provider.clone().filter(|s| !s.is_empty()))
            .or_else(|| ctx.tts_provider.clone().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("HAKIMI_TRANSCRIPTION_PROVIDER").ok())
            .unwrap_or_else(|| "openai".to_string());

        if provider != "openai" {
            return Err(HakimiError::Tool(format!(
                "unsupported transcription provider: '{provider}'. Use 'openai'."
            )));
        }

        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| ctx.transcription_model.clone().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("HAKIMI_TRANSCRIPTION_MODEL").ok())
            .unwrap_or_else(|| "whisper-1".to_string());

        let response_format = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        let prompt = args.get("prompt").and_then(|v| v.as_str());
        let language = args.get("language").and_then(|v| v.as_str());

        debug!(
            provider = %provider,
            model = %model,
            source = %audio_path,
            response_format = %response_format,
            "transcribe_audio request"
        );

        let source = load_audio_source(audio_path).await?;
        request_openai_transcription(&source, &model, response_format, prompt, language, ctx).await
    }
}

#[derive(Debug, Clone)]
struct AudioSource {
    file_name: String,
    mime_type: String,
    bytes: Vec<u8>,
}

async fn load_audio_source(audio_path: &str) -> Result<AudioSource> {
    if audio_path.starts_with("http://") || audio_path.starts_with("https://") {
        download_audio(audio_path).await
    } else {
        read_local_audio(audio_path)
    }
}

async fn download_audio(url: &str) -> Result<AudioSource> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to download audio: {e}")))?;

    if !response.status().is_success() {
        return Err(HakimiError::Tool(format!(
            "failed to download audio: HTTP {}",
            response.status()
        )));
    }

    let mime_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(';')
                .next()
                .unwrap_or("application/octet-stream")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| guess_audio_mime_type(url));

    let bytes = response
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read downloaded audio: {e}")))?;

    Ok(AudioSource {
        file_name: file_name_from_source(url),
        mime_type,
        bytes: bytes.to_vec(),
    })
}

fn read_local_audio(path: &str) -> Result<AudioSource> {
    let bytes = std::fs::read(path)
        .map_err(|e| HakimiError::Tool(format!("failed to read local audio file '{path}': {e}")))?;

    Ok(AudioSource {
        file_name: file_name_from_source(path),
        mime_type: guess_audio_mime_type(path),
        bytes,
    })
}

async fn request_openai_transcription(
    source: &AudioSource,
    model: &str,
    response_format: &str,
    prompt: Option<&str>,
    language: Option<&str>,
    ctx: &ToolContext,
) -> Result<String> {
    let api_key = ctx
        .transcription_api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HAKIMI_TRANSCRIPTION_API_KEY").ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| {
            HakimiError::Tool(
                "HAKIMI_TRANSCRIPTION_API_KEY or OPENAI_API_KEY environment variable not set."
                    .into(),
            )
        })?;

    let base_url = ctx
        .transcription_base_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HAKIMI_TRANSCRIPTION_BASE_URL").ok())
        .or_else(|| ctx.tts_base_url.clone().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let file_part = Part::bytes(source.bytes.clone())
        .file_name(source.file_name.clone())
        .mime_str(&source.mime_type)
        .map_err(|e| HakimiError::Tool(format!("failed to prepare audio upload: {e}")))?;

    let mut form = Form::new()
        .text("model", model.to_string())
        .text("response_format", response_format.to_string())
        .part("file", file_part);

    if let Some(prompt) = prompt.filter(|value| !value.trim().is_empty()) {
        form = form.text("prompt", prompt.to_string());
    }
    if let Some(language) = language.filter(|value| !value.trim().is_empty()) {
        form = form.text("language", language.to_string());
    }

    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    let response = client
        .post(&url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("transcription API request failed: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read transcription response: {e}")))?;

    if !status.is_success() {
        return Err(HakimiError::Tool(format!(
            "transcription API returned status {status}: {body}"
        )));
    }

    if response_format == "json" || response_format == "verbose_json" {
        return serde_json::from_str::<JsonValue>(&body)
            .map(|value| value.to_string())
            .map_err(|e| {
                HakimiError::Tool(format!("failed to parse JSON transcription response: {e}"))
            });
    }

    Ok(body.trim().to_string())
}

fn file_name_from_source(source: &str) -> String {
    let trimmed = source.split('?').next().unwrap_or(source);
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "audio.wav".to_string())
}

fn guess_audio_mime_type(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        "audio/mpeg".to_string()
    } else if lower.ends_with(".wav") {
        "audio/wav".to_string()
    } else if lower.ends_with(".m4a") {
        "audio/mp4".to_string()
    } else if lower.ends_with(".ogg") || lower.ends_with(".oga") {
        "audio/ogg".to_string()
    } else if lower.ends_with(".webm") {
        "audio/webm".to_string()
    } else if lower.ends_with(".flac") {
        "audio/flac".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            workdir: ".".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_tool_metadata() {
        let tool = TranscribeAudioTool;
        assert_eq!(tool.name(), "transcribe_audio");
        assert_eq!(tool.toolset(), "media");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_schema_has_required_field() {
        let tool = TranscribeAudioTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().expect("required array");
        assert!(required.contains(&json!("audio_path")));
    }

    #[test]
    fn test_guess_audio_mime_type() {
        assert_eq!(guess_audio_mime_type("voice.mp3"), "audio/mpeg");
        assert_eq!(guess_audio_mime_type("voice.WAV"), "audio/wav");
        assert_eq!(guess_audio_mime_type("voice.ogg"), "audio/ogg");
        assert_eq!(guess_audio_mime_type("voice.webm"), "audio/webm");
    }

    #[test]
    fn test_file_name_from_source_handles_query() {
        assert_eq!(
            file_name_from_source("https://example.com/audio/test.m4a?download=1"),
            "test.m4a"
        );
    }

    #[test]
    fn test_read_local_audio() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sample.wav");
        std::fs::write(&path, b"audio-bytes").expect("write sample");
        let source = read_local_audio(path.to_str().expect("path str")).expect("load source");
        assert_eq!(source.file_name, "sample.wav");
        assert_eq!(source.mime_type, "audio/wav");
        assert_eq!(source.bytes, b"audio-bytes");
    }

    #[tokio::test]
    async fn test_execute_requires_audio_path() {
        let tool = TranscribeAudioTool;
        let result = tool.execute(&json!({}), &test_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_rejects_unsupported_provider() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sample.wav");
        std::fs::write(&path, b"audio-bytes").expect("write sample");
        let tool = TranscribeAudioTool;
        let result = tool
            .execute(
                &json!({
                    "audio_path": path.to_string_lossy(),
                    "provider": "edge"
                }),
                &test_ctx(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_uses_context_model_and_fails_without_key() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sample.wav");
        std::fs::write(&path, b"audio-bytes").expect("write sample");
        let tool = TranscribeAudioTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            workdir: ".".to_string(),
            transcription_model: Some("whisper-1".to_string()),
            ..Default::default()
        };
        let result = tool
            .execute(&json!({"audio_path": path.to_string_lossy()}), &ctx)
            .await;
        assert!(result.is_err());
        let message = format!("{}", result.expect_err("expected missing key error"));
        assert!(message.contains("API_KEY"));
    }
}
