//! Hakimi TUI — a ratatui-based terminal user interface for the Hakimi Agent.

pub mod app;
pub mod ui;

use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// Display message shown in the TUI chat history
// ---------------------------------------------------------------------------

/// Role of a displayed message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
    System,
    Error,
}

/// A single chat message displayed in the TUI.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn tool(name: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: format!("[{}] {}", name.into(), content.into()),
            timestamp: Utc::now(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            role: Role::Error,
            content: content.into(),
            timestamp: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool activity entry for the side panel
// ---------------------------------------------------------------------------

/// Status of a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Success,
    Error,
}

/// A tool call displayed in the tools panel.
#[derive(Debug, Clone)]
pub struct ToolActivity {
    pub name: String,
    pub arguments_summary: String,
    pub status: ToolStatus,
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Agent communication channels
// ---------------------------------------------------------------------------

/// Command sent from the TUI to the agent background task.
#[derive(Debug)]
pub enum AgentCommand {
    Chat(String),
    Shutdown,
}

/// Event sent from the agent background task back to the TUI.
#[derive(Debug)]
pub enum AgentEvent {
    /// Agent is processing (thinking/tool-calling).
    Thinking,
    /// A tool is being called.
    ToolCall { name: String, arguments: String },
    /// A tool call completed.
    ToolResult {
        name: String,
        content: String,
        is_error: bool,
    },
    /// Final text response from the assistant.
    Response(String),
    /// An error occurred.
    Error(String),
    /// Agent finished processing this turn.
    Done,
}

// ---------------------------------------------------------------------------
// Spinner frames
// ---------------------------------------------------------------------------

pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_frames_non_empty() {
        assert!(!SPINNER_FRAMES.is_empty());
    }

    #[test]
    fn role_variants_exist() {
        let roles = [
            Role::User,
            Role::Assistant,
            Role::Tool,
            Role::System,
            Role::Error,
        ];
        assert_eq!(roles.len(), 5);
        // Ensure they are all distinct
        for i in 0..roles.len() {
            for j in (i + 1)..roles.len() {
                assert_ne!(roles[i], roles[j]);
            }
        }
    }

    #[test]
    fn chat_message_user() {
        let msg = ChatMessage::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn chat_message_user_from_string() {
        let msg = ChatMessage::user(String::from("hello"));
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn chat_message_assistant() {
        let msg = ChatMessage::assistant("response");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "response");
    }

    #[test]
    fn chat_message_tool() {
        let msg = ChatMessage::tool("bash", "ls output");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.content, "[bash] ls output");
    }

    #[test]
    fn chat_message_system() {
        let msg = ChatMessage::system("info");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, "info");
    }

    #[test]
    fn chat_message_error() {
        let msg = ChatMessage::error("something broke");
        assert_eq!(msg.role, Role::Error);
        assert_eq!(msg.content, "something broke");
    }

    #[test]
    fn tool_status_variants() {
        let statuses = [ToolStatus::Running, ToolStatus::Success, ToolStatus::Error];
        assert_eq!(statuses.len(), 3);
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j]);
            }
        }
    }

    #[test]
    fn tool_activity_creation() {
        let activity = ToolActivity {
            name: "bash".to_string(),
            arguments_summary: "ls -la".to_string(),
            status: ToolStatus::Running,
            timestamp: Utc::now(),
        };
        assert_eq!(activity.name, "bash");
        assert_eq!(activity.arguments_summary, "ls -la");
        assert_eq!(activity.status, ToolStatus::Running);
    }
}
