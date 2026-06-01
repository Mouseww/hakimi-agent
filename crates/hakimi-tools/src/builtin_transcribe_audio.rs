use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use reqwest::multipart::{Form, Part};
use serde_json::{Value as JsonValue, json};
use std::path::Path;
use tracing::debug;

use crate::{Tool, is_whisper_hallucination};

const TRANSCRIPTION_MAX_FILE_SIZE_BYTES: usize = 25 * 1024 * 1024;
const WAV_CHUNK_HEADER_RESERVE_BYTES: usize = 64 * 1024;

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
         Uses an OpenAI-compatible audio transcription API and returns the recognized text. \
         Oversized local WAV files are split into provider-sized chunks for text output."
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
        let transcript = if should_chunk_for_transcription(
            &source,
            response_format,
            TRANSCRIPTION_MAX_FILE_SIZE_BYTES,
        ) {
            transcribe_wav_in_chunks(&source, &model, prompt, language, ctx).await?
        } else {
            request_openai_transcription(&source, &model, response_format, prompt, language, ctx)
                .await?
        };
        Ok(filter_text_transcription_response(
            response_format,
            transcript,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioSourceKind {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
struct AudioSource {
    file_name: String,
    mime_type: String,
    bytes: Vec<u8>,
    kind: AudioSourceKind,
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
        kind: AudioSourceKind::Remote,
    })
}

fn read_local_audio(path: &str) -> Result<AudioSource> {
    let bytes = std::fs::read(path)
        .map_err(|e| HakimiError::Tool(format!("failed to read local audio file '{path}': {e}")))?;

    Ok(AudioSource {
        file_name: file_name_from_source(path),
        mime_type: guess_audio_mime_type(path),
        bytes,
        kind: AudioSourceKind::Local,
    })
}

fn should_chunk_for_transcription(
    source: &AudioSource,
    response_format: &str,
    max_file_size: usize,
) -> bool {
    source.kind == AudioSourceKind::Local
        && response_format == "text"
        && source.file_name.to_ascii_lowercase().ends_with(".wav")
        && source.bytes.len() > max_file_size
}

async fn transcribe_wav_in_chunks(
    source: &AudioSource,
    model: &str,
    prompt: Option<&str>,
    language: Option<&str>,
    ctx: &ToolContext,
) -> Result<String> {
    let chunks = split_wav_for_transcription(source, TRANSCRIPTION_MAX_FILE_SIZE_BYTES)?;
    if chunks.is_empty() {
        return Err(HakimiError::Tool("no audio chunks were created".into()));
    }

    debug!(
        file_name = %source.file_name,
        chunks = chunks.len(),
        "transcribing oversized WAV in chunks"
    );

    let mut transcripts = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let transcript = request_openai_transcription(chunk, model, "text", prompt, language, ctx)
            .await
            .map_err(|err| {
                HakimiError::Tool(format!(
                    "chunk {}/{} transcription failed: {err}",
                    index + 1,
                    chunks.len()
                ))
            })?;
        let filtered = filter_text_transcription_response("text", transcript);
        let transcript = filtered.trim();
        if !transcript.is_empty() {
            transcripts.push(transcript.to_string());
        }
    }

    Ok(transcripts.join(" "))
}

fn split_wav_for_transcription(
    source: &AudioSource,
    max_file_size: usize,
) -> Result<Vec<AudioSource>> {
    let wav = parse_wav_layout(&source.bytes)?;
    let exact_header_budget = max_file_size.checked_sub(wav.data_start).ok_or_else(|| {
        HakimiError::Tool("STT max file size is too small for WAV chunking".into())
    })?;
    let reserved_budget = max_file_size.saturating_sub(WAV_CHUNK_HEADER_RESERVE_BYTES);
    let max_data_bytes = if reserved_budget >= wav.block_align {
        reserved_budget
    } else {
        exact_header_budget
    };
    let block_align = wav.block_align.max(1);
    let max_data_bytes = (max_data_bytes / block_align) * block_align;
    if max_data_bytes == 0 {
        return Err(HakimiError::Tool(
            "STT max file size is too small for WAV chunking".into(),
        ));
    }

    let mut chunks = Vec::new();
    let mut offset = 0usize;
    let stem = wav_chunk_file_stem(&source.file_name);
    let data = &source.bytes[wav.data_start..wav.data_end];

    while offset < data.len() {
        let end = offset.saturating_add(max_data_bytes).min(data.len());
        let mut chunk_bytes = source.bytes[..wav.data_start].to_vec();
        chunk_bytes.extend_from_slice(&data[offset..end]);
        patch_wav_sizes(&mut chunk_bytes, wav.data_len_offset)?;

        let index = chunks.len() + 1;
        chunks.push(AudioSource {
            file_name: format!("{stem}_chunk{index:03}.wav"),
            mime_type: source.mime_type.clone(),
            bytes: chunk_bytes,
            kind: source.kind,
        });
        offset = end;
    }

    Ok(chunks)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WavLayout {
    data_start: usize,
    data_end: usize,
    data_len_offset: usize,
    block_align: usize,
}

fn parse_wav_layout(bytes: &[u8]) -> Result<WavLayout> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(HakimiError::Tool("expected a RIFF/WAVE audio file".into()));
    }

    let mut cursor = 12usize;
    let mut block_align = None;
    let mut data = None;
    while cursor <= bytes.len().saturating_sub(8) {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_len = read_u32_le(bytes, cursor + 4)? as usize;
        let data_start = cursor
            .checked_add(8)
            .ok_or_else(|| HakimiError::Tool("invalid WAV chunk offset".into()))?;
        let data_end = data_start
            .checked_add(chunk_len)
            .ok_or_else(|| HakimiError::Tool("invalid WAV chunk length".into()))?;
        if data_end > bytes.len() {
            return Err(HakimiError::Tool(
                "WAV chunk length exceeds file size".into(),
            ));
        }

        if chunk_id == b"fmt " && chunk_len >= 14 {
            let value = read_u16_le(bytes, data_start + 12)? as usize;
            block_align = Some(value.max(1));
        } else if chunk_id == b"data" {
            data = Some(WavLayout {
                data_start,
                data_end,
                data_len_offset: cursor + 4,
                block_align: block_align.unwrap_or(2),
            });
            break;
        }

        cursor = data_end
            .checked_add(chunk_len % 2)
            .ok_or_else(|| HakimiError::Tool("invalid WAV chunk padding".into()))?;
    }

    data.ok_or_else(|| HakimiError::Tool("WAV file has no data chunk".into()))
}

fn patch_wav_sizes(bytes: &mut [u8], data_len_offset: usize) -> Result<()> {
    let riff_len = bytes
        .len()
        .checked_sub(8)
        .ok_or_else(|| HakimiError::Tool("WAV chunk is too small".into()))?;
    let data_len = bytes
        .len()
        .checked_sub(data_len_offset + 4)
        .ok_or_else(|| HakimiError::Tool("WAV data chunk is invalid".into()))?;
    let riff_len =
        u32::try_from(riff_len).map_err(|_| HakimiError::Tool("WAV chunk is too large".into()))?;
    let data_len = u32::try_from(data_len)
        .map_err(|_| HakimiError::Tool("WAV data chunk is too large".into()))?;

    bytes[4..8].copy_from_slice(&riff_len.to_le_bytes());
    bytes[data_len_offset..data_len_offset + 4].copy_from_slice(&data_len.to_le_bytes());
    Ok(())
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| HakimiError::Tool("unexpected end of WAV header".into()))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| HakimiError::Tool("unexpected end of WAV header".into()))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn wav_chunk_file_stem(file_name: &str) -> String {
    Path::new(file_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("audio")
        .to_string()
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

fn filter_text_transcription_response(response_format: &str, transcript: String) -> String {
    if response_format == "text" && is_whisper_hallucination(&transcript) {
        String::new()
    } else {
        transcript
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
    fn test_filters_text_response_whisper_hallucination() {
        assert_eq!(
            filter_text_transcription_response("text", "Thank you.".to_string()),
            ""
        );
        assert_eq!(
            filter_text_transcription_response("json", "Thank you.".to_string()),
            "Thank you."
        );
        assert_eq!(
            filter_text_transcription_response("text", "open the dashboard".to_string()),
            "open the dashboard"
        );
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
        assert_eq!(source.kind, AudioSourceKind::Local);
    }

    #[test]
    fn test_should_chunk_only_local_text_wav_over_limit() {
        let source = AudioSource {
            file_name: "recording.wav".to_string(),
            mime_type: "audio/wav".to_string(),
            bytes: vec![0; 128],
            kind: AudioSourceKind::Local,
        };
        assert!(should_chunk_for_transcription(&source, "text", 127));
        assert!(!should_chunk_for_transcription(&source, "json", 127));
        assert!(!should_chunk_for_transcription(&source, "text", 128));
        assert!(!should_chunk_for_transcription(
            &AudioSource {
                kind: AudioSourceKind::Remote,
                ..source.clone()
            },
            "text",
            127
        ));
        assert!(!should_chunk_for_transcription(
            &AudioSource {
                file_name: "recording.mp3".to_string(),
                ..source
            },
            "text",
            127
        ));
    }

    #[test]
    fn test_split_wav_for_transcription_patches_chunk_sizes() {
        let samples: Vec<i16> = (0..20).collect();
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("recording.wav");
        crate::write_pcm16_wav(&path, &samples).expect("write wav");
        let source = read_local_audio(path.to_str().expect("path str")).expect("load source");

        let chunks = split_wav_for_transcription(&source, 64).expect("split wav");

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].file_name, "recording_chunk001.wav");
        for chunk in &chunks {
            let layout = parse_wav_layout(&chunk.bytes).expect("parse chunk");
            assert!(chunk.bytes.len() <= 64);
            assert_eq!(
                read_u32_le(&chunk.bytes, 4).expect("riff len") as usize,
                chunk.bytes.len() - 8
            );
            assert_eq!(
                read_u32_le(&chunk.bytes, layout.data_len_offset).expect("data len") as usize,
                layout.data_end - layout.data_start
            );
            assert_eq!((layout.data_end - layout.data_start) % 2, 0);
        }
    }

    #[test]
    fn test_split_wav_rejects_invalid_input() {
        let source = AudioSource {
            file_name: "bad.wav".to_string(),
            mime_type: "audio/wav".to_string(),
            bytes: b"not-a-wav".to_vec(),
            kind: AudioSourceKind::Local,
        };

        let err = split_wav_for_transcription(&source, 1024).expect_err("invalid wav");
        assert!(format!("{err}").contains("RIFF/WAVE"));
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
