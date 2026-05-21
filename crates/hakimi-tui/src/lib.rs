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
    ToolCall {
        name: String,
        arguments: String,
    },
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
