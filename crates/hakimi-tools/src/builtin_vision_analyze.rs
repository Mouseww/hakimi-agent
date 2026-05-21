//! Vision analysis tool — replaces the placeholder image_describe.
//!
//! Downloads images from URLs, converts to base64, and sends to vision-capable
//! models for analysis.

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use tracing::{debug, warn};

use crate::Tool;

/// Built-in tool for real image analysis via vision-capable models.
pub struct VisionAnalyzeTool;

#[async_trait]
impl Tool for VisionAnalyzeTool {
    fn name(&self) -> &str {
        "vision_analyze"
    }

    fn toolset(&self) -> &str {
        "vision"
    }

    fn description(&self) -> &str {
        "Analyze and describe an image from a URL or local file path. \
         Downloads the image, encodes it as base64, and sends it to a vision-capable model \
         for analysis with an optional question/prompt."
    }

    fn emoji(&self) -> &str {
        "\u{1f441}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "image_url": {
                    "type": "string",
                    "description": "URL of the image to analyze (http/https) or absolute local file path."
                },
                "question": {
                    "type": "string",
                    "description": "Optional specific question about the image. If not provided, a general description will be requested."
                }
            },
            "required": ["image_url"]
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let image_url = args
            .get("image_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: image_url".into()))?;

        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe this image in detail.");

        debug!(image_url = %image_url, question = %question, "vision_analyze request");

        // Determine if this is a URL or a local file path.
        let (image_bytes, mime_type) = if image_url.starts_with("http://") || image_url.starts_with("https://") {
            download_image(image_url).await?
        } else {
            load_local_image(image_url)?
        };

        // Encode as base64.
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

        // Build the vision request content.
        let vision_request = json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", mime_type, b64)
            }
        });

        // Return structured result that the agent loop can use.
        // The actual vision model call would be made by the transport layer.
        Ok(json!({
            "vision_request": true,
            "image_source": image_url,
            "mime_type": mime_type,
            "image_size_bytes": image_bytes.len(),
            "question": question,
            "content_block": vision_request,
            "instruction": format!(
                "Image loaded ({} bytes, {}). Ask the vision model: {}",
                image_bytes.len(), mime_type, question
            )
        }).to_string())
    }
}

/// Download an image from a URL and return (bytes, mime_type).
async fn download_image(url: &str) -> Result<(Vec<u8>, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| HakimiError::Tool(format!("Failed to create HTTP client: {e}")))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("Failed to download image: {e}")))?;

    if !response.status().is_success() {
        return Err(HakimiError::Tool(format!(
            "Failed to download image: HTTP {}",
            response.status()
        )));
    }

    let mime_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("image/jpeg").trim().to_string())
        .unwrap_or_else(|| guess_mime_type(url));

    let bytes = response
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("Failed to read image bytes: {e}")))?;

    Ok((bytes.to_vec(), mime_type))
}

/// Load a local image file and return (bytes, mime_type).
fn load_local_image(path: &str) -> Result<(Vec<u8>, String)> {
    let bytes = std::fs::read(path).map_err(|e| {
        HakimiError::Tool(format!("Failed to read local image file '{path}': {e}"))
    })?;

    let mime_type = guess_mime_type(path);
    Ok((bytes, mime_type))
}

/// Guess MIME type from file extension.
fn guess_mime_type(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".svg") {
        "image/svg+xml".to_string()
    } else if lower.ends_with(".bmp") {
        "image/bmp".to_string()
    } else {
        "image/jpeg".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        let tool = VisionAnalyzeTool;
        assert_eq!(tool.name(), "vision_analyze");
        assert_eq!(tool.toolset(), "vision");
        assert!(!tool.description().is_empty());
        assert!(!tool.emoji().is_empty());
    }

    #[test]
    fn test_schema_has_required_fields() {
        let tool = VisionAnalyzeTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("image_url")));
    }

    #[test]
    fn test_guess_mime_type_png() {
        assert_eq!(guess_mime_type("image.png"), "image/png");
        assert_eq!(guess_mime_type("/path/to/photo.PNG"), "image/png");
    }

    #[test]
    fn test_guess_mime_type_jpg() {
        assert_eq!(guess_mime_type("photo.jpg"), "image/jpeg");
        assert_eq!(guess_mime_type("photo.jpeg"), "image/jpeg");
        assert_eq!(guess_mime_type("photo"), "image/jpeg"); // default
    }

    #[test]
    fn test_guess_mime_type_gif() {
        assert_eq!(guess_mime_type("anim.gif"), "image/gif");
    }

    #[test]
    fn test_guess_mime_type_webp() {
        assert_eq!(guess_mime_type("image.webp"), "image/webp");
    }

    #[test]
    fn test_guess_mime_type_svg() {
        assert_eq!(guess_mime_type("icon.svg"), "image/svg+xml");
    }

    #[test]
    fn test_guess_mime_type_bmp() {
        assert_eq!(guess_mime_type("image.bmp"), "image/bmp");
    }

    #[test]
    fn test_load_local_image_nonexistent() {
        let result = load_local_image("/nonexistent/path/image.png");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_missing_url() {
        let tool = VisionAnalyzeTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: ".".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_with_question() {
        let tool = VisionAnalyzeTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: ".".to_string(),
            model: None,
            delegate_executor: None,
        };
        // This will fail since the URL is invalid, but tests parameter parsing.
        let result = tool
            .execute(
                &json!({"image_url": "https://example.com/nonexistent.jpg", "question": "What is this?"}),
                &ctx,
            )
            .await;
        // Should fail at download, not at parameter parsing.
        assert!(result.is_err());
    }

    #[test]
    fn test_schema_has_correct_properties() {
        let tool = VisionAnalyzeTool;
        let schema = tool.schema();
        let props = schema["properties"].as_object().expect("properties should be an object");
        assert!(props.contains_key("image_url"), "schema must have image_url property");
        assert!(props.contains_key("question"), "schema must have question property");
        assert_eq!(props["image_url"]["type"], "string");
        assert_eq!(props["question"]["type"], "string");
    }

    #[test]
    fn test_guess_mime_type_from_url() {
        // guess_mime_type does simple ends_with matching on the full path
        assert_eq!(guess_mime_type("https://example.com/image.png"), "image/png");
        assert_eq!(guess_mime_type("http://cdn.example.com/photo.jpg"), "image/jpeg");
        // URLs with query strings won't match (ends_with sees the query, not extension)
        assert_eq!(guess_mime_type("https://example.com/image.png?v=1"), "image/jpeg");
    }

    #[test]
    fn test_guess_mime_type_case_insensitive() {
        assert_eq!(guess_mime_type("FILE.GIF"), "image/gif");
        assert_eq!(guess_mime_type("photo.WEBP"), "image/webp");
        assert_eq!(guess_mime_type("icon.SVG"), "image/svg+xml");
        assert_eq!(guess_mime_type("scan.BMP"), "image/bmp");
    }
}
