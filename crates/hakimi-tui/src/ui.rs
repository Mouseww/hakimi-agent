//! TUI rendering for the Hakimi Agent.

use crate::app::App;
use crate::{Role, ToolStatus};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

/// Color scheme constants
const COLOR_USER: Color = Color::Blue;
const COLOR_ASSISTANT: Color = Color::Green;
const COLOR_TOOL: Color = Color::Yellow;
const COLOR_ERROR: Color = Color::Red;
const COLOR_SYSTEM: Color = Color::DarkGray;
const COLOR_HEADER_BG: Color = Color::Rgb(30, 30, 50);
const COLOR_STATUS_BG: Color = Color::Rgb(20, 20, 40);
const COLOR_INPUT_BG: Color = Color::Rgb(25, 25, 45);
const COLOR_PANEL_BG: Color = Color::Rgb(20, 20, 35);
const COLOR_ACCENT: Color = Color::Cyan;

/// Render the full TUI layout.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main area (chat + tools panel)
            Constraint::Length(3), // Input area
            Constraint::Length(1), // Status bar
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_main_area(frame, app, chunks[1]);
    render_input(frame, app, chunks[2]);
    render_status_bar(frame, app, chunks[3]);
}

/// Render the header bar with title and model info.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let thinking_indicator = if app.is_thinking {
        format!("  {} Thinking...", app.spinner_frame())
    } else {
        String::new()
    };

    let title_line = Line::from(vec![
        Span::styled(
            " ◆ ",
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Hakimi Agent",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {} ", app.model_name),
            Style::default().fg(COLOR_SYSTEM),
        ),
        Span::styled(thinking_indicator, Style::default().fg(COLOR_TOOL)),
    ]);

    let header = Paragraph::new(title_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT))
                .style(Style::default().bg(COLOR_HEADER_BG)),
        )
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(header, area);
}

/// Render the main content area (chat history + optional tools panel).
fn render_main_area(frame: &mut Frame, app: &App, area: Rect) {
    if app.show_tools_panel {
        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);

        render_chat_history(frame, app, horizontal_chunks[0]);
        render_tools_panel(frame, app, horizontal_chunks[1]);
    } else {
        render_chat_history(frame, app, area);
    }
}

/// Render the scrollable chat history.
fn render_chat_history(frame: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders

    // Build all message lines
    let all_lines: Vec<Line> = app
        .messages
        .iter()
        .flat_map(|msg| {
            let prefix = match msg.role {
                Role::User => Span::styled(
                    "You │ ",
                    Style::default().fg(COLOR_USER).add_modifier(Modifier::BOLD),
                ),
                Role::Assistant => Span::styled(
                    "AI  │ ",
                    Style::default()
                        .fg(COLOR_ASSISTANT)
                        .add_modifier(Modifier::BOLD),
                ),
                Role::Tool => Span::styled(
                    "Tool│ ",
                    Style::default().fg(COLOR_TOOL).add_modifier(Modifier::BOLD),
                ),
                Role::System => Span::styled(
                    "Sys │ ",
                    Style::default()
                        .fg(COLOR_SYSTEM)
                        .add_modifier(Modifier::ITALIC),
                ),
                Role::Error => Span::styled(
                    "Err │ ",
                    Style::default()
                        .fg(COLOR_ERROR)
                        .add_modifier(Modifier::BOLD),
                ),
            };

            let content_style = match msg.role {
                Role::User => Style::default().fg(COLOR_USER),
                Role::Assistant => Style::default().fg(COLOR_ASSISTANT),
                Role::Tool => Style::default().fg(COLOR_TOOL),
                Role::System => Style::default()
                    .fg(COLOR_SYSTEM)
                    .add_modifier(Modifier::ITALIC),
                Role::Error => Style::default().fg(COLOR_ERROR),
            };

            // Split content into lines
            let content_lines: Vec<&str> = msg.content.split('\n').collect();
            let mut result_lines = Vec::new();

            for (i, line) in content_lines.iter().enumerate() {
                if i == 0 {
                    result_lines.push(Line::from(vec![
                        prefix.clone(),
                        Span::styled(line.to_string(), content_style),
                    ]));
                } else {
                    // Continuation lines get a blank prefix for alignment
                    result_lines.push(Line::from(vec![
                        Span::raw("    │ "),
                        Span::styled(line.to_string(), content_style),
                    ]));
                }
            }

            // Add a blank line separator after each message
            result_lines.push(Line::from(""));

            result_lines
        })
        .collect();

    // Apply scroll offset (from bottom)
    let total_lines = all_lines.len();
    let visible_lines = if total_lines <= inner_height {
        all_lines
    } else {
        let end = total_lines.saturating_sub(app.scroll_offset);
        let start = end.saturating_sub(inner_height);
        all_lines[start..end].to_vec()
    };

    let chat_title = format!(" Chat ({}/{}) ", total_lines, app.messages.len());

    let chat_paragraph = Paragraph::new(visible_lines)
        .block(
            Block::default()
                .title(Span::styled(
                    chat_title,
                    Style::default()
                        .fg(COLOR_ACCENT)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(chat_paragraph, area);
}

/// Render the tools activity panel on the right side.
fn render_tools_panel(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .tool_activity
        .iter()
        .rev() // Most recent first
        .take(area.height.saturating_sub(3) as usize)
        .map(|activity| {
            let status_icon = match activity.status {
                ToolStatus::Running => Span::styled("⟳ ", Style::default().fg(COLOR_TOOL)),
                ToolStatus::Success => Span::styled("✓ ", Style::default().fg(COLOR_ASSISTANT)),
                ToolStatus::Error => Span::styled("✗ ", Style::default().fg(COLOR_ERROR)),
            };

            let name_span = Span::styled(
                &activity.name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );

            let args_span = Span::styled(
                format!(" {}", activity.arguments_summary),
                Style::default().fg(COLOR_SYSTEM),
            );

            ListItem::new(Line::from(vec![status_icon, name_span, args_span]))
        })
        .collect();

    let tool_count = app.tool_activity.len();
    let panel_title = format!(" Tools ({tool_count}) ");

    let tools_list = List::new(items).block(
        Block::default()
            .title(Span::styled(
                panel_title,
                Style::default().fg(COLOR_TOOL).add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(COLOR_PANEL_BG)),
    );

    frame.render_widget(tools_list, area);
}

/// Render the input area at the bottom.
fn render_input(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = if app.is_thinking {
        format!("{} ", app.spinner_frame())
    } else {
        "⟩ ".to_string()
    };

    let prompt_style = if app.is_thinking {
        Style::default().fg(COLOR_TOOL)
    } else {
        Style::default()
            .fg(COLOR_ACCENT)
            .add_modifier(Modifier::BOLD)
    };

    let input_text = Line::from(vec![
        Span::styled(&prompt, prompt_style),
        Span::styled(&app.input, Style::default().fg(Color::White)),
    ]);

    let border_color = if app.is_thinking {
        COLOR_TOOL
    } else {
        Color::DarkGray
    };

    let input_title = app
        .completion_hint
        .as_ref()
        .map(|hint| format!(" Input - {hint} "))
        .unwrap_or_else(|| " Input ".to_string());

    let input_paragraph = Paragraph::new(input_text)
        .block(
            Block::default()
                .title(Span::styled(input_title, Style::default().fg(COLOR_SYSTEM)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(COLOR_INPUT_BG)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(input_paragraph, area);
}

/// Render the status bar at the very bottom.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let slash_token_end = app
        .input
        .find(char::is_whitespace)
        .unwrap_or(app.input.len());
    let cursor_in_slash_token =
        app.input.starts_with('/') && app.cursor_position <= slash_token_end;
    let tab_hint = if cursor_in_slash_token {
        "Tab:complete"
    } else if app.show_tools_panel {
        "Tab:hide-tools"
    } else {
        "Tab:show-tools"
    };

    let status_text = format!(
        " Session: {} │ Tokens: {} │ API calls: {} │ {} │ ↑↓:scroll │ Ctrl+C:quit",
        &app.session_id[..8.min(app.session_id.len())],
        app.total_tokens,
        app.api_calls,
        tab_hint,
    );

    let status_bar = Paragraph::new(Span::styled(status_text, Style::default().fg(COLOR_SYSTEM)))
        .style(Style::default().bg(COLOR_STATUS_BG));

    frame.render_widget(status_bar, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::{Terminal, backend::TestBackend};
    use tokio::sync::mpsc;

    fn make_app() -> App {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (_event_tx, event_rx) = mpsc::unbounded_channel();
        App::new(
            cmd_tx,
            event_rx,
            "test-model".to_string(),
            "test-session-id-1234".to_string(),
        )
    }

    #[test]
    fn render_does_not_panic_with_default_state() {
        let app = make_app();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_does_not_panic_with_no_tools_panel() {
        let mut app = make_app();
        app.show_tools_panel = false;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_does_not_panic_while_thinking() {
        let mut app = make_app();
        app.is_thinking = true;
        app.input = "typing something...".to_string();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_handles_completion_hint() {
        let mut app = make_app();
        app.input = "/hist".to_string();
        app.completion_hint =
            Some("Slash match: /history [N] - Review recent conversation messages".to_string());
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_handles_long_messages() {
        let mut app = make_app();
        let long_content = "a".repeat(1000);
        app.messages.push(crate::ChatMessage::user(&long_content));
        app.messages
            .push(crate::ChatMessage::assistant(&long_content));
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_handles_multiline_messages() {
        let mut app = make_app();
        app.messages
            .push(crate::ChatMessage::assistant("line1\nline2\nline3"));
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_handles_many_messages() {
        let mut app = make_app();
        for i in 0..100 {
            app.messages
                .push(crate::ChatMessage::user(format!("message {i}")));
        }
        app.scroll_offset = 50;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_handles_tool_activity() {
        let mut app = make_app();
        app.tool_activity.push(crate::ToolActivity {
            name: "bash".to_string(),
            arguments_summary: "ls -la".to_string(),
            status: crate::ToolStatus::Running,
            timestamp: chrono::Utc::now(),
        });
        app.tool_activity.push(crate::ToolActivity {
            name: "read_file".to_string(),
            arguments_summary: "/tmp/test.txt".to_string(),
            status: crate::ToolStatus::Success,
            timestamp: chrono::Utc::now(),
        });
        app.tool_activity.push(crate::ToolActivity {
            name: "web_search".to_string(),
            arguments_summary: "rust testing".to_string(),
            status: crate::ToolStatus::Error,
            timestamp: chrono::Utc::now(),
        });
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_handles_small_terminal() {
        let app = make_app();
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_all_role_types() {
        let mut app = make_app();
        app.messages.push(crate::ChatMessage::user("user msg"));
        app.messages
            .push(crate::ChatMessage::assistant("assistant msg"));
        app.messages
            .push(crate::ChatMessage::tool("bash", "tool output"));
        app.messages.push(crate::ChatMessage::system("system info"));
        app.messages
            .push(crate::ChatMessage::error("error occurred"));
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }
}
