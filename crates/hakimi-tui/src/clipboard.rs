//! Clipboard helpers for local TUI slash commands.

use crate::{ChatMessage, Role};
use base64::Engine;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyAssistantResponse {
    Copied { chars: usize },
    InvalidArgument,
    NoAssistantMessage,
    ClipboardUnavailable,
}

impl CopyAssistantResponse {
    pub fn message(&self) -> &'static str {
        match self {
            CopyAssistantResponse::Copied { .. } => "copied to clipboard",
            CopyAssistantResponse::InvalidArgument => "usage: /copy [number]",
            CopyAssistantResponse::NoAssistantMessage => {
                "nothing to copy - start a conversation first"
            }
            CopyAssistantResponse::ClipboardUnavailable => {
                "clipboard copy failed - install pbcopy, wl-copy, xclip, xsel, PowerShell, or use an OSC 52-capable terminal"
            }
        }
    }

    pub fn is_error(&self) -> bool {
        !matches!(self, CopyAssistantResponse::Copied { .. })
    }
}

pub fn parse_copy_index(arg: Option<&str>) -> Result<usize, CopyAssistantResponse> {
    let raw = arg.unwrap_or_default().trim();
    if raw.is_empty() {
        return Ok(1);
    }

    match raw.parse::<usize>() {
        Ok(index) if index > 0 => Ok(index),
        _ => Err(CopyAssistantResponse::InvalidArgument),
    }
}

pub fn nth_latest_assistant_text(messages: &[ChatMessage], index: usize) -> Option<&str> {
    messages
        .iter()
        .rev()
        .filter(|message| message.role == Role::Assistant)
        .filter_map(|message| {
            let content = message.content.as_str();
            if content.trim().is_empty() {
                None
            } else {
                Some(content)
            }
        })
        .nth(index.saturating_sub(1))
}

pub fn copy_assistant_response<F>(
    messages: &[ChatMessage],
    arg: Option<&str>,
    writer: F,
) -> CopyAssistantResponse
where
    F: FnOnce(&str) -> bool,
{
    let index = match parse_copy_index(arg) {
        Ok(index) => index,
        Err(response) => return response,
    };

    let Some(text) = nth_latest_assistant_text(messages, index) else {
        return CopyAssistantResponse::NoAssistantMessage;
    };

    if writer(text) {
        CopyAssistantResponse::Copied {
            chars: text.chars().count(),
        }
    } else {
        CopyAssistantResponse::ClipboardUnavailable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClipboardCommand {
    program: String,
    args: Vec<String>,
}

fn clipboard_commands() -> Vec<ClipboardCommand> {
    match std::env::consts::OS {
        "macos" => vec![ClipboardCommand {
            program: "pbcopy".to_string(),
            args: Vec::new(),
        }],
        "windows" => vec![
            ClipboardCommand {
                program: "powershell".to_string(),
                args: powershell_set_clipboard_args(),
            },
            ClipboardCommand {
                program: "pwsh".to_string(),
                args: powershell_set_clipboard_args(),
            },
        ],
        _ => {
            let mut commands = Vec::new();
            if std::env::var_os("WSL_INTEROP").is_some()
                || std::env::var_os("WSL_DISTRO_NAME").is_some()
            {
                commands.push(ClipboardCommand {
                    program: "powershell.exe".to_string(),
                    args: powershell_set_clipboard_args(),
                });
            }
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                commands.push(ClipboardCommand {
                    program: "wl-copy".to_string(),
                    args: vec!["--type".to_string(), "text/plain".to_string()],
                });
            }
            commands.push(ClipboardCommand {
                program: "xclip".to_string(),
                args: vec![
                    "-selection".to_string(),
                    "clipboard".to_string(),
                    "-in".to_string(),
                ],
            });
            commands.push(ClipboardCommand {
                program: "xsel".to_string(),
                args: vec!["--clipboard".to_string(), "--input".to_string()],
            });
            commands
        }
    }
}

fn powershell_set_clipboard_args() -> Vec<String> {
    vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        "Set-Clipboard -Value $input".to_string(),
    ]
}

pub fn write_clipboard_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    for candidate in clipboard_commands() {
        let mut child = match Command::new(&candidate.program)
            .args(&candidate.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => continue,
        };

        let wrote_stdin = child
            .stdin
            .as_mut()
            .map(|stdin| stdin.write_all(text.as_bytes()).is_ok())
            .unwrap_or(false);

        drop(child.stdin.take());

        if wrote_stdin && child.wait().map(|status| status.success()).unwrap_or(false) {
            return true;
        }
    }

    write_osc52_clipboard_text(text)
}

fn osc52_sequence(text: &str) -> String {
    let payload = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{payload}\x07")
}

fn write_osc52_clipboard_text(text: &str) -> bool {
    let mut stdout = std::io::stdout();
    let sequence = osc52_sequence(text);
    stdout
        .write_all(sequence.as_bytes())
        .and_then(|_| stdout.flush())
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(content: &str) -> ChatMessage {
        ChatMessage::assistant(content)
    }

    #[test]
    fn parse_copy_index_defaults_to_latest() {
        assert_eq!(parse_copy_index(None), Ok(1));
        assert_eq!(parse_copy_index(Some("  ")), Ok(1));
        assert_eq!(parse_copy_index(Some("2")), Ok(2));
    }

    #[test]
    fn parse_copy_index_rejects_invalid_values() {
        assert_eq!(
            parse_copy_index(Some("0")),
            Err(CopyAssistantResponse::InvalidArgument)
        );
        assert_eq!(
            parse_copy_index(Some("-1")),
            Err(CopyAssistantResponse::InvalidArgument)
        );
        assert_eq!(
            parse_copy_index(Some("two")),
            Err(CopyAssistantResponse::InvalidArgument)
        );
    }

    #[test]
    fn nth_latest_assistant_text_ignores_non_assistant_messages() {
        let messages = vec![
            ChatMessage::user("question"),
            assistant("first"),
            ChatMessage::system("notice"),
            assistant("second"),
            ChatMessage::error("ignored"),
        ];

        assert_eq!(nth_latest_assistant_text(&messages, 1), Some("second"));
        assert_eq!(nth_latest_assistant_text(&messages, 2), Some("first"));
        assert_eq!(nth_latest_assistant_text(&messages, 3), None);
    }

    #[test]
    fn copy_assistant_response_reports_success() {
        let messages = vec![assistant("first"), assistant("second")];

        let response = copy_assistant_response(&messages, Some("2"), |text| text == "first");

        assert_eq!(response, CopyAssistantResponse::Copied { chars: 5 });
    }

    #[test]
    fn copy_assistant_response_defaults_to_latest() {
        let messages = vec![assistant("older"), assistant("newer")];

        let response = copy_assistant_response(&messages, None, |text| text == "newer");

        assert_eq!(response, CopyAssistantResponse::Copied { chars: 5 });
    }

    #[test]
    fn copy_assistant_response_reports_missing_when_index_is_out_of_range() {
        let messages = vec![assistant("only response")];

        let response = copy_assistant_response(&messages, Some("2"), |_| true);

        assert_eq!(response, CopyAssistantResponse::NoAssistantMessage);
    }

    #[test]
    fn copy_assistant_response_reports_writer_failure() {
        let messages = vec![assistant("answer")];

        let response = copy_assistant_response(&messages, None, |_| false);

        assert_eq!(response, CopyAssistantResponse::ClipboardUnavailable);
    }

    #[test]
    fn osc52_sequence_wraps_base64_payload() {
        assert_eq!(osc52_sequence("hello"), "\x1b]52;c;aGVsbG8=\x07");
    }
}
