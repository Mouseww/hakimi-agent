use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;
use tracing::{debug, info};

use crate::Tool;

/// Built-in tool that generates images from text prompts using AI image generation APIs.
///
/// Supports multiple providers:
/// - `fal` (default): FAL.ai image generation API
/// - `openai`: OpenAI-compatible image generation (DALL-E, etc.)
///
/// Configuration is read from environment variables:
/// - `HAKIMI_IMAGE_GEN_PROVIDER`: Provider name ("fal" or "openai"), default "fal"
/// - `HAKIMI_IMAGE_GEN_API_KEY`: API key for the provider
/// - `HAKIMI_IMAGE_GEN_BASE_URL`: Base URL override
/// - `HAKIMI_IMAGE_GEN_MODEL`: Model name override
/// - `HAKIMI_IMAGE_GEN_OUTPUT_DIR`: Directory for output files (default: ~/.hakimi/image_cache/)
pub struct ImageGenerateTool;

#[async_trait]
impl Tool for ImageGenerateTool {
    fn name(&self) -> &str {
        "image_generate"
    }

    fn toolset(&self) -> &str {
        "image"
    }

    fn description(&self) -> &str {
        "Generate images from text prompts using AI image generation APIs. Returns a file path to the generated image. \
         Supports FAL.ai and OpenAI-compatible (DALL-E) providers. \
         Configure via HAKIMI_IMAGE_GEN_API_KEY environment variable."
    }

    fn emoji(&self) -> &str {
        "\u{1f3a8}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text prompt describing the image to generate.",
                    "maxLength": 4000
                },
                "aspect_ratio": {
                    "type": "string",
                    "enum": ["square", "landscape", "portrait"],
                    "description": "Aspect ratio of the generated image. 'square' is 1:1, 'landscape' is 16:9, 'portrait' is 9:16. Default: landscape."
                },
                "model": {
                    "type": "string",
                    "description": "Model to use for generation. Provider-specific. If not set, uses the default model for the configured provider."
                },
                "provider": {
                    "type": "string",
                    "enum": ["fal", "openai"],
                    "description": "Image generation provider. Default: auto-detect from env (HAKIMI_IMAGE_GEN_PROVIDER). Falls back to 'fal'."
                },
                "output_path": {
                    "type": "string",
                    "description": "Custom output file path. If not provided, auto-generates in the image output directory."
                }
            },
            "required": ["prompt"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(2048) // Result is just a file path
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: prompt".into()))?;

        if prompt.trim().is_empty() {
            return Err(HakimiError::Tool("prompt parameter cannot be empty".into()));
        }

        // Determine provider
        let provider = args
            .get("provider")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::var("HAKIMI_IMAGE_GEN_PROVIDER").unwrap_or_else(|_| "fal".to_string())
            });

        let aspect_ratio = args
            .get("aspect_ratio")
            .and_then(|v| v.as_str())
            .unwrap_or("landscape");

        let model = args.get("model").and_then(|v| v.as_str()).map(String::from);

        let output_path = args
            .get("output_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);

        debug!(
            provider = %provider,
            prompt_len = prompt.len(),
            aspect_ratio = %aspect_ratio,
            "image generation request"
        );

        let result_path = match provider.as_str() {
            "fal" => {
                generate_fal_image(prompt, aspect_ratio, model.as_deref(), output_path).await?
            }
            "openai" => {
                generate_openai_image(prompt, aspect_ratio, model.as_deref(), output_path).await?
            }
            _ => {
                return Err(HakimiError::Tool(format!(
                    "unsupported image generation provider: '{provider}'. Use 'fal' or 'openai'."
                )));
            }
        };

        info!(path = %result_path.display(), provider = %provider, "image generated");
        Ok(format!("IMAGE:{}", result_path.display()))
    }
}

/// Get the output directory for generated images.
fn get_output_dir(custom: Option<&str>) -> PathBuf {
    if let Some(dir) = custom {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("HAKIMI_IMAGE_GEN_OUTPUT_DIR") {
        return PathBuf::from(dir);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".hakimi").join("image_cache")
}

/// Generate a unique filename for the image output.
fn generate_filename(prefix: &str, ext: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    format!(
        "{prefix}_{ts}_{:.8}.{ext}",
        uuid.to_string().replace('-', "")
    )
}

/// Map aspect ratio strings to resolution dimensions.
fn aspect_ratio_to_dimensions(aspect_ratio: &str) -> (u32, u32) {
    match aspect_ratio {
        "landscape" => (1024, 576), // 16:9
        "portrait" => (576, 1024),  // 9:16
        _ => (1024, 1024),          // 1:1
    }
}

/// Map aspect ratio to the string format used by FAL.ai.
#[allow(dead_code)]
fn aspect_ratio_to_fal(aspect_ratio: &str) -> &str {
    match aspect_ratio {
        "landscape" => "16:9",
        "portrait" => "9:16",
        _ => "1:1",
    }
}

/// Map aspect ratio to OpenAI size string.
fn aspect_ratio_to_openai_size(aspect_ratio: &str) -> &str {
    match aspect_ratio {
        "landscape" => "1792x1024",
        "portrait" => "1024x1792",
        _ => "1024x1024",
    }
}

/// Generate an image using FAL.ai API.
///
/// FAL.ai uses a queue-based API:
/// 1. POST to queue endpoint to submit the request
/// 2. GET the result when ready
async fn generate_fal_image(
    prompt: &str,
    aspect_ratio: &str,
    model: Option<&str>,
    output_path: Option<PathBuf>,
) -> Result<PathBuf> {
    let api_key = std::env::var("HAKIMI_IMAGE_GEN_API_KEY").map_err(|_| {
        HakimiError::Tool(
            "HAKIMI_IMAGE_GEN_API_KEY environment variable not set. \
             Set it to your FAL.ai API key, or use provider='openai' for OpenAI."
                .into(),
        )
    })?;

    let model = model.unwrap_or("fal-ai/flux/schnell");
    let base_url = std::env::var("HAKIMI_IMAGE_GEN_BASE_URL")
        .unwrap_or_else(|_| "https://fal.run".to_string());

    let url = format!("{}/{}", base_url.trim_end_matches('/'), model);

    debug!(url = %url, model = %model, aspect_ratio = %aspect_ratio, "FAL.ai image generation request");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    let (width, height) = aspect_ratio_to_dimensions(aspect_ratio);

    let body = json!({
        "prompt": prompt,
        "image_size": {
            "width": width,
            "height": height
        },
        "num_inference_steps": 4,
        "num_images": 1
    });

    let response = client
        .post(&url)
        .header("Authorization", format!("Key {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("FAL.ai API request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(HakimiError::Tool(format!(
            "FAL.ai API returned status {status}: {error_body}"
        )));
    }

    let response_json: JsonValue = response
        .json()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to parse FAL.ai response: {e}")))?;

    // FAL.ai returns image URLs in the response
    let image_url = response_json
        .get("images")
        .and_then(|imgs| imgs.as_array())
        .and_then(|arr| arr.first())
        .and_then(|img| img.get("url").or(Some(img)).and_then(|v| v.as_str()))
        .or_else(|| {
            response_json
                .get("image")
                .and_then(|v| v.get("url").and_then(|u| u.as_str()).or(v.as_str()))
        })
        .or_else(|| response_json.get("url").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            HakimiError::Tool(format!(
                "FAL.ai returned no image URL in response: {}",
                serde_json::to_string(&response_json).unwrap_or_default()
            ))
        })?;

    // Download the image
    let image_bytes = client
        .get(image_url)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to download generated image: {e}")))?
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read image data: {e}")))?;

    if image_bytes.is_empty() {
        return Err(HakimiError::Tool("downloaded image is empty".into()));
    }

    // Determine output path
    let out_path = output_path.unwrap_or_else(|| {
        let dir = get_output_dir(None);
        let filename = generate_filename("img", "png");
        dir.join(filename)
    });

    // Ensure parent directory exists
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| HakimiError::Tool(format!("failed to create output directory: {e}")))?;
    }

    std::fs::write(&out_path, &image_bytes)
        .map_err(|e| HakimiError::Tool(format!("failed to write image file: {e}")))?;

    Ok(out_path)
}

/// Generate an image using OpenAI-compatible API (DALL-E).
async fn generate_openai_image(
    prompt: &str,
    aspect_ratio: &str,
    model: Option<&str>,
    output_path: Option<PathBuf>,
) -> Result<PathBuf> {
    let api_key = std::env::var("HAKIMI_IMAGE_GEN_API_KEY").map_err(|_| {
        HakimiError::Tool(
            "HAKIMI_IMAGE_GEN_API_KEY environment variable not set. \
             Set it to your OpenAI API key."
                .into(),
        )
    })?;

    let base_url = std::env::var("HAKIMI_IMAGE_GEN_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

    let model = model.unwrap_or("dall-e-3");
    let size = aspect_ratio_to_openai_size(aspect_ratio);

    let url = format!("{}/images/generations", base_url.trim_end_matches('/'));

    debug!(url = %url, model = %model, size = %size, "OpenAI image generation request");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| HakimiError::Tool(format!("failed to create HTTP client: {e}")))?;

    let body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "size": size,
        "response_format": "url"
    });

    let response = client
        .post(&url)
        .bearer_auth(&api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("OpenAI API request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(HakimiError::Tool(format!(
            "OpenAI image API returned status {status}: {error_body}"
        )));
    }

    let response_json: JsonValue = response
        .json()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to parse OpenAI response: {e}")))?;

    let image_url = response_json
        .get("data")
        .and_then(|data| data.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("url").and_then(|v| v.as_str()))
        .ok_or_else(|| {
            HakimiError::Tool(format!(
                "OpenAI returned no image URL in response: {}",
                serde_json::to_string(&response_json).unwrap_or_default()
            ))
        })?;

    // Download the image
    let image_bytes = client
        .get(image_url)
        .send()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to download generated image: {e}")))?
        .bytes()
        .await
        .map_err(|e| HakimiError::Tool(format!("failed to read image data: {e}")))?;

    if image_bytes.is_empty() {
        return Err(HakimiError::Tool("downloaded image is empty".into()));
    }

    // Determine output path
    let out_path = output_path.unwrap_or_else(|| {
        let dir = get_output_dir(None);
        let filename = generate_filename("img", "png");
        dir.join(filename)
    });

    // Ensure parent directory exists
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| HakimiError::Tool(format!("failed to create output directory: {e}")))?;
    }

    std::fs::write(&out_path, &image_bytes)
        .map_err(|e| HakimiError::Tool(format!("failed to write image file: {e}")))?;

    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        let tool = ImageGenerateTool;
        assert_eq!(tool.name(), "image_generate");
        assert_eq!(tool.toolset(), "image");
        assert_eq!(tool.emoji(), "\u{1f3a8}");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_schema_structure() {
        let tool = ImageGenerateTool;
        let schema = tool.schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["prompt"].is_object());
        assert!(schema["properties"]["aspect_ratio"].is_object());
        assert!(schema["properties"]["model"].is_object());
        assert!(schema["properties"]["provider"].is_object());
        assert!(schema["properties"]["output_path"].is_object());

        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&JsonValue::String("prompt".to_string())));
    }

    #[test]
    fn test_schema_aspect_ratio_options() {
        let tool = ImageGenerateTool;
        let schema = tool.schema();

        let ratios = schema["properties"]["aspect_ratio"]["enum"]
            .as_array()
            .unwrap();
        assert!(ratios.contains(&JsonValue::String("square".to_string())));
        assert!(ratios.contains(&JsonValue::String("landscape".to_string())));
        assert!(ratios.contains(&JsonValue::String("portrait".to_string())));
    }

    #[test]
    fn test_schema_provider_options() {
        let tool = ImageGenerateTool;
        let schema = tool.schema();

        let providers = schema["properties"]["provider"]["enum"].as_array().unwrap();
        assert!(providers.contains(&JsonValue::String("fal".to_string())));
        assert!(providers.contains(&JsonValue::String("openai".to_string())));
    }

    #[test]
    fn test_aspect_ratio_to_dimensions() {
        assert_eq!(aspect_ratio_to_dimensions("landscape"), (1024, 576));
        assert_eq!(aspect_ratio_to_dimensions("portrait"), (576, 1024));
        assert_eq!(aspect_ratio_to_dimensions("square"), (1024, 1024));
        // Default fallback
        assert_eq!(aspect_ratio_to_dimensions("unknown"), (1024, 1024));
    }

    #[test]
    fn test_aspect_ratio_to_fal() {
        assert_eq!(aspect_ratio_to_fal("landscape"), "16:9");
        assert_eq!(aspect_ratio_to_fal("portrait"), "9:16");
        assert_eq!(aspect_ratio_to_fal("square"), "1:1");
    }

    #[test]
    fn test_aspect_ratio_to_openai_size() {
        assert_eq!(aspect_ratio_to_openai_size("landscape"), "1792x1024");
        assert_eq!(aspect_ratio_to_openai_size("portrait"), "1024x1792");
        assert_eq!(aspect_ratio_to_openai_size("square"), "1024x1024");
    }

    #[test]
    fn test_get_output_dir_default() {
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::remove_var("HAKIMI_IMAGE_GEN_OUTPUT_DIR");
        }
        let dir = get_output_dir(None);
        // Platform-agnostic check: ensure path contains .hakimi and image_cache components
        let path_str = dir.to_string_lossy();
        assert!(path_str.contains(".hakimi"), "Expected .hakimi in path: {}", path_str);
        assert!(path_str.contains("image_cache"), "Expected image_cache in path: {}", path_str);
    }

    #[test]
    fn test_get_output_dir_custom() {
        let dir = get_output_dir(Some("/tmp/img_test"));
        assert_eq!(dir, PathBuf::from("/tmp/img_test"));
    }

    #[test]
    fn test_get_output_dir_env() {
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::set_var("HAKIMI_IMAGE_GEN_OUTPUT_DIR", "/custom/img/dir");
        }
        let dir = get_output_dir(None);
        assert_eq!(dir, PathBuf::from("/custom/img/dir"));
        unsafe {
            std::env::remove_var("HAKIMI_IMAGE_GEN_OUTPUT_DIR");
        }
    }

    #[test]
    fn test_generate_filename() {
        let filename = generate_filename("img", "png");
        assert!(filename.starts_with("img_"));
        assert!(filename.ends_with(".png"));
        // Should be unique
        let filename2 = generate_filename("img", "png");
        assert_ne!(filename, filename2);
    }

    #[tokio::test]
    async fn test_empty_prompt_rejected() {
        let tool = ImageGenerateTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        let result = tool.execute(&json!({"prompt": ""}), &ctx).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("empty"));
    }

    #[tokio::test]
    async fn test_whitespace_prompt_rejected() {
        let tool = ImageGenerateTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        let result = tool.execute(&json!({"prompt": "   "}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_prompt_rejected() {
        let tool = ImageGenerateTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        let result = tool.execute(&json!({}), &ctx).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("prompt"));
    }

    #[tokio::test]
    async fn test_unsupported_provider_rejected() {
        let tool = ImageGenerateTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        let result = tool
            .execute(&json!({"prompt": "a cat", "provider": "invalid"}), &ctx)
            .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("unsupported"));
    }

    #[tokio::test]
    async fn test_fal_missing_api_key() {
        // Ensure no API key is set
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::remove_var("HAKIMI_IMAGE_GEN_API_KEY");
        }
        let tool = ImageGenerateTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        let result = tool
            .execute(&json!({"prompt": "a cat", "provider": "fal"}), &ctx)
            .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("HAKIMI_IMAGE_GEN_API_KEY"));
    }

    #[tokio::test]
    async fn test_openai_missing_api_key() {
        // Ensure no API key is set
        // SAFETY: tests run single-threaded for env var manipulation
        unsafe {
            std::env::remove_var("HAKIMI_IMAGE_GEN_API_KEY");
        }
        let tool = ImageGenerateTool;
        let ctx = hakimi_common::ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        let result = tool
            .execute(&json!({"prompt": "a cat", "provider": "openai"}), &ctx)
            .await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("HAKIMI_IMAGE_GEN_API_KEY"));
    }
}
