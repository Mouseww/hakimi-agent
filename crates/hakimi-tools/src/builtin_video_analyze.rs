use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::debug;

use crate::Tool;
use crate::url_safety::{assert_safe_http_url, safe_http_redirect_policy};

const MAX_VIDEO_BASE64_BYTES: usize = 50 * 1024 * 1024;
const VIDEO_SIZE_WARN_BYTES: usize = 20 * 1024 * 1024;

/// Built-in tool for video analysis request preparation.
pub struct VideoAnalyzeTool;

#[async_trait]
impl Tool for VideoAnalyzeTool {
    fn name(&self) -> &str {
        "video_analyze"
    }

    fn toolset(&self) -> &str {
        "video"
    }

    fn description(&self) -> &str {
        "Analyze a video from a URL or absolute local file path. \
         Loads mp4, webm, mov, avi, mkv, mpeg, or mpg video files, encodes \
         them as base64, and returns a structured video-capable model request."
    }

    fn emoji(&self) -> &str {
        "\u{1f3ac}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "video_url": {
                    "type": "string",
                    "description": "Video URL (http/https), file:// URL, or absolute local file path to analyze."
                },
                "question": {
                    "type": "string",
                    "description": "Specific question about the video."
                }
            },
            "required": ["video_url", "question"]
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let video_url = args
            .get("video_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: video_url".into()))?;
        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Fully describe and explain everything happening in this video.");

        debug!(video_url = %video_url, question = %question, "video_analyze request");

        let (video_bytes, mime_type) =
            if video_url.starts_with("http://") || video_url.starts_with("https://") {
                download_video(video_url).await?
            } else {
                load_local_video(video_url)?
            };

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&video_bytes);
        validate_video_payload_size(video_bytes.len(), b64.len())?;

        let full_prompt = video_prompt(question);
        let video_request = json!({
            "type": "video_url",
            "video_url": {
                "url": format!("data:{};base64,{}", mime_type, b64)
            }
        });

        Ok(json!({
            "video_request": true,
            "video_source": video_url,
            "mime_type": mime_type,
            "video_size_bytes": video_bytes.len(),
            "large_video_warning": video_bytes.len() > VIDEO_SIZE_WARN_BYTES,
            "question": question,
            "prompt": full_prompt.clone(),
            "content_blocks": [
                {
                    "type": "text",
                    "text": full_prompt
                },
                video_request
            ],
            "instruction": format!(
                "Video loaded ({} bytes, {}). Ask a video-capable model: {}",
                video_bytes.len(), mime_type, question
            )
        })
        .to_string())
    }
}

async fn download_video(url: &str) -> Result<(Vec<u8>, String)> {
    assert_safe_http_url(url)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(safe_http_redirect_policy(5))
        .build()
        .map_err(|e| HakimiError::Tool(format!("Failed to create HTTP client: {e}")))?;

    let response = client
        .get(url)
        .header("accept", "video/*,*/*;q=0.8")
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("Failed to download video: {e}")))?;

    if !response.status().is_success() {
        return Err(HakimiError::Tool(format!(
            "Failed to download video: HTTP {}",
            response.status()
        )));
    }

    if let Some(content_length) = response.content_length()
        && content_length > MAX_VIDEO_BASE64_BYTES as u64
    {
        return Err(video_too_large(content_length as usize, "content-length"));
    }

    let header_mime = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(content_type_video_mime);
    let fallback_mime = detect_video_mime_type(url).map(str::to_string);
    let mime_type = header_mime
        .or(fallback_mime)
        .ok_or_else(|| unsupported_video_format(url))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("Failed to read video bytes: {e}")))?;
    if bytes.len() > MAX_VIDEO_BASE64_BYTES {
        return Err(video_too_large(bytes.len(), "downloaded video"));
    }

    Ok((bytes.to_vec(), mime_type))
}

fn load_local_video(path: &str) -> Result<(Vec<u8>, String)> {
    let resolved = path.strip_prefix("file://").unwrap_or(path);
    let mime_type =
        detect_video_mime_type(resolved).ok_or_else(|| unsupported_video_format(resolved))?;
    let bytes = std::fs::read(resolved).map_err(|e| {
        HakimiError::Tool(format!("Failed to read local video file '{resolved}': {e}"))
    })?;
    if bytes.len() > MAX_VIDEO_BASE64_BYTES {
        return Err(video_too_large(bytes.len(), "local video"));
    }

    Ok((bytes, mime_type.to_string()))
}

fn video_prompt(question: &str) -> String {
    format!(
        "Fully describe and explain everything happening in this video, \
         including visual content, motion, audio cues, text overlays, and scene \
         transitions. Then answer the following question:\n\n{question}"
    )
}

fn validate_video_payload_size(raw_len: usize, encoded_len: usize) -> Result<()> {
    if raw_len > MAX_VIDEO_BASE64_BYTES {
        return Err(video_too_large(raw_len, "video"));
    }
    if encoded_len > MAX_VIDEO_BASE64_BYTES {
        return Err(video_too_large(encoded_len, "base64 payload"));
    }
    Ok(())
}

fn detect_video_mime_type(source: &str) -> Option<&'static str> {
    let without_query = source.split('?').next().unwrap_or(source);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let lower = without_fragment.to_ascii_lowercase();

    if lower.ends_with(".mp4") {
        Some("video/mp4")
    } else if lower.ends_with(".webm") {
        Some("video/webm")
    } else if lower.ends_with(".mov") {
        Some("video/mov")
    } else if lower.ends_with(".avi") || lower.ends_with(".mkv") {
        Some("video/mp4")
    } else if lower.ends_with(".mpeg") || lower.ends_with(".mpg") {
        Some("video/mpeg")
    } else {
        None
    }
}

fn content_type_video_mime(content_type: &str) -> Option<String> {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "video/mp4" | "video/webm" | "video/mov" | "video/mpeg" => Some(mime),
        "video/quicktime" => Some("video/mov".to_string()),
        "video/x-msvideo" | "video/x-matroska" => Some("video/mp4".to_string()),
        _ => None,
    }
}

fn unsupported_video_format(source: &str) -> HakimiError {
    HakimiError::Tool(format!(
        "Unsupported video format for '{source}'. Supported extensions: mp4, webm, mov, avi, mkv, mpeg, mpg"
    ))
}

fn video_too_large(size: usize, label: &str) -> HakimiError {
    HakimiError::Tool(format!(
        "Video too large ({label}: {size} bytes, max {MAX_VIDEO_BASE64_BYTES} bytes). Compress or trim the video and retry."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_context() -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: ".".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        }
    }

    #[test]
    fn tool_metadata_matches_video_surface() {
        let tool = VideoAnalyzeTool;
        assert_eq!(tool.name(), "video_analyze");
        assert_eq!(tool.toolset(), "video");
        assert!(tool.description().contains("mp4"));
        assert!(!tool.emoji().is_empty());
    }

    #[test]
    fn schema_requires_video_url_and_question() {
        let tool = VideoAnalyzeTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("video_url")));
        assert!(required.contains(&json!("question")));
        assert_eq!(schema["properties"]["video_url"]["type"], "string");
        assert_eq!(schema["properties"]["question"]["type"], "string");
    }

    #[test]
    fn detects_supported_video_mime_types() {
        assert_eq!(detect_video_mime_type("clip.mp4"), Some("video/mp4"));
        assert_eq!(detect_video_mime_type("clip.webm"), Some("video/webm"));
        assert_eq!(detect_video_mime_type("clip.mov"), Some("video/mov"));
        assert_eq!(detect_video_mime_type("clip.avi"), Some("video/mp4"));
        assert_eq!(detect_video_mime_type("clip.mkv"), Some("video/mp4"));
        assert_eq!(detect_video_mime_type("clip.mpeg"), Some("video/mpeg"));
        assert_eq!(detect_video_mime_type("clip.mpg"), Some("video/mpeg"));
    }

    #[test]
    fn detects_video_mime_with_query_or_fragment() {
        assert_eq!(
            detect_video_mime_type("https://cdn.example.com/clip.MP4?sig=1#t=3"),
            Some("video/mp4")
        );
    }

    #[test]
    fn rejects_unsupported_video_mime_types() {
        assert_eq!(detect_video_mime_type("image.png"), None);
        assert!(
            unsupported_video_format("image.png")
                .to_string()
                .contains("Unsupported")
        );
    }

    #[test]
    fn accepts_video_content_type_headers() {
        assert_eq!(
            content_type_video_mime("video/webm; charset=binary").as_deref(),
            Some("video/webm")
        );
        assert_eq!(
            content_type_video_mime("video/quicktime").as_deref(),
            Some("video/mov")
        );
        assert_eq!(
            content_type_video_mime("video/x-matroska").as_deref(),
            Some("video/mp4")
        );
        assert_eq!(content_type_video_mime("application/octet-stream"), None);
    }

    #[test]
    fn validates_encoded_payload_size() {
        assert!(validate_video_payload_size(10, 20).is_ok());
        assert!(validate_video_payload_size(MAX_VIDEO_BASE64_BYTES + 1, 20).is_err());
        assert!(validate_video_payload_size(10, MAX_VIDEO_BASE64_BYTES + 1).is_err());
    }

    #[tokio::test]
    async fn execute_requires_video_url() {
        let tool = VideoAnalyzeTool;
        let result = tool
            .execute(&json!({ "question": "What happens?" }), &test_context())
            .await;
        assert!(
            result
                .expect_err("missing video_url should fail")
                .to_string()
                .contains("missing required parameter: video_url")
        );
    }

    #[tokio::test]
    async fn execute_blocks_metadata_video_url() {
        let tool = VideoAnalyzeTool;
        let err = tool
            .execute(
                &json!({
                    "video_url": "http://169.254.169.254/latest/meta-data",
                    "question": "What happens?"
                }),
                &test_context(),
            )
            .await
            .expect_err("metadata video URL should be rejected before download");

        assert!(err.to_string().contains("metadata"));
    }

    #[tokio::test]
    async fn execute_returns_structured_payload_for_local_video() {
        let path = std::env::temp_dir().join(format!(
            "hakimi-video-analyze-test-{}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, [0, 0, 0, 24, b'f', b't', b'y', b'p']).expect("write test video");

        let tool = VideoAnalyzeTool;
        let result = tool
            .execute(
                &json!({
                    "video_url": path.to_string_lossy(),
                    "question": "What happens?"
                }),
                &test_context(),
            )
            .await
            .expect("local video should produce a structured payload");

        let payload: serde_json::Value =
            serde_json::from_str(&result).expect("result should be JSON");
        assert_eq!(payload["video_request"], true);
        assert_eq!(payload["mime_type"], "video/mp4");
        assert_eq!(payload["question"], "What happens?");
        assert_eq!(payload["content_blocks"][0]["type"], "text");
        assert!(
            payload["content_blocks"][1]["video_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:video/mp4;base64,")
        );

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn execute_accepts_file_url_for_local_video() {
        let path = std::env::temp_dir().join(format!(
            "hakimi-video-analyze-file-url-test-{}.webm",
            std::process::id()
        ));
        std::fs::write(&path, [0, 1, 2, 3]).expect("write test video");

        let tool = VideoAnalyzeTool;
        let result = tool
            .execute(
                &json!({
                    "video_url": format!("file://{}", path.to_string_lossy()),
                    "question": "Summarize it."
                }),
                &test_context(),
            )
            .await
            .expect("file URL video should produce a structured payload");

        let payload: serde_json::Value =
            serde_json::from_str(&result).expect("result should be JSON");
        assert_eq!(payload["mime_type"], "video/webm");

        let _ = std::fs::remove_file(path);
    }
}
