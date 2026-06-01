use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{SecondsFormat, Utc};
use hakimi_common::{Message, MessageRole, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryConfig {
    pub output_dir: PathBuf,
    pub success_filename: String,
    pub failure_filename: String,
}

impl TrajectoryConfig {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
            success_filename: "trajectory_samples.jsonl".to_string(),
            failure_filename: "failed_trajectories.jsonl".to_string(),
        }
    }

    fn path_for_completion(&self, completed: bool) -> PathBuf {
        self.output_dir.join(if completed {
            &self.success_filename
        } else {
            &self.failure_filename
        })
    }
}

impl Default for TrajectoryConfig {
    fn default() -> Self {
        Self::new(".")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrajectoryMessage {
    #[serde(rename = "from")]
    pub from_role: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrajectoryEntry {
    pub conversations: Vec<TrajectoryMessage>,
    pub timestamp: String,
    pub model: String,
    pub completed: bool,
}

pub fn convert_scratchpad_to_think(content: &str) -> String {
    if content.is_empty() || !content.contains("<REASONING_SCRATCHPAD>") {
        return content.to_string();
    }

    content
        .replace("<REASONING_SCRATCHPAD>", "<think>")
        .replace("</REASONING_SCRATCHPAD>", "</think>")
}

pub fn has_incomplete_scratchpad(content: &str) -> bool {
    !content.is_empty()
        && content.contains("<REASONING_SCRATCHPAD>")
        && !content.contains("</REASONING_SCRATCHPAD>")
}

pub fn convert_to_trajectory_format(messages: &[Message]) -> Vec<TrajectoryMessage> {
    let mut trajectory = Vec::new();
    let mut index = 0;

    while index < messages.len() {
        let msg = &messages[index];
        match msg.role {
            MessageRole::System => trajectory.push(TrajectoryMessage {
                from_role: "system".to_string(),
                value: message_text_value(msg),
            }),
            MessageRole::User => trajectory.push(TrajectoryMessage {
                from_role: "human".to_string(),
                value: message_text_value(msg),
            }),
            MessageRole::Assistant => {
                trajectory.push(TrajectoryMessage {
                    from_role: "gpt".to_string(),
                    value: assistant_value(msg),
                });

                if let Some(tool_calls) = msg.tool_calls.as_ref()
                    && !tool_calls.is_empty()
                {
                    let mut responses = Vec::new();
                    let mut next = index + 1;
                    while next < messages.len() && messages[next].role == MessageRole::Tool {
                        let fallback_name = tool_calls
                            .get(responses.len())
                            .map(|call| call.name.as_str())
                            .unwrap_or("unknown");
                        responses.push(tool_response_value(&messages[next], fallback_name));
                        next += 1;
                    }
                    if !responses.is_empty() {
                        trajectory.push(TrajectoryMessage {
                            from_role: "tool".to_string(),
                            value: responses.join("\n"),
                        });
                        index = next - 1;
                    }
                }
            }
            MessageRole::Tool => trajectory.push(TrajectoryMessage {
                from_role: "tool".to_string(),
                value: tool_response_value(msg, msg.name.as_deref().unwrap_or("unknown")),
            }),
        }

        index += 1;
    }

    trajectory
}

pub fn save_trajectory(
    messages: &[Message],
    model: &str,
    completed: bool,
    config: &TrajectoryConfig,
) -> io::Result<PathBuf> {
    let path = config.path_for_completion(completed);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let entry = TrajectoryEntry {
        conversations: convert_to_trajectory_format(messages),
        timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
        model: model.to_string(),
        completed,
    };

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, &entry).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    Ok(path)
}

fn message_text_value(msg: &Message) -> String {
    let mut value = msg.content.clone().unwrap_or_default();
    if let Some(images) = msg.images.as_ref()
        && !images.is_empty()
    {
        if !value.is_empty() {
            value.push('\n');
        }
        value.push_str(&format!(
            "[image attachments omitted from text trajectory: {}]",
            images.len()
        ));
    }
    value
}

fn assistant_value(msg: &Message) -> String {
    let mut value = String::new();

    if let Some(reasoning) = msg
        .reasoning
        .as_deref()
        .or(msg.reasoning_content.as_deref())
        && !reasoning.trim().is_empty()
    {
        value.push_str("<think>\n");
        value.push_str(reasoning.trim());
        value.push_str("\n</think>\n");
    }

    if let Some(content) = msg.content.as_deref()
        && !content.trim().is_empty()
    {
        value.push_str(&convert_scratchpad_to_think(content));
        value.push('\n');
    }

    if let Some(tool_calls) = msg.tool_calls.as_ref() {
        for call in tool_calls {
            value.push_str("<tool_call>\n");
            value.push_str(&tool_call_json(call));
            value.push_str("\n</tool_call>\n");
        }
    }

    if !value.contains("<think>") {
        value.insert_str(0, "<think>\n</think>\n");
    }

    value.trim_end().to_string()
}

fn tool_call_json(call: &ToolCall) -> String {
    let arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
    json!({
        "name": call.name.clone(),
        "arguments": arguments,
    })
    .to_string()
}

fn tool_response_value(msg: &Message, fallback_name: &str) -> String {
    let content = msg.content.as_deref().unwrap_or_default();
    let parsed_content = if content.trim_start().starts_with(['{', '[']) {
        serde_json::from_str::<serde_json::Value>(content)
            .unwrap_or_else(|_| serde_json::Value::String(content.to_string()))
    } else {
        serde_json::Value::String(content.to_string())
    };

    let response = json!({
        "tool_call_id": msg.tool_call_id.as_deref().unwrap_or_default(),
        "name": msg.name.as_deref().unwrap_or(fallback_name),
        "content": parsed_content,
    });

    format!("<tool_response>\n{}\n</tool_response>", response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::{ImageContent, ToolCall};
    use std::path::Path;

    #[test]
    fn converts_scratchpad_tags_to_think_tags() {
        let converted = convert_scratchpad_to_think(
            "<REASONING_SCRATCHPAD>plan</REASONING_SCRATCHPAD>\nanswer",
        );
        assert_eq!(converted, "<think>plan</think>\nanswer");
        assert!(has_incomplete_scratchpad("<REASONING_SCRATCHPAD>plan"));
        assert!(!has_incomplete_scratchpad(
            "<REASONING_SCRATCHPAD>plan</REASONING_SCRATCHPAD>"
        ));
    }

    #[test]
    fn converts_messages_to_sharegpt_roles_and_tool_xml() {
        let messages = vec![
            Message::system("system prompt"),
            Message::user("find version"),
            Message {
                role: MessageRole::Assistant,
                content: Some("I should inspect the environment.".to_string()),
                images: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "terminal".to_string(),
                    arguments: r#"{"command":"python --version"}"#.to_string(),
                    index: None,
                }]),
                tool_call_id: None,
                name: None,
                reasoning: Some("Need a command.".to_string()),
                reasoning_content: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            },
            Message::tool_result("call-1", "terminal", r#"{"stdout":"Python 3.11"}"#),
            Message::assistant("Python 3.11 is installed."),
        ];

        let trajectory = convert_to_trajectory_format(&messages);

        assert_eq!(trajectory[0].from_role, "system");
        assert_eq!(trajectory[1].from_role, "human");
        assert_eq!(trajectory[2].from_role, "gpt");
        assert!(
            trajectory[2]
                .value
                .contains("<think>\nNeed a command.\n</think>")
        );
        assert!(trajectory[2].value.contains("<tool_call>"));
        assert!(trajectory[2].value.contains("\"name\":\"terminal\""));
        assert_eq!(trajectory[3].from_role, "tool");
        assert!(trajectory[3].value.contains("<tool_response>"));
        assert!(trajectory[3].value.contains("\"tool_call_id\":\"call-1\""));
        assert_eq!(trajectory[4].from_role, "gpt");
    }

    #[test]
    fn includes_text_marker_for_image_attachments() {
        let mut msg = Message::user("describe this");
        msg.images = Some(vec![ImageContent {
            mime_type: "image/png".to_string(),
            data: "base64".to_string(),
        }]);

        let trajectory = convert_to_trajectory_format(&[msg]);
        assert_eq!(
            trajectory[0].value,
            "describe this\n[image attachments omitted from text trajectory: 1]"
        );
    }

    #[test]
    fn appends_successful_trajectory_jsonl() {
        let dir =
            std::env::temp_dir().join(format!("hakimi-trajectory-test-{}", uuid::Uuid::new_v4()));
        let config = TrajectoryConfig::new(&dir);
        let path = save_trajectory(
            &[Message::user("hello"), Message::assistant("hi")],
            "test-model",
            true,
            &config,
        )
        .expect("trajectory should save");

        let saved = std::fs::read_to_string(&path).expect("saved jsonl should be readable");
        let entry: TrajectoryEntry =
            serde_json::from_str(saved.trim()).expect("jsonl entry should deserialize");
        assert!(entry.completed);
        assert_eq!(entry.model, "test-model");
        assert_eq!(entry.conversations.len(), 2);
        assert!(Path::new(&path).ends_with("trajectory_samples.jsonl"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
