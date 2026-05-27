use async_trait::async_trait;
use hakimi_common::{Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::debug;

use crate::Tool;

/// Backward-compatible image analysis tool backed by `vision_analyze`.
pub struct ImageDescribeTool;

#[async_trait]
impl Tool for ImageDescribeTool {
    fn name(&self) -> &str {
        "image_describe"
    }

    fn toolset(&self) -> &str {
        "media"
    }

    fn description(&self) -> &str {
        "Analyze and describe an image from a URL or absolute local file path. \
         This is a compatibility alias for vision_analyze and returns the same \
         structured vision request payload."
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
                    "description": "URL or absolute local file path of the image to analyze."
                },
                "question": {
                    "type": "string",
                    "description": "Optional question about the image. If not provided, a general description will be returned."
                }
            },
            "required": ["image_url"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let image_url = args.get("image_url").and_then(|v| v.as_str()).unwrap_or("");
        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe this image in detail.");
        debug!(image_url = %image_url, question = %question, "image describe request");

        crate::VisionAnalyzeTool.execute(args, ctx).await
    }
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
    fn test_tool_metadata_describes_alias() {
        let tool = ImageDescribeTool;
        assert_eq!(tool.name(), "image_describe");
        assert_eq!(tool.toolset(), "media");
        assert!(tool.description().contains("compatibility alias"));
        assert!(!tool.description().contains("placeholder"));
    }

    #[test]
    fn test_schema_matches_legacy_arguments() {
        let tool = ImageDescribeTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("image_url")));
        assert_eq!(schema["properties"]["image_url"]["type"], "string");
        assert_eq!(schema["properties"]["question"]["type"], "string");
    }

    #[tokio::test]
    async fn test_execute_reuses_vision_validation() {
        let tool = ImageDescribeTool;
        let result = tool.execute(&json!({}), &test_context()).await;
        let err = result.expect_err("missing image_url should fail");
        assert!(
            err.to_string()
                .contains("missing required parameter: image_url"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_execute_returns_structured_vision_payload_for_local_file() {
        let path = std::env::temp_dir().join(format!(
            "hakimi-image-describe-test-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'])
            .expect("write test image");

        let tool = ImageDescribeTool;
        let result = tool
            .execute(
                &json!({
                    "image_url": path.to_string_lossy(),
                    "question": "What is in this image?"
                }),
                &test_context(),
            )
            .await
            .expect("local image should produce a structured payload");

        let payload: serde_json::Value =
            serde_json::from_str(&result).expect("result should be JSON");
        assert_eq!(payload["vision_request"], true);
        assert_eq!(payload["mime_type"], "image/png");
        assert_eq!(payload["question"], "What is in this image?");
        assert!(
            payload["content_block"]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(!result.contains("will be available once"));

        let _ = std::fs::remove_file(path);
    }
}
