//! Application state and event handling for the Hakimi TUI.

use crate::{
    AgentCommand, AgentEvent, ChatMessage, SPINNER_FRAMES, ToolActivity, ToolStatus,
    clipboard::{copy_assistant_response, write_clipboard_text},
};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use hakimi_common::{SlashCommandSpec, canonical_slash_command, complete_slash_command_prefix};
use hakimi_config::VoiceConfig;
use std::path::Path;
use tokio::sync::mpsc;

const TOOL_CHAT_PREVIEW_CHARS: usize = 120;
const TOOL_PANEL_PREVIEW_CHARS: usize = 80;
const HISTORY_PREVIEW_CHARS: usize = 160;
const COMPLETION_HINT_LIMIT: usize = 5;
const COMPLETION_HINT_CHARS: usize = 96;
const VOICE_MAX_CONSECUTIVE_NO_SPEECH: u8 = 3;

fn compact_one_line(input: &str, max_chars: usize) -> String {
    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

fn parse_history_limit(arg: Option<&str>) -> Result<Option<usize>, &'static str> {
    let raw = arg.unwrap_or_default().trim();
    if raw.is_empty() {
        return Ok(None);
    }

    match raw.parse::<usize>() {
        Ok(limit) if limit > 0 => Ok(Some(limit)),
        _ => Err("usage: /history [number]"),
    }
}

fn parse_undo_turns(arg: Option<&str>) -> Result<usize, &'static str> {
    let raw = arg.unwrap_or_default().trim();
    if raw.is_empty() {
        return Ok(1);
    }

    match raw.parse::<usize>() {
        Ok(turns) if turns > 0 => Ok(turns),
        _ => Err("usage: /undo [turns]"),
    }
}

fn render_tui_checkpoint_command(arg: Option<&str>, workdir: &Path) -> String {
    hakimi_tools::checkpoint_response(arg, workdir)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UndoResult {
    turns_undone: usize,
    removed_messages: usize,
    target_text: String,
}

fn undo_recent_user_turns(
    messages: &mut Vec<ChatMessage>,
    turns: usize,
) -> Result<UndoResult, &'static str> {
    let mut seen_user_turns = 0usize;
    let mut target_index = None;

    for (index, message) in messages.iter().enumerate().rev() {
        if message.role != crate::Role::User {
            continue;
        }
        seen_user_turns += 1;
        target_index = Some(index);
        if seen_user_turns == turns {
            break;
        }
    }

    let Some(target_index) = target_index else {
        return Err("nothing to undo");
    };

    let target_text = messages[target_index].content.clone();
    let removed_messages = messages.len().saturating_sub(target_index);
    messages.truncate(target_index);

    Ok(UndoResult {
        turns_undone: seen_user_turns,
        removed_messages,
        target_text,
    })
}

fn render_history_messages(
    messages: &[ChatMessage],
    arg: Option<&str>,
) -> Result<String, &'static str> {
    let limit = parse_history_limit(arg)?;
    let visible: Vec<(usize, &ChatMessage)> = messages
        .iter()
        .filter(|message| {
            message.role == crate::Role::User || message.role == crate::Role::Assistant
        })
        .enumerate()
        .map(|(index, message)| (index + 1, message))
        .collect();

    if visible.is_empty() {
        return Err("nothing in conversation history yet");
    }

    let start = limit
        .map(|limit| visible.len().saturating_sub(limit))
        .unwrap_or(0);
    let shown = visible.len() - start;
    let mut lines = vec![format!(
        "Conversation history (showing {shown} of {} messages):",
        visible.len()
    )];

    for (index, message) in visible.into_iter().skip(start) {
        let label = if message.role == crate::Role::User {
            "You"
        } else {
            "Hakimi"
        };
        let preview = compact_one_line(&message.content, HISTORY_PREVIEW_CHARS);
        let preview = if preview.is_empty() {
            "(empty)".to_string()
        } else {
            preview
        };
        lines.push(format!("  [{label} #{index}] {preview}"));
    }

    Ok(lines.join("\n"))
}

fn render_completion_hint(matches: &[&SlashCommandSpec]) -> Option<String> {
    let first = matches.first()?;
    let hint = if matches.len() == 1 {
        let args = if first.args_hint.is_empty() {
            String::new()
        } else {
            format!(" {}", first.args_hint)
        };
        format!("Slash match: /{}{} - {}", first.name, args, first.summary)
    } else {
        let mut names: Vec<String> = matches
            .iter()
            .take(COMPLETION_HINT_LIMIT)
            .map(|spec| format!("/{}", spec.name))
            .collect();
        if matches.len() > COMPLETION_HINT_LIMIT {
            names.push(format!("+{} more", matches.len() - COMPLETION_HINT_LIMIT));
        }
        format!("Slash matches: {}", names.join(", "))
    };
    Some(compact_one_line(&hint, COMPLETION_HINT_CHARS))
}

fn env_any_present(names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn parse_ctrl_record_key(raw: &str) -> Option<char> {
    let normalized = raw.trim().to_ascii_lowercase().replace(' ', "");
    let suffix = normalized
        .strip_prefix("ctrl+")
        .or_else(|| normalized.strip_prefix("control+"))?;
    let mut chars = suffix.chars();
    let ch = chars.next()?;
    if chars.next().is_none() && ch.is_ascii_alphabetic() {
        Some(ch)
    } else {
        None
    }
}

fn format_voice_record_key(raw: &str) -> String {
    parse_ctrl_record_key(raw)
        .map(|ch| format!("Ctrl+{}", ch.to_ascii_uppercase()))
        .unwrap_or_else(|| "Ctrl+B".to_string())
}

fn voice_record_key_matches(key: &KeyEvent, raw: &str) -> bool {
    let expected = parse_ctrl_record_key(raw).unwrap_or('b');
    let KeyCode::Char(actual) = &key.code else {
        return false;
    };
    key.modifiers.contains(KeyModifiers::CONTROL) && actual.eq_ignore_ascii_case(&expected)
}

#[derive(Debug, Clone, PartialEq)]
pub struct TuiVoiceStatus {
    pub enabled: bool,
    pub tts: bool,
    pub continuous: bool,
    pub recording: bool,
    pub processing: bool,
    pub restart_pending: bool,
    pub consecutive_no_speech: u8,
    pub record_key: String,
    pub record_key_label: String,
    pub provider: String,
    pub model: String,
    pub voice: String,
    pub transcription_model: String,
    pub silence_threshold: u32,
    pub silence_duration_seconds: f32,
    pub beep_enabled: bool,
    pub auto_play: bool,
    pub tts_ready: bool,
    pub transcription_ready: bool,
    pub ffmpeg_available: bool,
    pub audio_environment: hakimi_tools::VoiceEnvironmentReport,
}

impl Default for TuiVoiceStatus {
    fn default() -> Self {
        Self::from_config_with_ffmpeg(&VoiceConfig::default(), false)
    }
}

impl TuiVoiceStatus {
    pub fn from_config(config: &VoiceConfig) -> Self {
        Self::from_config_with_ffmpeg(config, ffmpeg_available())
    }

    fn from_config_with_ffmpeg(config: &VoiceConfig, ffmpeg_available: bool) -> Self {
        let provider = if config.provider.trim().is_empty() {
            "openai".to_string()
        } else {
            config.provider.trim().to_string()
        };
        let tts_api_configured = !config.api_key.trim().is_empty()
            || env_any_present(&[
                "HAKIMI_TTS_API_KEY",
                "VOICE_TOOLS_OPENAI_KEY",
                "OPENAI_API_KEY",
            ]);
        let transcription_api_configured = !config.api_key.trim().is_empty()
            || env_any_present(&[
                "HAKIMI_TRANSCRIPTION_API_KEY",
                "VOICE_TOOLS_OPENAI_KEY",
                "OPENAI_API_KEY",
            ]);
        let record_key = if config.record_key.trim().is_empty() {
            "ctrl+b".to_string()
        } else {
            config.record_key.trim().to_string()
        };

        Self {
            enabled: false,
            tts: false,
            continuous: false,
            recording: false,
            processing: false,
            restart_pending: false,
            consecutive_no_speech: 0,
            record_key_label: format_voice_record_key(&record_key),
            record_key,
            provider: provider.clone(),
            model: if config.model.trim().is_empty() {
                "tts-1".to_string()
            } else {
                config.model.trim().to_string()
            },
            voice: if config.voice.trim().is_empty() {
                "alloy".to_string()
            } else {
                config.voice.trim().to_string()
            },
            transcription_model: if config.transcription_model.trim().is_empty() {
                "whisper-1".to_string()
            } else {
                config.transcription_model.trim().to_string()
            },
            silence_threshold: config.silence_threshold,
            silence_duration_seconds: config.silence_duration_seconds,
            beep_enabled: config.beep_enabled,
            auto_play: config.auto_play,
            tts_ready: provider.eq_ignore_ascii_case("edge") || tts_api_configured,
            transcription_ready: transcription_api_configured,
            ffmpeg_available,
            audio_environment: hakimi_tools::detect_voice_environment(),
        }
    }

    pub(crate) fn status_bar_hint(&self) -> String {
        let state = if self.recording {
            "rec"
        } else if self.processing {
            "stt"
        } else if self.continuous {
            "loop"
        } else if self.enabled {
            "on"
        } else {
            "off"
        };
        format!("Voice:{state} {}", self.record_key_label)
    }

    fn render_status(&self) -> String {
        let mode = if self.enabled { "on" } else { "off" };
        let continuous = if self.continuous { "on" } else { "off" };
        let tts = if self.tts { "on" } else { "off" };
        let tts_status = if self.tts_ready {
            "ready"
        } else {
            "needs API key"
        };
        let stt_status = if self.transcription_ready {
            "ready"
        } else {
            "needs API key"
        };
        let ffmpeg = if self.ffmpeg_available {
            "available"
        } else {
            "not found"
        };
        let beep = if self.beep_enabled { "on" } else { "off" };
        let auto_play = if self.auto_play { "on" } else { "off" };

        let audio_environment = self.audio_environment.render();

        format!(
            "Voice mode: {mode}\n\
             Record key: {record_key}\n\
             TTS guidance: {tts}; tool {tts_status} (provider={provider}, model={model}, voice={voice})\n\
             STT tool: {stt_status} (model={transcription_model})\n\
             ffmpeg: {ffmpeg}; auto_play={auto_play}; beep={beep}; continuous={continuous}\n\
             Capture settings: threshold={threshold}, silence={silence:.1}s\n\
             {cue_status}\n\
             TTS playback: Markdown cleanup and MP3 cache planning ready (max {tts_max_chars} chars)\n\
             Recording artifact: PCM16 WAV writer ready ({sample_rate} Hz mono, min speech {min_speech:.1}s, no-speech timeout {no_speech:.0}s)\n\
             {audio_environment}\n\
             TUI continuous capture is ready through voice_capture; {record_key} records, transcribes, submits the transcript, and restarts listening until {record_key} is pressed again or 3 recordings contain no speech.",
            record_key = self.record_key_label,
            continuous = continuous,
            provider = self.provider,
            model = self.model,
            voice = self.voice,
            transcription_model = self.transcription_model,
            threshold = self.silence_threshold,
            silence = self.silence_duration_seconds,
            cue_status = hakimi_tools::render_voice_cue_status(self.beep_enabled),
            tts_max_chars = hakimi_tools::VOICE_TTS_MAX_CHARS,
            sample_rate = hakimi_tools::VOICE_SAMPLE_RATE,
            min_speech = hakimi_tools::MIN_SPEECH_RECORDING_SECONDS,
            no_speech = hakimi_tools::NO_SPEECH_TIMEOUT_SECONDS,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TuiCommand {
    Help,
    History(Option<String>),
    Undo(Option<String>),
    Copy(Option<String>),
    Checkpoints(Option<String>),
    Clear,
    Tools,
    Voice(Option<String>),
    Quit,
}

fn parse_tui_command(input: &str) -> Option<TuiCommand> {
    let rest = input.trim().strip_prefix('/')?;
    let (cmd, arg) = match rest.split_once(char::is_whitespace) {
        Some((cmd, arg)) => (cmd, Some(arg.trim().to_string())),
        None => (rest, None),
    };

    match canonical_slash_command(cmd)? {
        "help" => Some(TuiCommand::Help),
        "history" => Some(TuiCommand::History(arg)),
        "undo" => Some(TuiCommand::Undo(arg)),
        "copy" => Some(TuiCommand::Copy(arg)),
        "checkpoints" => Some(TuiCommand::Checkpoints(arg)),
        "clear" => Some(TuiCommand::Clear),
        "tools" => Some(TuiCommand::Tools),
        "voice" => Some(TuiCommand::Voice(arg)),
        "quit" => Some(TuiCommand::Quit),
        _ => None,
    }
}

/// The main application state.
pub struct App {
    /// Chat messages displayed in the main panel.
    pub messages: Vec<ChatMessage>,
    /// Current input text.
    pub input: String,
    /// Cursor position within the input.
    pub cursor_position: usize,
    /// Contextual hint for slash command completion.
    pub completion_hint: Option<String>,
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
    /// Local voice-mode readiness and command state.
    pub voice: TuiVoiceStatus,
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
            completion_hint: None,
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
            voice: TuiVoiceStatus::default(),
        }
    }

    pub fn with_voice_config(mut self, config: &VoiceConfig) -> Self {
        self.voice = TuiVoiceStatus::from_config(config);
        self
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

        if voice_record_key_matches(&key, &self.voice.record_key) {
            self.handle_voice_record_key();
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
                    let clear_input = self.handle_slash_command(&text);
                    if clear_input {
                        self.input.clear();
                        self.cursor_position = 0;
                    }
                    self.completion_hint = None;
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
                self.completion_hint = None;
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
                if self.apply_slash_completion() {
                    return;
                }
                self.show_tools_panel = !self.show_tools_panel;
                self.refresh_completion_hint();
            }

            // Backspace
            KeyCode::Backspace if self.cursor_position > 0 => {
                let before = &self.input[..self.cursor_position - 1];
                let after = &self.input[self.cursor_position..];
                self.input = format!("{before}{after}");
                self.cursor_position -= 1;
                self.refresh_completion_hint();
            }

            // Delete
            KeyCode::Delete if self.cursor_position < self.input.len() => {
                let before = &self.input[..self.cursor_position];
                let after = &self.input[self.cursor_position + 1..];
                self.input = format!("{before}{after}");
                self.refresh_completion_hint();
            }

            // Home
            KeyCode::Home => {
                self.cursor_position = 0;
                self.refresh_completion_hint();
            }

            // End
            KeyCode::End => {
                self.cursor_position = self.input.len();
                self.refresh_completion_hint();
            }

            // Left arrow
            KeyCode::Left if self.cursor_position > 0 => {
                self.cursor_position -= 1;
                self.refresh_completion_hint();
            }

            // Right arrow
            KeyCode::Right if self.cursor_position < self.input.len() => {
                self.cursor_position += 1;
                self.refresh_completion_hint();
            }

            // Regular character input
            KeyCode::Char(c) => {
                let before = &self.input[..self.cursor_position];
                let after = &self.input[self.cursor_position..];
                self.input = format!("{before}{c}{after}");
                self.cursor_position += 1;
                self.refresh_completion_hint();
            }

            // Escape — could be used for interrupt in future
            KeyCode::Esc => {
                // Currently no-op; could interrupt agent
            }

            _ => {}
        }
    }

    fn current_slash_token(&self) -> Option<&str> {
        if !self.input.starts_with('/') {
            return None;
        }
        let token_end = self
            .input
            .find(char::is_whitespace)
            .unwrap_or(self.input.len());
        if self.cursor_position > token_end {
            return None;
        }
        Some(&self.input[..token_end])
    }

    fn refresh_completion_hint(&mut self) {
        let Some(token) = self.current_slash_token() else {
            self.completion_hint = None;
            return;
        };
        let completion = complete_slash_command_prefix(token);
        self.completion_hint = render_completion_hint(&completion.matches);
    }

    fn apply_slash_completion(&mut self) -> bool {
        let Some(token) = self.current_slash_token() else {
            self.completion_hint = None;
            return false;
        };
        let completion = complete_slash_command_prefix(token);
        if let Some(replacement) = completion.replacement {
            let rest = self.input[token.len()..].to_string();
            self.input = format!("{replacement}{rest}");
            self.cursor_position = replacement.len();
            self.completion_hint = render_completion_hint(&completion.matches);
            return true;
        }
        if !completion.matches.is_empty() {
            self.completion_hint = render_completion_hint(&completion.matches);
            return true;
        }
        self.completion_hint = Some(format!("No slash command matches `{token}`"));
        true
    }

    /// Handle slash commands locally (without sending to agent).
    fn handle_slash_command(&mut self, cmd: &str) -> bool {
        match parse_tui_command(cmd) {
            Some(TuiCommand::Help) => {
                self.messages.push(ChatMessage::system(
                    "Commands:\n  /help               — Show this help\n  /history [N]        — Show recent conversation messages\n  /undo [N]           — Rewind recent user turns into the composer\n  /copy [N]           — Copy the Nth latest assistant response\n  /checkpoints [cmd]  — Inspect or manage file checkpoints\n  /clear              — Clear chat history\n  /tools              — Toggle tools panel\n  /voice [cmd]        — Show or toggle voice readiness\n  /quit               — Exit the application\n\nTab completes slash commands before the first space.",
                ));
            }
            Some(TuiCommand::History(arg)) => {
                match render_history_messages(&self.messages, arg.as_deref()) {
                    Ok(history) => self.messages.push(ChatMessage::system(history)),
                    Err(message) => self.messages.push(ChatMessage::error(message)),
                }
            }
            Some(TuiCommand::Undo(arg)) => {
                let turns = match parse_undo_turns(arg.as_deref()) {
                    Ok(turns) => turns,
                    Err(message) => {
                        self.messages.push(ChatMessage::error(message));
                        return true;
                    }
                };
                match undo_recent_user_turns(&mut self.messages, turns) {
                    Ok(result) => {
                        self.input = result.target_text;
                        self.cursor_position = self.input.len();
                        let plural = if result.turns_undone == 1 {
                            "turn"
                        } else {
                            "turns"
                        };
                        self.messages.push(ChatMessage::system(format!(
                            "Undid {} {plural} ({} messages). Edit and press Enter to resend.",
                            result.turns_undone, result.removed_messages
                        )));
                        self.scroll_offset = 0;
                        return false;
                    }
                    Err(message) => self.messages.push(ChatMessage::error(message)),
                }
            }
            Some(TuiCommand::Copy(arg)) => {
                let response =
                    copy_assistant_response(&self.messages, arg.as_deref(), write_clipboard_text);
                match response {
                    crate::clipboard::CopyAssistantResponse::Copied { chars } => self
                        .messages
                        .push(ChatMessage::system(format!("copied {chars} characters"))),
                    other if other.is_error() => {
                        self.messages.push(ChatMessage::error(other.message()))
                    }
                    other => self.messages.push(ChatMessage::system(other.message())),
                }
            }
            Some(TuiCommand::Checkpoints(arg)) => {
                let output = match std::env::current_dir() {
                    Ok(workdir) => render_tui_checkpoint_command(arg.as_deref(), &workdir),
                    Err(err) => format!("Checkpoint command failed: {err}"),
                };
                self.messages.push(ChatMessage::system(output));
            }
            Some(TuiCommand::Clear) => {
                self.messages.clear();
                self.messages
                    .push(ChatMessage::system("Chat history cleared."));
                self.scroll_offset = 0;
            }
            Some(TuiCommand::Tools) => {
                self.show_tools_panel = !self.show_tools_panel;
                let state = if self.show_tools_panel { "on" } else { "off" };
                self.messages
                    .push(ChatMessage::system(format!("Tools panel: {state}")));
            }
            Some(TuiCommand::Voice(arg)) => {
                self.handle_voice_command(arg.as_deref());
            }
            Some(TuiCommand::Quit) => {
                let _ = self.cmd_tx.send(AgentCommand::Shutdown);
                self.should_quit = true;
            }
            _ => {
                self.messages.push(ChatMessage::error(format!(
                    "Unknown command: {cmd}. Type /help for available commands."
                )));
            }
        }
        true
    }

    fn handle_voice_command(&mut self, arg: Option<&str>) {
        match arg.unwrap_or("status").trim().to_ascii_lowercase().as_str() {
            "" | "status" | "doctor" => {
                self.messages
                    .push(ChatMessage::system(self.voice.render_status()));
            }
            "on" | "enable" => {
                self.voice.enabled = true;
                self.voice.continuous = false;
                self.voice.restart_pending = false;
                self.voice.consecutive_no_speech = 0;
                self.messages.push(ChatMessage::system(format!(
                    "Voice mode enabled. Press {} to start continuous recording.",
                    self.voice.record_key_label
                )));
            }
            "off" | "disable" => {
                self.voice.enabled = false;
                self.voice.tts = false;
                self.voice.continuous = false;
                self.voice.recording = false;
                self.voice.processing = false;
                self.voice.restart_pending = false;
                self.voice.consecutive_no_speech = 0;
                self.messages
                    .push(ChatMessage::system("Voice mode disabled."));
            }
            "tts" => {
                self.voice.enabled = true;
                self.voice.tts = !self.voice.tts;
                let state = if self.voice.tts {
                    "enabled"
                } else {
                    "disabled"
                };
                self.messages.push(ChatMessage::system(format!(
                    "TTS guidance {state}. Use text_to_speech for explicit audio output."
                )));
            }
            _ => {
                self.messages.push(ChatMessage::error(
                    "usage: /voice [on|off|tts|status|doctor]",
                ));
            }
        }
    }

    fn handle_voice_record_key(&mut self) {
        if !self.voice.enabled {
            self.messages.push(ChatMessage::system(format!(
                "Voice mode is off. Use /voice on before using {}.",
                self.voice.record_key_label
            )));
            return;
        }

        if self.voice.recording {
            let _ = self.cmd_tx.send(AgentCommand::CancelVoiceCapture);
            self.voice.continuous = false;
            self.voice.recording = false;
            self.voice.processing = false;
            self.voice.restart_pending = false;
            self.voice.consecutive_no_speech = 0;
            self.is_thinking = true;
            self.messages.push(ChatMessage::system(format!(
                "Stopping continuous voice capture. Press {} again after Hakimi returns to ready.",
                self.voice.record_key_label
            )));
            return;
        }

        if self.voice.processing || self.is_thinking {
            self.messages.push(ChatMessage::system(format!(
                "Voice capture is already active. Wait for recording or transcription to finish before pressing {} again.",
                self.voice.record_key_label
            )));
            return;
        }

        if !self.voice.audio_environment.capture_available {
            self.messages.push(ChatMessage::error(format!(
                "Voice capture is not ready: {}",
                self.voice.audio_environment.capture_backend
            )));
            return;
        }

        if !self.voice.transcription_ready {
            self.messages.push(ChatMessage::error(
                "Voice transcription is not configured. Set voice.api_key, HAKIMI_TRANSCRIPTION_API_KEY, VOICE_TOOLS_OPENAI_KEY, or OPENAI_API_KEY.",
            ));
            return;
        }

        self.voice.continuous = true;
        self.voice.consecutive_no_speech = 0;
        self.start_voice_capture(false);
    }

    fn start_voice_capture(&mut self, restarted: bool) {
        let command = AgentCommand::VoiceCapture {
            duration_seconds: hakimi_tools::NO_SPEECH_TIMEOUT_SECONDS,
            silence_threshold: self.voice.silence_threshold,
        };

        if self.cmd_tx.send(command).is_err() {
            self.voice.recording = false;
            self.voice.processing = false;
            self.voice.restart_pending = false;
            self.voice.continuous = false;
            self.is_thinking = false;
            self.messages
                .push(ChatMessage::error("Failed to start voice capture."));
            return;
        }

        self.voice.recording = true;
        self.voice.processing = false;
        self.voice.restart_pending = false;
        self.is_thinking = true;
        self.scroll_offset = 0;
        self.play_voice_cue(hakimi_tools::VoiceCueKind::Start);

        let message = if restarted {
            format!(
                "Voice continuous mode is listening again with {}.",
                self.voice.record_key_label
            )
        } else {
            format!(
                "Recording with {}. Hakimi will transcribe, respond, and keep listening until you press it again.",
                self.voice.record_key_label
            )
        };
        self.messages.push(ChatMessage::system(message));
    }

    /// Process incoming agent events (non-blocking).
    pub fn poll_agent_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AgentEvent::Thinking => {
                    self.is_thinking = true;
                }

                AgentEvent::ToolCall { name, arguments } => {
                    // Show a compact one-line summary in chat. Full arguments remain in the model history/logs.
                    let args_preview = compact_one_line(&arguments, TOOL_CHAT_PREVIEW_CHARS);
                    self.messages
                        .push(ChatMessage::tool(&name, format!("call: {args_preview}")));

                    // Show in tool activity panel
                    self.tool_activity.push(ToolActivity {
                        name: name.clone(),
                        arguments_summary: compact_one_line(&arguments, TOOL_PANEL_PREVIEW_CHARS),
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
                    if name == "voice_capture" {
                        self.voice.recording = false;
                        self.voice.processing = !is_error;
                    }

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

                    // Show a compact one-line result summary in chat.
                    let preview = compact_one_line(&content, TOOL_CHAT_PREVIEW_CHARS);
                    if is_error {
                        self.messages
                            .push(ChatMessage::error(format!("[{name}] {preview}")));
                    } else {
                        self.messages
                            .push(ChatMessage::tool(&name, format!("result: {preview}")));
                    }

                    self.scroll_offset = 0;
                }

                AgentEvent::Response(text) => {
                    self.voice.recording = false;
                    self.voice.processing = false;
                    self.voice.restart_pending = self.voice.continuous;
                    self.is_thinking = false;
                    if !text.is_empty() {
                        self.messages.push(ChatMessage::assistant(&text));
                    }
                    self.scroll_offset = 0;
                    self.api_calls += 1;
                }

                AgentEvent::Error(err) => {
                    self.voice.recording = false;
                    self.voice.processing = false;
                    self.voice.continuous = false;
                    self.voice.restart_pending = false;
                    self.voice.consecutive_no_speech = 0;
                    self.is_thinking = false;
                    self.messages.push(ChatMessage::error(&err));
                    self.scroll_offset = 0;
                }

                AgentEvent::VoiceTranscript {
                    transcript,
                    audio_path,
                } => {
                    self.voice.recording = false;
                    self.voice.processing = true;
                    self.voice.consecutive_no_speech = 0;
                    self.play_voice_cue(hakimi_tools::VoiceCueKind::Stop);
                    if let Some(path) = audio_path.filter(|path| !path.trim().is_empty()) {
                        self.messages
                            .push(ChatMessage::system(format!("Voice transcript from {path}")));
                    }
                    self.messages.push(ChatMessage::user(transcript));
                    self.scroll_offset = 0;
                }

                AgentEvent::VoiceNoSpeech { reason, audio_path } => {
                    self.voice.recording = false;
                    self.voice.processing = false;
                    self.voice.consecutive_no_speech = if self.voice.continuous {
                        self.voice.consecutive_no_speech.saturating_add(1)
                    } else {
                        0
                    };
                    let no_speech_count = self.voice.consecutive_no_speech;
                    let should_stop =
                        self.voice.continuous && no_speech_count >= VOICE_MAX_CONSECUTIVE_NO_SPEECH;
                    self.voice.restart_pending = self.voice.continuous && !should_stop;
                    if should_stop {
                        self.voice.continuous = false;
                        self.voice.restart_pending = false;
                    }
                    self.is_thinking = self.voice.restart_pending;
                    self.play_voice_cue(hakimi_tools::VoiceCueKind::Stop);
                    let suffix = audio_path
                        .filter(|path| !path.trim().is_empty())
                        .map(|path| format!(" Recording preserved at {path}."))
                        .unwrap_or_default();
                    let loop_status = if should_stop {
                        " Continuous voice mode stopped after 3 recordings without speech."
                    } else if self.voice.restart_pending {
                        " Listening will restart automatically."
                    } else {
                        ""
                    };
                    self.messages.push(ChatMessage::system(format!(
                        "{reason}{suffix}{loop_status}"
                    )));
                    self.scroll_offset = 0;
                }

                AgentEvent::VoiceCaptureCancelled => {
                    self.voice.recording = false;
                    self.voice.processing = false;
                    self.voice.continuous = false;
                    self.voice.restart_pending = false;
                    self.voice.consecutive_no_speech = 0;
                    self.is_thinking = false;
                    self.play_voice_cue(hakimi_tools::VoiceCueKind::Stop);
                    if let Some(activity) = self
                        .tool_activity
                        .iter_mut()
                        .rev()
                        .find(|a| a.name == "voice_capture" && a.status == ToolStatus::Running)
                    {
                        activity.status = ToolStatus::Error;
                    }
                    self.messages
                        .push(ChatMessage::system("Voice capture stopped."));
                    self.scroll_offset = 0;
                }

                AgentEvent::Done => {
                    if self.voice.restart_pending && self.voice.enabled && self.voice.continuous {
                        self.start_voice_capture(true);
                    } else {
                        self.voice.recording = false;
                        self.voice.processing = false;
                        self.voice.restart_pending = false;
                        self.is_thinking = false;
                    }
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

    fn play_voice_cue(&self, kind: hakimi_tools::VoiceCueKind) {
        if !self.voice.beep_enabled {
            return;
        }

        #[cfg(not(test))]
        {
            let _ = hakimi_tools::start_voice_cue(kind);
        }

        #[cfg(test)]
        {
            let _ = kind;
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
    fn compact_one_line_collapses_whitespace_and_truncates() {
        let text = "first line\nsecond    line\tthird line and a long tail";
        let compact = compact_one_line(text, 24);
        assert_eq!(compact, "first line second line t…");
        assert!(!compact.contains('\n'));
        assert!(!compact.contains('\t'));
    }

    #[test]
    fn poll_tool_messages_are_single_line_and_short() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());

        event_tx
            .send(crate::AgentEvent::ToolCall {
                name: "bash".to_string(),
                arguments: "{\n  \"command\": \"printf 'hello\\nworld' && echo done\"\n}".repeat(8),
            })
            .unwrap();
        event_tx
            .send(crate::AgentEvent::ToolResult {
                name: "bash".to_string(),
                content: "line1\nline2\nline3 ".repeat(20),
                is_error: false,
            })
            .unwrap();

        app.poll_agent_events();
        let tool_messages: Vec<_> = app
            .messages
            .iter()
            .filter(|m| m.role == crate::Role::Tool)
            .collect();
        assert_eq!(tool_messages.len(), 2);
        for msg in tool_messages {
            assert!(!msg.content.contains('\n'));
            assert!(msg.content.chars().count() <= TOOL_CHAT_PREVIEW_CHARS + 32);
        }
    }

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
        assert!(app.completion_hint.is_none());
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
        assert!(!app.voice.enabled);
        assert_eq!(app.voice.record_key_label, "Ctrl+B");
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

    #[test]
    fn tab_completes_unique_slash_command_prefix() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/hist".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }

        app.handle_key_event(key(KeyCode::Tab));

        assert_eq!(app.input, "/history ");
        assert_eq!(app.cursor_position, "/history ".len());
        assert!(app.show_tools_panel);
        assert!(
            app.completion_hint
                .as_deref()
                .unwrap_or_default()
                .contains("/history")
        );
    }

    #[test]
    fn tab_on_ambiguous_slash_prefix_shows_candidates_without_toggling_panel() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/c".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }

        app.handle_key_event(key(KeyCode::Tab));

        assert_eq!(app.input, "/c");
        assert_eq!(app.cursor_position, 2);
        assert!(app.show_tools_panel);
        let hint = app.completion_hint.as_deref().unwrap_or_default();
        assert!(hint.contains("/clear"));
        assert!(hint.contains("/config"));
    }

    #[test]
    fn tab_keeps_tools_toggle_for_regular_input() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "hello".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }

        app.handle_key_event(key(KeyCode::Tab));

        assert_eq!(app.input, "hello");
        assert!(!app.show_tools_panel);
        assert!(app.completion_hint.is_none());
    }

    #[test]
    fn slash_completion_hint_clears_after_first_argument() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/history 2".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }

        assert!(app.completion_hint.is_none());
    }

    #[test]
    fn slash_alias_commands_still_execute_locally() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("question"));
        app.messages.push(crate::ChatMessage::assistant("answer"));
        for c in "/hist 1".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        let content = &app.messages.last().unwrap().content;
        assert!(content.contains("showing 1 of 2 messages"));
        assert!(content.contains("[Hakimi #2] answer"));
    }

    #[test]
    fn parse_tui_command_accepts_checkpoint_alias() {
        assert_eq!(
            parse_tui_command("/ckpt status"),
            Some(TuiCommand::Checkpoints(Some("status".to_string())))
        );
    }

    #[test]
    fn parse_tui_command_keeps_checkpoint_arguments() {
        assert_eq!(
            parse_tui_command("/checkpoints diff deadbeef crates/hakimi-tui/src/app.rs"),
            Some(TuiCommand::Checkpoints(Some(
                "diff deadbeef crates/hakimi-tui/src/app.rs".to_string()
            )))
        );
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
        assert!(app.messages[1].content.contains("/history"));
        assert!(app.messages[1].content.contains("/undo"));
        assert!(app.messages[1].content.contains("/copy"));
        assert!(app.messages[1].content.contains("/checkpoints"));
        assert!(app.messages[1].content.contains("/voice"));
    }

    #[test]
    fn slash_history_without_conversation_shows_error() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/history".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("nothing in conversation history")
        );
    }

    #[test]
    fn slash_history_renders_latest_user_and_assistant_messages() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages
            .push(crate::ChatMessage::user("first question"));
        app.messages
            .push(crate::ChatMessage::assistant("first answer"));
        app.messages
            .push(crate::ChatMessage::tool("bash", "hidden output"));
        app.messages
            .push(crate::ChatMessage::user("second question"));
        for c in "/history 2".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        let content = &app.messages.last().unwrap().content;
        assert_eq!(app.messages.last().unwrap().role, crate::Role::System);
        assert!(content.contains("showing 2 of 3 messages"));
        assert!(content.contains("[Hakimi #2] first answer"));
        assert!(content.contains("[You #3] second question"));
        assert!(!content.contains("first question"));
        assert!(!content.contains("hidden output"));
    }

    #[test]
    fn slash_history_alias_rejects_non_numeric_argument() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("question"));
        for c in "/hist nope".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("usage: /history")
        );
    }

    #[test]
    fn slash_undo_prefills_latest_user_turn() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages
            .push(crate::ChatMessage::user("first question"));
        app.messages
            .push(crate::ChatMessage::assistant("first answer"));
        app.messages
            .push(crate::ChatMessage::user("second question"));
        app.messages
            .push(crate::ChatMessage::assistant("second answer"));

        for c in "/undo".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.input, "second question");
        assert_eq!(app.cursor_position, "second question".len());
        assert_eq!(app.messages.len(), 4);
        assert_eq!(app.messages[3].role, crate::Role::System);
        assert!(app.messages[3].content.contains("Undid 1 turn"));
        assert!(app.messages[3].content.contains("2 messages"));
        assert!(!app.is_thinking);
    }

    #[test]
    fn slash_undo_n_turns_rewinds_to_requested_user_message() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("q1"));
        app.messages.push(crate::ChatMessage::assistant("a1"));
        app.messages.push(crate::ChatMessage::user("q2"));
        app.messages.push(crate::ChatMessage::assistant("a2"));
        app.messages.push(crate::ChatMessage::user("q3"));
        app.messages.push(crate::ChatMessage::assistant("a3"));

        for c in "/rewind 2".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.input, "q2");
        assert_eq!(app.messages.len(), 4);
        assert_eq!(app.messages[1].content, "q1");
        assert_eq!(app.messages[2].content, "a1");
        assert!(app.messages[3].content.contains("Undid 2 turns"));
        assert!(app.messages[3].content.contains("4 messages"));
    }

    #[test]
    fn slash_undo_clamps_to_oldest_turn() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("only question"));
        app.messages
            .push(crate::ChatMessage::assistant("only answer"));

        for c in "/undo 99".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.input, "only question");
        assert_eq!(app.messages.len(), 2);
        assert!(app.messages[1].content.contains("Undid 1 turn"));
    }

    #[test]
    fn slash_undo_rejects_invalid_count() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::user("question"));
        for c in "/undo nope".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(app.input.is_empty());
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("usage: /undo")
        );
    }

    #[test]
    fn slash_undo_without_user_turn_shows_error() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/undo".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("nothing to undo")
        );
    }

    #[test]
    fn slash_copy_without_assistant_message_shows_error() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/copy".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("nothing to copy")
        );
    }

    #[test]
    fn slash_copy_alias_rejects_non_numeric_argument() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.messages.push(crate::ChatMessage::assistant("answer"));
        for c in "/cp nope".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert_eq!(app.messages.last().unwrap().role, crate::Role::Error);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("usage: /copy")
        );
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
    fn slash_voice_status_reports_readiness_without_model_call() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/voice status".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        let message = app.messages.last().unwrap();
        assert_eq!(message.role, crate::Role::System);
        assert!(message.content.contains("Voice mode: off"));
        assert!(message.content.contains("Record key: Ctrl+B"));
        assert!(
            message
                .content
                .contains("Recording artifact: PCM16 WAV writer ready")
        );
        assert!(
            message
                .content
                .contains("TUI continuous capture is ready through voice_capture")
        );
        assert!(!app.is_thinking);
    }

    #[test]
    fn slash_voice_status_reports_recording_artifact_thresholds() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_voice_command(Some("status"));

        let message = app.messages.last().unwrap();
        assert!(message.content.contains("16000 Hz mono"));
        assert!(message.content.contains("min speech 0.3s"));
        assert!(message.content.contains("no-speech timeout 15s"));
    }

    #[test]
    fn slash_voice_status_reports_audio_cue_readiness() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_voice_command(Some("status"));

        let message = app.messages.last().unwrap();
        assert!(message.content.contains("Audio cues: enabled"));
        assert!(message.content.contains("start=880Hz x1"));
        assert!(message.content.contains("stop=660Hz x2"));
    }

    #[test]
    fn slash_voice_status_respects_disabled_audio_cues() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (_event_tx, event_rx) = mpsc::unbounded_channel();
        let voice = VoiceConfig {
            beep_enabled: false,
            ..VoiceConfig::default()
        };
        let mut app = App::new(
            cmd_tx,
            event_rx,
            "test-model".to_string(),
            "test-session-123".to_string(),
        )
        .with_voice_config(&voice);
        app.handle_voice_command(Some("status"));

        let message = app.messages.last().unwrap();
        assert!(
            message
                .content
                .contains("Audio cues: disabled by voice.beep_enabled=false")
        );
    }

    #[test]
    fn slash_voice_status_reports_tts_playback_readiness() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        app.handle_voice_command(Some("status"));

        let message = app.messages.last().unwrap();
        assert!(
            message
                .content
                .contains("TTS playback: Markdown cleanup and MP3 cache planning ready")
        );
        assert!(message.content.contains("max 4000 chars"));
    }

    #[test]
    fn slash_voice_tts_enables_voice_guidance() {
        let (mut app, _cmd_rx, _event_tx) = make_app();
        for c in "/voice tts".chars() {
            app.handle_key_event(key(KeyCode::Char(c)));
        }
        app.handle_key_event(key(KeyCode::Enter));

        assert!(app.voice.enabled);
        assert!(app.voice.tts);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("TTS guidance enabled")
        );
    }

    #[test]
    fn voice_status_bar_reflects_capture_phases() {
        let mut voice = TuiVoiceStatus {
            enabled: true,
            ..TuiVoiceStatus::default()
        };
        assert_eq!(voice.status_bar_hint(), "Voice:on Ctrl+B");

        voice.recording = true;
        assert_eq!(voice.status_bar_hint(), "Voice:rec Ctrl+B");

        voice.recording = false;
        voice.processing = true;
        assert_eq!(voice.status_bar_hint(), "Voice:stt Ctrl+B");

        voice.processing = false;
        voice.continuous = true;
        assert_eq!(voice.status_bar_hint(), "Voice:loop Ctrl+B");
    }

    #[test]
    fn configured_voice_record_key_starts_voice_capture() {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (_event_tx, event_rx) = mpsc::unbounded_channel();
        let voice = VoiceConfig {
            record_key: "ctrl+o".to_string(),
            provider: "edge".to_string(),
            ..VoiceConfig::default()
        };
        let mut app = App::new(
            cmd_tx,
            event_rx,
            "test-model".to_string(),
            "test-session-123".to_string(),
        )
        .with_voice_config(&voice);
        app.voice.audio_environment.capture_available = true;
        app.voice.audio_environment.capture_backend = "test-recorder".to_string();
        app.voice.transcription_ready = true;

        app.handle_voice_command(Some("on"));
        app.handle_key_event(key_with_mod(KeyCode::Char('O'), KeyModifiers::CONTROL));

        let message = app.messages.last().unwrap();
        assert_eq!(app.voice.record_key_label, "Ctrl+O");
        assert_eq!(message.role, crate::Role::System);
        assert!(message.content.contains("Recording with Ctrl+O"));
        assert!(message.content.contains("keep listening"));
        assert!(app.voice.recording);
        assert!(app.voice.continuous);
        assert!(app.is_thinking);

        match cmd_rx.try_recv().expect("voice command") {
            AgentCommand::VoiceCapture {
                duration_seconds,
                silence_threshold,
            } => {
                assert_eq!(duration_seconds, hakimi_tools::NO_SPEECH_TIMEOUT_SECONDS);
                assert_eq!(silence_threshold, app.voice.silence_threshold);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn voice_record_key_cancels_active_capture() {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (_event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            cmd_tx,
            event_rx,
            "test-model".to_string(),
            "test-session-123".to_string(),
        );
        app.voice.enabled = true;
        app.voice.continuous = true;
        app.voice.recording = true;
        app.is_thinking = true;

        app.handle_key_event(key_with_mod(KeyCode::Char('b'), KeyModifiers::CONTROL));

        assert!(!app.voice.recording);
        assert!(!app.voice.processing);
        assert!(!app.voice.continuous);
        assert!(app.is_thinking);
        assert!(
            app.messages
                .last()
                .expect("message")
                .content
                .contains("Stopping continuous voice capture")
        );
        match cmd_rx.try_recv().expect("cancel command") {
            AgentCommand::CancelVoiceCapture => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn voice_cancel_event_clears_running_capture_activity() {
        let (mut app, mut cmd_rx, event_tx) = make_app();
        app.voice.enabled = true;
        app.voice.continuous = true;
        app.voice.recording = true;
        app.is_thinking = true;
        app.tool_activity.push(ToolActivity {
            name: "voice_capture".to_string(),
            arguments_summary: "{}".to_string(),
            status: ToolStatus::Running,
            timestamp: Utc::now(),
        });

        event_tx
            .send(AgentEvent::VoiceCaptureCancelled)
            .expect("send cancel event");
        event_tx.send(AgentEvent::Done).expect("send done event");
        app.poll_agent_events();

        assert!(!app.voice.recording);
        assert!(!app.voice.processing);
        assert!(!app.voice.continuous);
        assert!(!app.is_thinking);
        assert_eq!(app.tool_activity[0].status, ToolStatus::Error);
        assert!(cmd_rx.try_recv().is_err());
        assert!(
            app.messages
                .last()
                .expect("message")
                .content
                .contains("Voice capture stopped")
        );
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
    fn poll_voice_transcript_adds_user_message_and_keeps_stt_state() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.voice.recording = true;

        event_tx
            .send(crate::AgentEvent::VoiceTranscript {
                transcript: "turn on the lights".to_string(),
                audio_path: Some("/tmp/hakimi_voice.wav".to_string()),
            })
            .unwrap();
        app.poll_agent_events();

        assert!(!app.voice.recording);
        assert!(app.voice.processing);
        assert_eq!(app.messages.last().unwrap().role, crate::Role::User);
        assert_eq!(app.messages.last().unwrap().content, "turn on the lights");
    }

    #[test]
    fn continuous_voice_restarts_after_response_done() {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.voice.enabled = true;
        app.voice.continuous = true;
        app.voice.recording = true;

        event_tx
            .send(crate::AgentEvent::VoiceTranscript {
                transcript: "summarize the page".to_string(),
                audio_path: Some("/tmp/hakimi_voice.wav".to_string()),
            })
            .unwrap();
        event_tx
            .send(crate::AgentEvent::Response("summary complete".to_string()))
            .unwrap();
        event_tx.send(crate::AgentEvent::Done).unwrap();
        app.poll_agent_events();

        assert!(app.voice.recording);
        assert!(app.voice.continuous);
        assert!(!app.voice.restart_pending);
        assert_eq!(app.voice.consecutive_no_speech, 0);
        assert!(app.is_thinking);
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("listening again")
        );
        match cmd_rx.try_recv().expect("restarted capture") {
            AgentCommand::VoiceCapture {
                duration_seconds,
                silence_threshold,
            } => {
                assert_eq!(duration_seconds, hakimi_tools::NO_SPEECH_TIMEOUT_SECONDS);
                assert_eq!(silence_threshold, app.voice.silence_threshold);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn poll_voice_no_speech_clears_capture_state() {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.voice.recording = true;
        app.voice.processing = true;
        app.is_thinking = true;

        event_tx
            .send(crate::AgentEvent::VoiceNoSpeech {
                reason: "recording peak RMS 10 is below threshold 200".to_string(),
                audio_path: Some("/tmp/quiet.wav".to_string()),
            })
            .unwrap();
        app.poll_agent_events();

        assert!(!app.voice.recording);
        assert!(!app.voice.processing);
        assert!(!app.is_thinking);
        assert!(app.messages.last().unwrap().content.contains("peak RMS"));
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("/tmp/quiet.wav")
        );
    }

    #[test]
    fn continuous_voice_restarts_after_no_speech_below_limit() {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.voice.enabled = true;
        app.voice.continuous = true;
        app.voice.recording = true;
        app.is_thinking = true;

        event_tx
            .send(crate::AgentEvent::VoiceNoSpeech {
                reason: "No speech transcript detected.".to_string(),
                audio_path: None,
            })
            .unwrap();
        event_tx.send(crate::AgentEvent::Done).unwrap();
        app.poll_agent_events();

        assert_eq!(app.voice.consecutive_no_speech, 1);
        assert!(app.voice.recording);
        assert!(app.voice.continuous);
        assert!(app.is_thinking);
        assert!(app.messages.iter().any(|message| {
            message
                .content
                .contains("Listening will restart automatically")
        }));
        match cmd_rx.try_recv().expect("restarted capture") {
            AgentCommand::VoiceCapture { .. } => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn continuous_voice_stops_after_three_no_speech_recordings() {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let mut app = App::new(cmd_tx, event_rx, "m".to_string(), "s".to_string());
        app.voice.enabled = true;
        app.voice.continuous = true;
        app.voice.recording = true;
        app.voice.consecutive_no_speech = 2;
        app.is_thinking = true;

        event_tx
            .send(crate::AgentEvent::VoiceNoSpeech {
                reason: "No speech transcript detected.".to_string(),
                audio_path: None,
            })
            .unwrap();
        event_tx.send(crate::AgentEvent::Done).unwrap();
        app.poll_agent_events();

        assert_eq!(app.voice.consecutive_no_speech, 3);
        assert!(!app.voice.recording);
        assert!(!app.voice.continuous);
        assert!(!app.voice.restart_pending);
        assert!(!app.is_thinking);
        assert!(cmd_rx.try_recv().is_err());
        assert!(
            app.messages
                .last()
                .unwrap()
                .content
                .contains("stopped after 3 recordings without speech")
        );
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
