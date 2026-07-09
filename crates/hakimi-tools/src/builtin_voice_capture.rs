use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tokio::process::Command;
use tracing::{debug, info};

use crate::{
    DEFAULT_SILENCE_RMS_THRESHOLD, NO_SPEECH_TIMEOUT_SECONDS, Tool, TranscribeAudioTool,
    VoiceCaptureFormat, find_voice_capture_plan, summarize_pcm16_wav_file,
};

/// Built-in push-to-talk capture tool for local voice-mode recordings.
pub struct VoiceCaptureTool;

#[async_trait]
impl Tool for VoiceCaptureTool {
    fn name(&self) -> &str {
        "voice_capture"
    }

    fn toolset(&self) -> &str {
        "media"
    }

    fn description(&self) -> &str {
        "Capture a short local microphone recording with an installed system recorder, validate the captured artifact, and optionally transcribe it through transcribe_audio."
    }

    fn emoji(&self) -> &str {
        "🎙️"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "duration_seconds": {
                    "type": "number",
                    "description": "Maximum recording duration. Default: 15 seconds."
                },
                "output_path": {
                    "type": "string",
                    "description": "Optional local output path. WAV is preferred for desktop backends; Termux may produce AAC."
                },
                "silence_threshold": {
                    "type": "integer",
                    "description": "PCM16 peak RMS threshold below which WAV recordings are rejected as silence. Default: 200."
                },
                "transcribe": {
                    "type": "boolean",
                    "description": "If true, dispatch the captured recording to transcribe_audio after validation. Default: false."
                },
                "provider": {
                    "type": "string",
                    "enum": ["openai"],
                    "description": "Transcription provider forwarded to transcribe_audio when transcribe is true."
                },
                "model": {
                    "type": "string",
                    "description": "Transcription model forwarded to transcribe_audio when transcribe is true."
                },
                "language": {
                    "type": "string",
                    "description": "Optional ISO language hint forwarded to transcribe_audio."
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional transcription prompt forwarded to transcribe_audio."
                },
                "response_format": {
                    "type": "string",
                    "enum": ["text", "json", "verbose_json", "srt", "vtt"],
                    "description": "Transcription response format. Default: text."
                }
            }
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(4096)
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let duration_seconds = args
            .get("duration_seconds")
            .and_then(|value| value.as_f64())
            .map(|value| value as f32)
            .unwrap_or(NO_SPEECH_TIMEOUT_SECONDS);
        let silence_threshold = args
            .get("silence_threshold")
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(DEFAULT_SILENCE_RMS_THRESHOLD);
        let output_path = args
            .get("output_path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from);

        let plan =
            find_voice_capture_plan(output_path.as_deref(), duration_seconds, silence_threshold)
                .ok_or_else(|| {
                    HakimiError::ToolSimple(
                "no local voice capture backend found; install ffmpeg, arecord/rec, or Termux:API"
                    .into(),
            )
                })?;

        if let Some(parent) = plan
            .output_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                HakimiError::ToolSimple(format!(
                    "failed to create voice recording directory '{}': {err}",
                    parent.display()
                ))
            })?;
        }

        debug!(
            backend = %plan.backend,
            output = %plan.output_path.display(),
            duration_seconds = plan.duration_seconds,
            "voice capture starting"
        );

        let mut command = Command::new(&plan.command.program);
        command.args(&plan.command.args);
        command.kill_on_drop(true);
        let output = tokio::time::timeout(
            Duration::from_secs_f32(plan.duration_seconds + 15.0),
            command.output(),
        )
        .await
        .map_err(|_| HakimiError::ToolSimple("voice capture command timed out".into()))?
        .map_err(|err| HakimiError::ToolSimple(format!("voice capture command failed: {err}")))?;

        if !output.status.success() {
            return Err(HakimiError::ToolSimple(format!(
                "voice capture backend '{}' exited with status {}: {}",
                plan.backend,
                output.status,
                command_output_excerpt(&output.stderr, &output.stdout)
            )));
        }

        if !plan.output_path.is_file() {
            return Err(HakimiError::ToolSimple(format!(
                "voice capture backend '{}' did not create '{}'",
                plan.backend,
                plan.output_path.display()
            )));
        }

        let mut response = json!({
            "audio_path": plan.output_path.to_string_lossy().to_string(),
            "backend": plan.backend.clone(),
            "format": plan.format.as_str(),
            "duration_seconds": plan.duration_seconds,
            "transcribed": false,
        });

        if plan.format == VoiceCaptureFormat::Pcm16Wav {
            let summary = summarize_pcm16_wav_file(&plan.output_path, plan.silence_threshold)
                .map_err(|err| {
                    HakimiError::ToolSimple(format!("failed to inspect captured WAV recording: {err}"))
                })?;
            response["recording"] = json!({
                "samples": summary.samples,
                "duration_seconds": summary.duration_seconds,
                "peak_rms": summary.peak_rms,
                "accepted": summary.accepted,
                "rejection_reason": summary.rejection_reason,
            });
            if !summary.accepted {
                return Ok(response.to_string());
            }
        }

        if args
            .get("transcribe")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            let transcription = transcribe_captured_recording(args, ctx, &plan.output_path).await?;
            response["transcribed"] = json!(true);
            response["transcript"] = json!(transcription);
        }

        info!(
            backend = %plan.backend,
            output = %plan.output_path.display(),
            "voice capture completed"
        );
        Ok(response.to_string())
    }
}

async fn transcribe_captured_recording(
    args: &JsonValue,
    ctx: &ToolContext,
    audio_path: &std::path::Path,
) -> Result<String> {
    let mut forwarded = serde_json::Map::new();
    forwarded.insert(
        "audio_path".to_string(),
        json!(audio_path.to_string_lossy().to_string()),
    );
    for key in ["provider", "model", "language", "prompt", "response_format"] {
        if let Some(value) = args.get(key) {
            forwarded.insert(key.to_string(), value.clone());
        }
    }
    TranscribeAudioTool
        .execute(&JsonValue::Object(forwarded), ctx)
        .await
}

fn command_output_excerpt(stderr: &[u8], stdout: &[u8]) -> String {
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(stderr));
    if combined.trim().is_empty() {
        combined.push_str(&String::from_utf8_lossy(stdout));
    }
    let excerpt: String = combined.trim().chars().take(500).collect();
    if excerpt.is_empty() {
        "no output".to_string()
    } else {
        excerpt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_exposes_record_and_transcribe_arguments() {
        let schema = VoiceCaptureTool.schema();

        assert!(schema["properties"]["duration_seconds"].is_object());
        assert!(schema["properties"]["output_path"].is_object());
        assert!(schema["properties"]["transcribe"].is_object());
        assert!(schema["properties"]["response_format"].is_object());
    }

    #[test]
    fn command_output_excerpt_prefers_stderr_and_truncates() {
        let stderr = "x".repeat(600);
        let stdout = b"stdout";
        let excerpt = command_output_excerpt(stderr.as_bytes(), stdout);

        assert_eq!(excerpt.len(), 500);
        assert!(excerpt.chars().all(|ch| ch == 'x'));
    }

    #[tokio::test]
    async fn transcribe_forwarder_preserves_requested_options() {
        let args = json!({
            "transcribe": true,
            "provider": "openai",
            "model": "whisper-1",
            "language": "en",
            "prompt": "Hakimi terms",
            "response_format": "text"
        });
        let ctx = ToolContext {
            session_id: "test".to_string(),
            workdir: ".".to_string(),
            ..Default::default()
        };
        let result = transcribe_captured_recording(
            &args,
            &ctx,
            std::path::Path::new("/tmp/hakimi-missing.wav"),
        )
        .await;

        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("hakimi-missing.wav"));
    }
}
