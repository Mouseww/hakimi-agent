//! Clarify tool — allows the agent to ask structured questions to the user.
//!
//! Supports multiple-choice and open-ended questions with structured JSON output.

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};

use crate::Tool;

/// Built-in tool for asking clarification questions to the user.
pub struct ClarifyTool;

#[async_trait]
impl Tool for ClarifyTool {
    fn name(&self) -> &str {
        "clarify"
    }

    fn toolset(&self) -> &str {
        "interaction"
    }

    fn description(&self) -> &str {
        "Ask the user a clarification question. Supports multiple-choice (up to 4 options \
         plus 'Other') or open-ended questions. Returns structured JSON with the user's answer."
    }

    fn emoji(&self) -> &str {
        "\u{2753}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user."
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional list of multiple-choice options (max 4). If omitted, the question is open-ended.",
                    "maxItems": 4
                },
                "context": {
                    "type": "string",
                    "description": "Optional additional context to help the user understand the question."
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let question = args
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: question".into()))?;

        let options: Option<Vec<String>> =
            args.get("options").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or("");

        // Build the structured clarify request.
        let request = ClarifyRequest {
            question: question.to_string(),
            options: options.unwrap_or_default(),
            context: context.to_string(),
            session_id: ctx.session_id.clone(),
        };

        // In a real implementation, this would:
        // 1. Present the question to the user via the active platform (CLI, gateway, etc.)
        // 2. Wait for the user's response
        // 3. Return the structured result
        //
        // For now, we return the structured request as JSON so the framework can handle it.
        Ok(
            serde_json::to_string_pretty(&request.to_output()).unwrap_or_else(|_| {
                json!({"error": "failed to serialize clarify request"}).to_string()
            }),
        )
    }
}

/// Internal representation of a clarify request.
struct ClarifyRequest {
    question: String,
    options: Vec<String>,
    context: String,
    session_id: String,
}

impl ClarifyRequest {
    /// Convert to the output JSON format.
    fn to_output(&self) -> JsonValue {
        let mut output = json!({
            "type": "clarify",
            "question": self.question,
            "session_id": self.session_id,
        });

        if !self.options.is_empty() {
            let options_with_index: Vec<JsonValue> = self
                .options
                .iter()
                .enumerate()
                .map(|(i, opt)| {
                    json!({
                        "index": i + 1,
                        "text": opt
                    })
                })
                .collect();

            output["options"] = json!(options_with_index);
            output["has_other"] = json!(true);
            output["format"] = json!("multiple_choice");
        } else {
            output["format"] = json!("open_ended");
        }

        if !self.context.is_empty() {
            output["context"] = json!(self.context);
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            user_id: Some("user1".to_string()),
            task_id: None,
            workdir: ".".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_tool_metadata() {
        let tool = ClarifyTool;
        assert_eq!(tool.name(), "clarify");
        assert_eq!(tool.toolset(), "interaction");
    }

    #[test]
    fn test_schema_has_question_required() {
        let tool = ClarifyTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("question")));
    }

    #[tokio::test]
    async fn test_open_ended_question() {
        let tool = ClarifyTool;
        let ctx = make_ctx();
        let result = tool
            .execute(&json!({"question": "What language?"}), &ctx)
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["type"], "clarify");
        assert_eq!(parsed["question"], "What language?");
        assert_eq!(parsed["format"], "open_ended");
        assert!(parsed["options"].is_null());
    }

    #[tokio::test]
    async fn test_multiple_choice_question() {
        let tool = ClarifyTool;
        let ctx = make_ctx();
        let result = tool
            .execute(
                &json!({
                    "question": "Which framework?",
                    "options": ["React", "Vue", "Angular", "Svelte"]
                }),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["format"], "multiple_choice");
        assert_eq!(parsed["has_other"], true);
        let opts = parsed["options"].as_array().unwrap();
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[0]["index"], 1);
        assert_eq!(opts[0]["text"], "React");
    }

    #[tokio::test]
    async fn test_question_with_context() {
        let tool = ClarifyTool;
        let ctx = make_ctx();
        let result = tool
            .execute(
                &json!({
                    "question": "Which version?",
                    "context": "We need to choose a Python version for the project."
                }),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        assert_eq!(
            parsed["context"],
            "We need to choose a Python version for the project."
        );
    }

    #[tokio::test]
    async fn test_missing_question_fails() {
        let tool = ClarifyTool;
        let ctx = make_ctx();
        let result = tool.execute(&json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_clarify_request_output_format() {
        let req = ClarifyRequest {
            question: "test?".to_string(),
            options: vec!["a".to_string(), "b".to_string()],
            context: "ctx".to_string(),
            session_id: "s1".to_string(),
        };
        let output = req.to_output();
        assert_eq!(output["type"], "clarify");
        assert_eq!(output["format"], "multiple_choice");
        assert_eq!(output["has_other"], true);
    }

    #[test]
    fn test_clarify_request_open_ended() {
        let req = ClarifyRequest {
            question: "test?".to_string(),
            options: vec![],
            context: "".to_string(),
            session_id: "s1".to_string(),
        };
        let output = req.to_output();
        assert_eq!(output["format"], "open_ended");
        assert!(output["context"].is_null());
    }

    #[test]
    fn test_clarify_request_single_option() {
        let req = ClarifyRequest {
            question: "confirm?".to_string(),
            options: vec!["Yes".to_string()],
            context: "".to_string(),
            session_id: "s2".to_string(),
        };
        let output = req.to_output();
        assert_eq!(output["format"], "multiple_choice");
        assert_eq!(output["has_other"], true);
        let opts = output["options"].as_array().unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0]["index"], 1);
        assert_eq!(opts[0]["text"], "Yes");
    }

    #[test]
    fn test_clarify_request_option_indexing() {
        let req = ClarifyRequest {
            question: "pick?".to_string(),
            options: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            context: "".to_string(),
            session_id: "s3".to_string(),
        };
        let output = req.to_output();
        let opts = output["options"].as_array().unwrap();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0]["index"], 1);
        assert_eq!(opts[1]["index"], 2);
        assert_eq!(opts[2]["index"], 3);
        assert_eq!(opts[0]["text"], "A");
        assert_eq!(opts[1]["text"], "B");
        assert_eq!(opts[2]["text"], "C");
    }

    #[test]
    fn test_clarify_request_session_id_preserved() {
        let req = ClarifyRequest {
            question: "q?".to_string(),
            options: vec![],
            context: "".to_string(),
            session_id: "my-session-123".to_string(),
        };
        let output = req.to_output();
        assert_eq!(output["session_id"], "my-session-123");
    }

    #[test]
    fn test_clarify_request_with_long_context() {
        let long_ctx = "A".repeat(1000);
        let req = ClarifyRequest {
            question: "details?".to_string(),
            options: vec![],
            context: long_ctx.clone(),
            session_id: "s4".to_string(),
        };
        let output = req.to_output();
        assert_eq!(output["context"], long_ctx);
    }

    #[tokio::test]
    async fn test_execute_with_empty_options_array() {
        let tool = ClarifyTool;
        let ctx = make_ctx();
        let result = tool
            .execute(&json!({"question": "What?", "options": []}), &ctx)
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        // Empty options treated as open-ended
        assert_eq!(parsed["format"], "open_ended");
        assert!(parsed["options"].is_null());
    }

    #[tokio::test]
    async fn test_execute_with_context_and_options() {
        let tool = ClarifyTool;
        let ctx = make_ctx();
        let result = tool
            .execute(
                &json!({
                    "question": "Pick a color",
                    "options": ["Red", "Blue"],
                    "context": "Choose your favorite."
                }),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["format"], "multiple_choice");
        assert_eq!(parsed["context"], "Choose your favorite.");
        let opts = parsed["options"].as_array().unwrap();
        assert_eq!(opts.len(), 2);
    }
}
