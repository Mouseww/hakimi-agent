//! Application state and event handling for the Hakimi TUI.

use crate::{AgentCommand, AgentEvent, ChatMessage, SPINNER_FRAMES, ToolActivity, ToolStatus};
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
                        self.messages
                            .push(ChatMessage::error(format!("[{name}] {preview}{suffix}")));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    /// Helper: create an App with dummy channels. Returns (app, cmd_rx, event_tx)
    /// so the receivers stay alive for the duration of the test.
    fn make_app() -> (
        App,
        mpsc::UnboundedReceiver<crate::AgentCommand>,
        mpsc::UnboundedSender<crate::AgentEvent>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        (
            App::new(
                cmd_tx,
                event_rx,
                "test-model".to_string(),
                "test-session-123".to_string(),
            ),
            cmd_rx,
            event_tx,
        )
    }

    /// Convenience: create just an App (for tests that don't need the channels alive).
    fn make_app_simple() -> App {
        make_app().0
    }

    /// Helper: build a KeyEvent from a KeyCode with no modifiers.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Helper: build a KeyEvent with a modifier.
    fn key_with_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // ---------------------------------------------------------------
    // App::new initial state
    // ---------------------------------------------------------------

    #[test]
    fn new_app_has_welcome_message() {
        let app = make_app_simple();
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, crate::Role::System);
        assert!(app.messages[0].content.contains("Welcome"));
    }

    #[test]
    fn new_app_has_empty_input() {
        let (app, _cmd_rx, _event_tx) = make_app();
        assert!(app.input.is_empty());
        assert_eq!(app.cursor_position, 0);
    }

    #[test]
    fn new_app_defaults() {
        let (app, _cmd_rx, _event_tx) = make_app();
        assert_eq!(app.scroll_offset, 0);
        assert!(app.show_tools_panel);
        assert!(!app.is_thinking);
        assert_eq!(app.spinner_index, 0);
        assert!(!app.should_quit);
        assert!(app.tool_activity.is_empty());
        assert_eq!(app.model_name, "test-model");
        assert_eq!(app.session_id, "test-session-123");
        assert_eq!(app.total_tokens, 0);
        assert_eq!(app.api_calls, 0);
    }

    // ---------------------------------------------------------------
    // Character input
    // ---------------------------------------------------------------

    #[test]
    fn handle_char_adds_to_input() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('h')));
        app.handle_key_event(key(KeyCode::Char('i')));
        assert_eq!(app.input, "hi");
        assert_eq!(app.cursor_position, 2);
    }

    #[test]
    fn handle_char_at_cursor_position() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Char('c')));
        // Move cursor left
        app.handle_key_event(key(KeyCode::Left));
        // Insert 'b' between 'a' and 'c'
        app.handle_key_event(key(KeyCode::Char('b')));
        assert_eq!(app.input, "abc");
        assert_eq!(app.cursor_position, 2);
    }

    // ---------------------------------------------------------------
    // Backspace
    // ---------------------------------------------------------------

    #[test]
    fn handle_backspace_removes_char() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Char('b')));
        app.handle_key_event(key(KeyCode::Backspace));
        assert_eq!(app.input, "a");
        assert_eq!(app.cursor_position, 1);
    }

    #[test]
    fn handle_backspace_at_start_is_noop() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Backspace));
        assert!(app.input.is_empty());
        assert_eq!(app.cursor_position, 0);
    }

    // ---------------------------------------------------------------
    // Enter — empty input is ignored
    // ---------------------------------------------------------------

    #[test]
    fn empty_input_enter_is_ignored() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        let initial_msg_count = app.messages.len();
        app.handle_key_event(key(KeyCode::Enter));
        assert_eq!(app.messages.len(), initial_msg_count);
        assert!(app.input.is_empty());
    }

    #[test]
    fn whitespace_only_input_enter_is_ignored() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        let initial_msg_count = app.messages.len();
        app.handle_key_event(key(KeyCode::Char(' ')));
        app.handle_key_event(key(KeyCode::Char(' ')));
        app.handle_key_event(key(KeyCode::Enter));
        // No new messages should be added (whitespace-only input is ignored)
        assert_eq!(app.messages.len(), initial_msg_count);
    }

    // ---------------------------------------------------------------
    // Enter — sends message and clears input
    // ---------------------------------------------------------------

    #[test]
    fn handle_enter_sends_message_and_clears() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('h')));
        app.handle_key_event(key(KeyCode::Char('i')));
        app.handle_key_event(key(KeyCode::Enter));
        assert!(app.input.is_empty());
        assert_eq!(app.cursor_position, 0);
        // Should now have welcome + user message = 2
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[1].role, crate::Role::User);
        assert_eq!(app.messages[1].content, "hi");
        assert!(app.is_thinking);
    }

    // ---------------------------------------------------------------
    // Scroll
    // ---------------------------------------------------------------

    #[test]
    fn scroll_up_increments_offset() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        // Add a few messages so we can scroll
        app.messages.push(crate::ChatMessage::user("msg1"));
        app.messages.push(crate::ChatMessage::assistant("msg2"));
        app.messages.push(crate::ChatMessage::user("msg3"));
        app.handle_key_event(key(KeyCode::Up));
        assert_eq!(app.scroll_offset, 1);
        app.handle_key_event(key(KeyCode::Up));
        assert_eq!(app.scroll_offset, 2);
    }

    #[test]
    fn scroll_up_clamped_at_max() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("msg1"));
        // Only 2 messages, max_scroll = 1
        for _ in 0..10 {
            app.handle_key_event(key(KeyCode::Up));
        }
        assert_eq!(app.scroll_offset, 1); // messages.len() - 1
    }

    #[test]
    fn scroll_down_decrements_offset() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("msg1"));
        app.messages.push(crate::ChatMessage::assistant("msg2"));
        app.messages.push(crate::ChatMessage::user("msg3"));
        app.scroll_offset = 2;
        app.handle_key_event(key(KeyCode::Down));
        assert_eq!(app.scroll_offset, 1);
        app.handle_key_event(key(KeyCode::Down));
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn scroll_down_floor_at_zero() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.scroll_offset = 0;
        app.handle_key_event(key(KeyCode::Down));
        assert_eq!(app.scroll_offset, 0);
    }

    // ---------------------------------------------------------------
    // Tab — toggle tools panel
    // ---------------------------------------------------------------

    #[test]
    fn toggle_tools_panel() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        assert!(app.show_tools_panel);
        app.handle_key_event(key(KeyCode::Tab));
        assert!(!app.show_tools_panel);
        app.handle_key_event(key(KeyCode::Tab));
        assert!(app.show_tools_panel);
    }

    // ---------------------------------------------------------------
    // Slash commands
    // ---------------------------------------------------------------

    #[test]
    fn slash_clear_resets_messages() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("hello"));
        app.messages.push(crate::ChatMessage::assistant("world"));
        // Type /clear
        for c in "/clear".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));
        // Should only have the "Chat history cleared." message
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, crate::Role::System);
        assert!(app.messages[0].content.contains("cleared"));
        assert!(app.input.is_empty());
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn slash_quit_sets_should_quit() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/quit".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));
        assert!(app.should_quit);
        assert!(app.input.is_empty());
    }

    #[test]
    fn slash_exit_sets_should_quit() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/exit".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));
        assert!(app.should_quit);
    }

    #[test]
    fn slash_help_shows_help() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/help".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));
        // welcome + help
        assert_eq!(app.messages.len(), 2);
        assert!(app.messages[1].content.contains("/help"));
    }

    #[test]
    fn slash_tools_toggles_panel() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        assert!(app.show_tools_panel);
        for c in "/tools".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));
        assert!(!app.show_tools_panel);
    }

    #[test]
    fn unknown_slash_command_shows_error() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/foobar".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[1].role, crate::Role::Error);
        assert!(app.messages[1].content.contains("Unknown command"));
    }

    // ---------------------------------------------------------------
    // Ctrl+C quits
    // ---------------------------------------------------------------

    #[test]
    fn ctrl_c_quits() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key_with_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    // ---------------------------------------------------------------
    // Ctrl+L resets scroll
    // ---------------------------------------------------------------

    #[test]
    fn ctrl_l_resets_scroll() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.scroll_offset = 5;
        app.handle_key_event(key_with_mod(KeyCode::Char('l'), KeyModifiers::CONTROL));
        assert_eq!(app.scroll_offset, 0);
    }

    // ---------------------------------------------------------------
    // Input blocked while thinking
    // ---------------------------------------------------------------

    #[test]
    fn input_blocked_while_thinking() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.is_thinking = true;
        let initial_input = app.input.clone();
        app.handle_key_event(key(KeyCode::Char('a')));
        assert_eq!(app.input, initial_input);
    }

    // ---------------------------------------------------------------
    // Cursor movement
    // ---------------------------------------------------------------

    #[test]
    fn home_moves_cursor_to_start() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Char('b')));
        app.handle_key_event(key(KeyCode::Home));
        assert_eq!(app.cursor_position, 0);
    }

    #[test]
    fn end_moves_cursor_to_end() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Char('b')));
        app.handle_key_event(key(KeyCode::Home));
        app.handle_key_event(key(KeyCode::End));
        assert_eq!(app.cursor_position, 2);
    }

    #[test]
    fn left_right_arrow_movement() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Char('b')));
        app.handle_key_event(key(KeyCode::Left));
        assert_eq!(app.cursor_position, 1);
        app.handle_key_event(key(KeyCode::Right));
        assert_eq!(app.cursor_position, 2);
    }

    #[test]
    fn left_arrow_at_start_is_noop() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Left));
        assert_eq!(app.cursor_position, 0);
    }

    // ---------------------------------------------------------------
    // Delete key
    // ---------------------------------------------------------------

    #[test]
    fn delete_removes_char_after_cursor() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Char('b')));
        app.handle_key_event(key(KeyCode::Char('c')));
        app.handle_key_event(key(KeyCode::Home));
        app.handle_key_event(key(KeyCode::Delete));
        assert_eq!(app.input, "bc");
    }

    #[test]
    fn delete_at_end_is_noop() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_key_event(key(KeyCode::Char('a')));
        app.handle_key_event(key(KeyCode::Delete));
        assert_eq!(app.input, "a");
    }

    // ---------------------------------------------------------------
    // PageUp / PageDown
    // ---------------------------------------------------------------

    #[test]
    fn page_up_scrolls_by_10() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for i in 0..20 {
            app.messages
                .push(crate::ChatMessage::user(format!("msg{i}")));
        }
        app.handle_key_event(key(KeyCode::PageUp));
        assert_eq!(app.scroll_offset, 10);
    }

    #[test]
    fn page_down_unscrolls_by_10() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.scroll_offset = 15;
        app.handle_key_event(key(KeyCode::PageDown));
        assert_eq!(app.scroll_offset, 5);
    }

    // ---------------------------------------------------------------
    // Spinner / tick
    // ---------------------------------------------------------------

    #[test]
    fn tick_advances_spinner_when_thinking() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.is_thinking = true;
        let initial = app.spinner_index;
        app.tick();
        assert_eq!(
            app.spinner_index,
            (initial + 1) % crate::SPINNER_FRAMES.len()
        );
    }

    #[test]
    fn tick_noop_when_not_thinking() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.tick();
        assert_eq!(app.spinner_index, 0);
    }

    #[test]
    fn spinner_frame_returns_valid_frame() {
        let (app, _cmd_rx, _event_tx) = make_app();
        let frame = app.spinner_frame();
        assert!(crate::SPINNER_FRAMES.contains(&frame));
    }

    // ---------------------------------------------------------------
    // poll_agent_events
    // ---------------------------------------------------------------

    #[test]
    fn poll_response_stops_thinking_and_adds_message() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.is_thinking = true;

        event_tx
            .send(crate::AgentEvent::Response("hello".to_string()))
            .unwrap();
        app.poll_agent_events();

        assert!(!app.is_thinking);
        assert_eq!(app.messages.last().unwrap().content, "hello");
        assert_eq!(app.messages.last().unwrap().role, crate::Role::Assistant);
        assert_eq!(app.api_calls, 1);
    }

    #[test]
    fn poll_error_stops_thinking_and_adds_error() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.is_thinking = true;

        event_tx
            .send(crate::AgentEvent::Error("oops".to_string()))
            .unwrap();
        app.poll_agent_events();

        assert!(!app.is_thinking);
        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(app.messages.last().unwrap().content.contains("oops"));
    }

    #[test]
    fn poll_done_stops_thinking() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.is_thinking = true;

        event_tx.send(crate::AgentEvent::Done).unwrap();
        app.poll_agent_events();

        assert!(!app.is_thinking);
    }

    #[test]
    fn poll_tool_call_adds_activity() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());

        event_tx
            .send(crate::AgentEvent::ToolCall {
                name: "bash".to_string(),
                arguments: "ls -la".to_string(),
            })
            .unwrap();
        app.poll_agent_events();

        assert_eq!(app.tool_activity.len(), 1);
        assert_eq!(app.tool_activity[0].name, "bash");
        assert_eq!(app.tool_activity[0].status, crate::ToolStatus::Running);
    }

    #[test]
    fn poll_tool_result_updates_activity_status() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());

        event_tx
            .send(crate::AgentEvent::ToolCall {
                name: "bash".to_string(),
                arguments: "ls".to_string(),
            })
            .unwrap();
        event_tx
            .send(crate::AgentEvent::ToolResult {
                name: "bash".to_string(),
                content: "file.txt".to_string(),
                is_error: false,
            })
            .unwrap();
        app.poll_agent_events();

        assert_eq!(app.tool_activity.len(), 1);
        assert_eq!(app.tool_activity[0].status, crate::ToolStatus::Success);
    }

    #[test]
    fn poll_tool_result_error_updates_activity_status() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());

        event_tx
            .send(crate::AgentEvent::ToolCall {
                name: "bash".to_string(),
                arguments: "ls".to_string(),
            })
            .unwrap();
        event_tx
            .send(crate::AgentEvent::ToolResult {
                name: "bash".to_string(),
                content: "permission denied".to_string(),
                is_error: true,
            })
            .unwrap();
        app.poll_agent_events();

        assert_eq!(app.tool_activity.len(), 1);
        assert_eq!(app.tool_activity[0].status, crate::ToolStatus::Error);
    }
}
