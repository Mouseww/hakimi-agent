use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::debug;

use crate::Tool;

/// Built-in tool for image analysis (placeholder for future vision model integration).
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
        "Analyze and describe an image from a URL. Requires a vision-capable model. Currently returns a placeholder response; future versions will encode the image and send it to a vision model."
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
                    "description": "URL of the image to analyze."
                },
                "question": {
                    "type": "string",
                    "description": "Optional question about the image. If not provided, a general description will be returned."
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

        debug!(image_url = %image_url, question = %question, "image describe request");

        // Placeholder: in the future, this would:
        // 1. Download the image
        // 2. Encode it as base64
        // 3. Send it to a vision-capable model with the question
        // 4. Return the model's response

        Ok(format!(
            "Image analysis requires a vision-capable model.\nURL: {}\nQuestion: {}\n\n\
             This feature will be available once a vision model provider is configured.",
            image_url, question
        ))
    }
}
