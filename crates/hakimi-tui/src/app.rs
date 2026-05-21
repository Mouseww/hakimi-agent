//! Application state and event handling for the Hakimi TUI.

use crate::{
    AgentCommand, AgentEvent, ChatMessage, SPINNER_FRAMES, ToolActivity, ToolStatus,
};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

/// The main application state.
pub struct App {
    /// Chat messages displayed in the main panel.
    pub messages: Vec<ChatMessage>,
    /// Current input text.
    pub input: String,
    /// Cursor position within the input.
    pub cursor_position: usize,
    /// Vertical scroll offset for chat history (0 = bottom/latest).
    pub scroll_offset: usize,
    /// Whether the tools activity panel is visible.
    pub show_tools_panel: bool,
    /// Whether the agent is currently processing.
    pub is_thinking: bool,
    /// Current spinner frame index.
    pub spinner_index: usize,
    /// Whether the application should exit.
    pub should_quit: bool,
    /// Channel to send commands to the agent task.
    pub cmd_tx: mpsc::UnboundedSender<AgentCommand>,
    /// Channel to receive events from the agent task.
    pub event_rx: mpsc::UnboundedReceiver<AgentEvent>,
    /// Recent tool activity for the side panel.
    pub tool_activity: Vec<ToolActivity>,
    /// Model name to display in header.
    pub model_name: String,
    /// Session ID to display in status bar.
    pub session_id: String,
    /// Total tokens used this session.
    pub total_tokens: u32,
    /// Number of API calls made.
    pub api_calls: usize,
}

impl App {
    /// Create a new `App` with the given channels and model info.
    pub fn new(
        cmd_tx: mpsc::UnboundedSender<AgentCommand>,
        event_rx: mpsc::UnboundedReceiver<AgentEvent>,
        model_name: String,
        session_id: String,
    ) -> Self {
        Self {
            messages: vec![ChatMessage::system(
                "Welcome to Hakimi Agent! Type a message and press Enter to chat.",
            )],
            input: String::new(),
            cursor_position: 0,
            scroll_offset: 0,
            show_tools_panel: true,
            is_thinking: false,
            spinner_index: 0,
            should_quit: false,
            cmd_tx,
            event_rx,
            tool_activity: Vec::new(),
            model_name,
            session_id,
            total_tokens: 0,
            api_calls: 0,
        }
    }

    /// Handle a single key event.
    pub fn handle_key_event(&mut self, key: KeyEvent) {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            let _ = self.cmd_tx.send(AgentCommand::Shutdown);
            self.should_quit = true;
            return;
        }

        // Ctrl+L clears screen / resets scroll
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            self.scroll_offset = 0;
            return;
        }

        // Don't process input while agent is thinking (except quit)
        if self.is_thinking {
            // Allow Escape to interrupt (future feature)
            return;
        }

        match key.code {
            // Submit message
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    return;
                }

                // Handle slash commands locally
                if text.starts_with('/') {
                    self.handle_slash_command(&text);
                    self.input.clear();
                    self.cursor_position = 0;
                    return;
                }

                // Display user message
                self.messages.push(ChatMessage::user(&text));
                self.scroll_offset = 0;

                // Send to agent
                if self.cmd_tx.send(AgentCommand::Chat(text)).is_err() {
                    self.messages
                        .push(ChatMessage::error("Failed to send message to agent."));
                } else {
                    self.is_thinking = true;
                }

                self.input.clear();
                self.cursor_position = 0;
            }

            // Scroll up
            KeyCode::Up => {
                let max_scroll = self.messages.len().saturating_sub(1);
                if self.scroll_offset < max_scroll {
                    self.scroll_offset += 1;
                }
            }

            // Scroll down
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }

            // Page up
            KeyCode::PageUp => {
                let max_scroll = self.messages.len().saturating_sub(1);
                self.scroll_offset = (self.scroll_offset + 10).min(max_scroll);
            }

            // Page down
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
            }

            // Toggle tools panel
            KeyCode::Tab => {
                self.show_tools_panel = !self.show_tools_panel;
            }

            // Backspace
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    let before = &self.input[..self.cursor_position - 1];
                    let after = &self.input[self.cursor_position..];
                    self.input = format!("{before}{after}");
                    self.cursor_position -= 1;
                }
            }

            // Delete
            KeyCode::Delete => {
                if self.cursor_position < self.input.len() {
                    let before = &self.input[..self.cursor_position];
                    let after = &self.input[self.cursor_position + 1..];
                    self.input = format!("{before}{after}");
                }
            }

            // Home
            KeyCode::Home => {
                self.cursor_position = 0;
            }

            // End
            KeyCode::End => {
                self.cursor_position = self.input.len();
            }

            // Left arrow
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
            }

            // Right arrow
            KeyCode::Right => {
                if self.cursor_position < self.input.len() {
                    self.cursor_position += 1;
                }
            }

            // Regular character input
            KeyCode::Char(c) => {
                let before = &self.input[..self.cursor_position];
                let after = &self.input[self.cursor_position..];
                self.input = format!("{before}{c}{after}");
                self.cursor_position += 1;
            }

            // Escape — could be used for interrupt in future
            KeyCode::Esc => {
                // Currently no-op; could interrupt agent
            }

            _ => {}
        }
    }

    /// Handle slash commands locally (without sending to agent).
    fn handle_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        match parts[0] {
            "/help" => {
                self.messages.push(ChatMessage::system(
                    "Commands:\n  /help     — Show this help\n  /clear    — Clear chat history\n  /tools    — Toggle tools panel\n  /quit     — Exit the application",
                ));
            }
            "/clear" => {
                self.messages.clear();
                self.messages
                    .push(ChatMessage::system("Chat history cleared."));
                self.scroll_offset = 0;
            }
            "/tools" => {
                self.show_tools_panel = !self.show_tools_panel;
                let state = if self.show_tools_panel { "on" } else { "off" };
                self.messages
                    .push(ChatMessage::system(format!("Tools panel: {state}")));
            }
            "/quit" | "/exit" => {
                let _ = self.cmd_tx.send(AgentCommand::Shutdown);
                self.should_quit = true;
            }
            _ => {
                self.messages.push(ChatMessage::error(format!(
                    "Unknown command: {cmd}. Type /help for available commands."
                )));
            }
        }
    }

    /// Process incoming agent events (non-blocking).
    pub fn poll_agent_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AgentEvent::Thinking => {
                    self.is_thinking = true;
                }

                AgentEvent::ToolCall { name, arguments } => {
                    // Show in chat
                    let args_preview: String = arguments.chars().take(200).collect();
                    self.messages.push(ChatMessage::tool(
                        &name,
                        format!("calling with: {args_preview}"),
                    ));

                    // Show in tool activity panel
                    self.tool_activity.push(ToolActivity {
                        name: name.clone(),
                        arguments_summary: {
                            let preview: String = arguments.chars().take(80).collect();
                            if arguments.len() > 80 {
                                format!("{preview}...")
                            } else {
                                preview
                            }
                        },
                        status: ToolStatus::Running,
                        timestamp: Utc::now(),
                    });

                    self.scroll_offset = 0;
                }

                AgentEvent::ToolResult {
                    name,
                    content,
                    is_error,
                } => {
                    // Update last matching tool activity status
                    if let Some(activity) = self
                        .tool_activity
                        .iter_mut()
                        .rev()
                        .find(|a| a.name == name && a.status == ToolStatus::Running)
                    {
                        activity.status = if is_error {
                            ToolStatus::Error
                        } else {
                            ToolStatus::Success
                        };
                    }

                    // Show result in chat (truncated)
                    let preview: String = content.chars().take(500).collect();
                    let suffix = if content.len() > 500 { "..." } else { "" };
                    if is_error {
                        self.messages.push(ChatMessage::error(format!(
                            "[{name}] {preview}{suffix}"
                        )));
                    } else {
                        self.messages.push(ChatMessage::tool(
                            &name,
                            format!("result: {preview}{suffix}"),
                        ));
                    }

                    self.scroll_offset = 0;
                }

                AgentEvent::Response(text) => {
                    self.is_thinking = false;
                    if !text.is_empty() {
                        self.messages.push(ChatMessage::assistant(&text));
                    }
                    self.scroll_offset = 0;
                    self.api_calls += 1;
                }

                AgentEvent::Error(err) => {
                    self.is_thinking = false;
                    self.messages.push(ChatMessage::error(&err));
                    self.scroll_offset = 0;
                }

                AgentEvent::Done => {
                    self.is_thinking = false;
                }
            }
        }
    }

    /// Advance the spinner animation.
    pub fn tick(&mut self) {
        if self.is_thinking {
            self.spinner_index = (self.spinner_index + 1) % SPINNER_FRAMES.len();
        }
    }

    /// Get the current spinner frame character.
    pub fn spinner_frame(&self) -> &str {
        SPINNER_FRAMES[self.spinner_index]
    }
}
