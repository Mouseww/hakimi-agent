//! TUI rendering for the Hakimi Agent.

use crate::app::App;
use crate::{Role, ToolStatus};
use hakimi_common::SkinRuntime;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

/// Color scheme constants
const COLOR_ASSISTANT: Color = Color::Green;
const COLOR_TOOL: Color = Color::Yellow;
const COLOR_ERROR: Color = Color::Red;
const COLOR_SYSTEM: Color = Color::DarkGray;
const COLOR_HEADER_BG: Color = Color::Rgb(30, 30, 50);
const COLOR_STATUS_BG: Color = Color::Rgb(20, 20, 40);
const COLOR_INPUT_BG: Color = Color::Rgb(25, 25, 45);
const COLOR_PANEL_BG: Color = Color::Rgb(20, 20, 35);
const COLOR_ACCENT: Color = Color::Cyan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TuiPalette {
    accent: Color,
    label: Color,
    prompt: Color,
    ok: Color,
    error: Color,
    warn: Color,
    dim: Color,
    text: Color,
    header_bg: Color,
    panel_bg: Color,
    status_bg: Color,
    status_text: Color,
    status_strong: Color,
    status_dim: Color,
    status_good: Color,
    session_label: Color,
    session_border: Color,
    response_border: Color,
    input_border: Color,
    completion_bg: Color,
    completion_current_bg: Color,
    completion_meta_bg: Color,
    completion_meta_current_bg: Color,
    selection_bg: Color,
}

impl TuiPalette {
    fn from_app(app: &App) -> Self {
        let skin = &app.skin_runtime;
        let selection_bg = skin_color(skin.color("selection_bg"), COLOR_PANEL_BG);
        let completion_current_bg =
            skin_color(skin.color("completion_menu_current_bg"), selection_bg);
        Self {
            accent: skin_color(skin.color("ui_accent"), COLOR_ACCENT),
            label: skin_color(skin.color("ui_label"), COLOR_ASSISTANT),
            prompt: skin_color(skin.color("prompt"), COLOR_ACCENT),
            ok: skin_color(skin.color("ui_ok"), COLOR_ASSISTANT),
            error: skin_color(skin.color("ui_error"), COLOR_ERROR),
            warn: skin_color(skin.color("ui_warn"), COLOR_TOOL),
            dim: skin_color(skin.color("banner_dim"), COLOR_SYSTEM),
            text: skin_color(skin.color("banner_text"), Color::White),
            header_bg: skin_color(skin.color("status_bar_bg"), COLOR_HEADER_BG),
            panel_bg: skin_color(skin.color("completion_menu_bg"), COLOR_PANEL_BG),
            status_bg: skin_color(skin.color("status_bar_bg"), COLOR_STATUS_BG),
            status_text: skin_color(skin.color("status_bar_text"), COLOR_SYSTEM),
            status_strong: skin_color(skin.color("status_bar_strong"), COLOR_ASSISTANT),
            status_dim: skin_color(skin.color("status_bar_dim"), COLOR_SYSTEM),
            status_good: skin_color(skin.color("status_bar_good"), COLOR_ASSISTANT),
            session_label: skin_color(skin.color("session_label"), COLOR_ASSISTANT),
            session_border: skin_color(skin.color("session_border"), COLOR_SYSTEM),
            response_border: skin_color(skin.color("response_border"), Color::DarkGray),
            input_border: skin_color(skin.color("input_rule"), Color::DarkGray),
            completion_bg: skin_color(skin.color("completion_menu_bg"), COLOR_INPUT_BG),
            completion_current_bg,
            completion_meta_bg: skin_color(skin.color("completion_menu_meta_bg"), COLOR_PANEL_BG),
            completion_meta_current_bg: skin_color(
                skin.color("completion_menu_meta_current_bg"),
                completion_current_bg,
            ),
            selection_bg,
        }
    }
}

fn skin_color(value: Option<&str>, fallback: Color) -> Color {
    value.and_then(parse_hex_color).unwrap_or(fallback)
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(red, green, blue))
}

fn assistant_prefix_label(raw: &str) -> String {
    let compact = raw.split_whitespace().collect::<String>();
    let mut label = compact.chars().take(3).collect::<String>();
    if label.is_empty() {
        label.push_str("AI");
    }
    format!("{label:>3}│ ")
}

fn tool_name_from_message_content(content: &str) -> Option<&str> {
    let rest = content.strip_prefix('[')?;
    let end = rest.find(']')?;
    let name = rest[..end].trim();
    if name.is_empty() { None } else { Some(name) }
}

fn tool_prefix_label(skin: &SkinRuntime, tool_name: Option<&str>) -> String {
    let tool_prefix = skin.tool_prefix.trim();
    let tool_prefix = if tool_prefix.is_empty() {
        "│"
    } else {
        tool_prefix
    };
    match tool_name.and_then(|name| skin.tool_emoji(name)) {
        Some(emoji) => format!("{emoji} Tool{tool_prefix} "),
        None => format!("Tool{tool_prefix} "),
    }
}

/// Render the full TUI layout.
pub fn render(frame: &mut Frame, app: &App) {
    let palette = TuiPalette::from_app(app);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Main area (chat + tools panel)
            Constraint::Length(3), // Input area
            Constraint::Length(1), // Status bar
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0], &palette);
    render_main_area(frame, app, chunks[1], &palette);
    render_input(frame, app, chunks[2], &palette);
    render_status_bar(frame, app, chunks[3], &palette);
}

/// Render the header bar with title and model info.
fn render_header(frame: &mut Frame, app: &App, area: Rect, palette: &TuiPalette) {
    let thinking_indicator = if app.is_thinking {
        format!("  {}", app.thinking_label())
    } else {
        String::new()
    };
    let agent_name = app
        .skin_runtime
        .branding("agent_name")
        .unwrap_or("Hakimi Agent");

    let title_line = Line::from(vec![
        Span::styled(
            " ◆ ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            agent_name,
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {} ", app.model_name),
            Style::default().fg(palette.dim),
        ),
        Span::styled(thinking_indicator, Style::default().fg(palette.warn)),
    ]);

    let header = Paragraph::new(title_line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.accent))
                .style(Style::default().bg(palette.header_bg)),
        )
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(header, area);
}

/// Render the main content area (chat history + optional tools panel).
fn render_main_area(frame: &mut Frame, app: &App, area: Rect, palette: &TuiPalette) {
    if app.show_tools_panel {
        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);

        render_chat_history(frame, app, horizontal_chunks[0], palette);
        render_tools_panel(frame, app, horizontal_chunks[1], palette);
    } else {
        render_chat_history(frame, app, area, palette);
    }
}

/// Render the scrollable chat history.
fn render_chat_history(frame: &mut Frame, app: &App, area: Rect, palette: &TuiPalette) {
    let inner_height = area.height.saturating_sub(2) as usize; // subtract borders
    let response_label = assistant_prefix_label(
        app.skin_runtime
            .branding("response_label")
            .unwrap_or(" AI ")
            .trim(),
    );

    // Build all message lines
    let all_lines: Vec<Line> = app
        .messages
        .iter()
        .flat_map(|msg| {
            let prefix = match msg.role {
                Role::User => Span::styled(
                    "You │ ",
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Role::Assistant => Span::styled(
                    response_label.clone(),
                    Style::default()
                        .fg(palette.label)
                        .add_modifier(Modifier::BOLD),
                ),
                Role::Tool => Span::styled(
                    tool_prefix_label(
                        &app.skin_runtime,
                        tool_name_from_message_content(&msg.content),
                    ),
                    Style::default()
                        .fg(palette.warn)
                        .add_modifier(Modifier::BOLD),
                ),
                Role::System => Span::styled(
                    "Sys │ ",
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::ITALIC),
                ),
                Role::Error => Span::styled(
                    "Err │ ",
                    Style::default()
                        .fg(palette.error)
                        .add_modifier(Modifier::BOLD),
                ),
            };

            let content_style = match msg.role {
                Role::User => Style::default().fg(palette.accent),
                Role::Assistant => Style::default().fg(palette.text),
                Role::Tool => Style::default().fg(palette.warn),
                Role::System => Style::default()
                    .fg(palette.dim)
                    .add_modifier(Modifier::ITALIC),
                Role::Error => Style::default().fg(palette.error),
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
                        Span::styled("    │ ", Style::default().fg(palette.dim)),
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
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.response_border))
                .style(Style::default().bg(palette.panel_bg)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(chat_paragraph, area);
}

/// Render the tools activity panel on the right side.
fn render_tools_panel(frame: &mut Frame, app: &App, area: Rect, palette: &TuiPalette) {
    let items: Vec<ListItem> = app
        .tool_activity
        .iter()
        .rev() // Most recent first
        .take(area.height.saturating_sub(3) as usize)
        .map(|activity| {
            let status_icon = match activity.status {
                ToolStatus::Running => Span::styled(
                    format!("{} ", app.spinner_frame()),
                    Style::default().fg(palette.warn),
                ),
                ToolStatus::Success => Span::styled("✓ ", Style::default().fg(palette.ok)),
                ToolStatus::Error => Span::styled("✗ ", Style::default().fg(palette.error)),
            };
            let emoji_span = match app.skin_runtime.tool_emoji(&activity.name) {
                Some(emoji) => Span::styled(format!("{emoji} "), Style::default().fg(palette.warn)),
                None => Span::raw(""),
            };

            let name_span = Span::styled(
                &activity.name,
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            );

            let args_span = Span::styled(
                format!(" {}", activity.arguments_summary),
                Style::default().fg(palette.dim),
            );

            ListItem::new(Line::from(vec![
                status_icon,
                emoji_span,
                name_span,
                args_span,
            ]))
        })
        .collect();

    let tool_count = app.tool_activity.len();
    let panel_title = format!(" Tools ({tool_count}) ");

    let tools_list = List::new(items).block(
        Block::default()
            .title(Span::styled(
                panel_title,
                Style::default()
                    .fg(palette.warn)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette.response_border))
            .style(Style::default().bg(palette.panel_bg)),
    );

    frame.render_widget(tools_list, area);
}

/// Render the input area at the bottom.
fn render_input(frame: &mut Frame, app: &App, area: Rect, palette: &TuiPalette) {
    let prompt = if app.is_thinking {
        format!("{} ", app.spinner_frame())
    } else {
        format!(
            "{} ",
            app.skin_runtime.branding("prompt_symbol").unwrap_or("⟩")
        )
    };

    let prompt_style = if app.is_thinking {
        Style::default().fg(palette.warn)
    } else {
        Style::default()
            .fg(palette.prompt)
            .add_modifier(Modifier::BOLD)
    };

    let input_text = Line::from(vec![
        Span::styled(&prompt, prompt_style),
        Span::styled(&app.input, Style::default().fg(palette.text)),
    ]);

    let border_color = match (app.is_thinking, app.completion_hint.is_some()) {
        (true, _) => palette.warn,
        (false, true) => palette.selection_bg,
        (false, false) => palette.input_border,
    };

    let (input_title, title_style, input_bg) = if let Some(hint) = app.completion_hint.as_ref() {
        (
            format!(" Input - {hint} "),
            Style::default()
                .fg(palette.status_strong)
                .bg(palette.completion_meta_current_bg),
            palette.completion_current_bg,
        )
    } else {
        (
            " Input ".to_string(),
            Style::default()
                .fg(palette.dim)
                .bg(palette.completion_meta_bg),
            palette.completion_bg,
        )
    };

    let input_paragraph = Paragraph::new(input_text)
        .block(
            Block::default()
                .title(Span::styled(input_title, title_style))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .style(Style::default().bg(input_bg)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(input_paragraph, area);
}

/// Render the status bar at the very bottom.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect, palette: &TuiPalette) {
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

    let session_id = &app.session_id[..8.min(app.session_id.len())];
    let separator = || Span::styled(" │ ", Style::default().fg(palette.session_border));
    let status_bar = Paragraph::new(Line::from(vec![
        Span::styled(" Session: ", Style::default().fg(palette.session_label)),
        Span::styled(session_id, Style::default().fg(palette.status_strong)),
        separator(),
        Span::styled("Tokens: ", Style::default().fg(palette.status_dim)),
        Span::styled(
            app.total_tokens.to_string(),
            Style::default().fg(palette.status_good),
        ),
        separator(),
        Span::styled("API calls: ", Style::default().fg(palette.status_dim)),
        Span::styled(
            app.api_calls.to_string(),
            Style::default().fg(palette.status_text),
        ),
        separator(),
        Span::styled(tab_hint, Style::default().fg(palette.status_strong)),
        separator(),
        Span::styled(
            app.voice.status_bar_hint(),
            Style::default().fg(palette.status_text),
        ),
        separator(),
        Span::styled("↑↓:scroll", Style::default().fg(palette.status_dim)),
        separator(),
        Span::styled("Ctrl+C:quit", Style::default().fg(palette.status_dim)),
    ]))
    .style(Style::default().bg(palette.status_bg));

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
    fn hex_skin_color_parser_accepts_six_digit_rgb() {
        assert_eq!(parse_hex_color("#112233"), Some(Color::Rgb(17, 34, 51)));
        assert_eq!(parse_hex_color("#AABBCC"), Some(Color::Rgb(170, 187, 204)));
    }

    #[test]
    fn hex_skin_color_parser_rejects_non_hex_values() {
        assert_eq!(parse_hex_color("gold"), None);
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#xyzxyz"), None);
    }

    #[test]
    fn palette_uses_runtime_skin_colors() {
        let mut app = make_app();
        let mut config = hakimi_config::HakimiConfig::default();
        config.display.skin = "ares".to_string();
        app = app.with_config(&config);

        let palette = TuiPalette::from_app(&app);

        assert_eq!(palette.accent, Color::Rgb(221, 74, 58));
        assert_eq!(palette.status_bg, Color::Rgb(42, 18, 18));
        assert_eq!(palette.status_text, Color::Rgb(241, 230, 207));
        assert_eq!(palette.status_strong, Color::Rgb(199, 169, 107));
        assert_eq!(palette.status_dim, Color::Rgb(110, 88, 75));
        assert_eq!(palette.status_good, Color::Rgb(123, 201, 111));
        assert_eq!(palette.session_label, Color::Rgb(199, 169, 107));
        assert_eq!(palette.session_border, Color::Rgb(110, 88, 75));
        assert_eq!(palette.completion_meta_bg, Color::Rgb(42, 18, 18));
        assert_eq!(palette.completion_meta_current_bg, Color::Rgb(74, 26, 26));
        assert_eq!(palette.selection_bg, Color::Rgb(74, 26, 26));
        assert_eq!(palette.response_border, Color::Rgb(199, 169, 107));
    }

    #[test]
    fn assistant_prefix_label_is_fixed_width() {
        assert_eq!(assistant_prefix_label(" AI "), " AI│ ");
        assert_eq!(assistant_prefix_label(" ⚔ Ares "), "⚔Ar│ ");
        assert_eq!(assistant_prefix_label(""), " AI│ ");
    }

    #[test]
    fn tool_message_label_uses_skin_emoji_when_present() {
        let mut skin = hakimi_common::SkinRuntime::default();
        skin.tool_prefix = "::".to_string();
        skin.tool_emojis.insert("bash".to_string(), "⚔".to_string());

        assert_eq!(
            tool_name_from_message_content("[bash] call: ls"),
            Some("bash")
        );
        assert_eq!(tool_prefix_label(&skin, Some("bash")), "⚔ Tool:: ");
        assert_eq!(tool_prefix_label(&skin, Some("read_file")), "Tool:: ");
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
    fn render_handles_voice_status_bar_hint() {
        let mut app = make_app();
        app.voice.enabled = true;
        app.voice.record_key_label = "Ctrl+O".to_string();
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
    fn render_uses_skin_tool_prefix_for_tool_messages() {
        let mut app = make_app();
        app.skin_runtime.tool_prefix = "::".to_string();
        app.skin_runtime
            .tool_emojis
            .insert("bash".to_string(), "⚔".to_string());
        app.messages
            .push(crate::ChatMessage::tool("bash", "tool output"));
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn render_uses_skin_tool_emojis_for_activity_panel() {
        let mut app = make_app();
        app.skin_runtime
            .tool_emojis
            .insert("web_search".to_string(), "🔮".to_string());
        app.tool_activity.push(crate::ToolActivity {
            name: "web_search".to_string(),
            arguments_summary: "rust".to_string(),
            status: crate::ToolStatus::Running,
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
