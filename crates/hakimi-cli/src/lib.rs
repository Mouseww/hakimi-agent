//! CLI command parsing for the Hakimi Agent.
//!
//! Provides the [`Command`] enum for REPL-level commands that start with `/`.

pub mod profiles;
pub mod setup_wizard;
pub mod doctor;

use std::fmt;

// ---------------------------------------------------------------------------
// Command enum
// ---------------------------------------------------------------------------

/// Slash-commands available in the interactive REPL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Show help / available commands.
    Help,
    /// Exit the REPL.
    Quit,
    /// Clear the terminal screen.
    Clear,
    /// Switch or display the current model.
    Model(Option<String>),
    /// Open or display configuration.
    Config(Option<String>),
    /// Resume a previous session by ID or title.
    Resume(Option<String>),
    /// List or describe available tools.
    Tools(Option<String>),
    /// List or describe available skills.
    Skills(Option<String>),
    /// Show current session / agent status.
    Status,
    /// Show token usage statistics.
    Usage,
    /// Profile management.
    Profile(Option<String>),
    /// Run diagnostics.
    Doctor,
    /// Run setup wizard.
    Setup,
    /// Cron job management.
    Cron(Option<String>),
}

impl Command {
    /// Try to parse a REPL input line into a [`Command`].
    ///
    /// Returns `None` when the input is not a slash-command (i.e. it should
    /// be forwarded to the LLM as a chat message).
    pub fn parse(input: &str) -> Option<Command> {
        let trimmed = input.trim();

        let rest = trimmed.strip_prefix('/')?;

        let (cmd, arg) = match rest.split_once(char::is_whitespace) {
            Some((c, a)) => (c, Some(a.trim())),
            None => (rest, None),
        };

        match cmd.to_lowercase().as_str() {
            "help" | "h" | "?" => Some(Command::Help),
            "quit" | "exit" | "q" => Some(Command::Quit),
            "clear" | "cls" => Some(Command::Clear),
            "model" | "m" => Some(Command::Model(arg.map(String::from))),
            "config" | "cfg" => Some(Command::Config(arg.map(String::from))),
            "resume" | "r" => Some(Command::Resume(arg.map(String::from))),
            "tools" | "t" => Some(Command::Tools(arg.map(String::from))),
            "skills" | "s" => Some(Command::Skills(arg.map(String::from))),
            "status" => Some(Command::Status),
            "usage" | "u" => Some(Command::Usage),
            "profile" | "p" => Some(Command::Profile(arg.map(String::from))),
            "doctor" => Some(Command::Doctor),
            "setup" => Some(Command::Setup),
            "cron" => Some(Command::Cron(arg.map(String::from))),
            _ => None,
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Help => write!(f, "/help"),
            Command::Quit => write!(f, "/quit"),
            Command::Clear => write!(f, "/clear"),
            Command::Model(None) => write!(f, "/model"),
            Command::Model(Some(m)) => write!(f, "/model {m}"),
            Command::Config(None) => write!(f, "/config"),
            Command::Config(Some(k)) => write!(f, "/config {k}"),
            Command::Resume(None) => write!(f, "/resume"),
            Command::Resume(Some(id)) => write!(f, "/resume {id}"),
            Command::Tools(None) => write!(f, "/tools"),
            Command::Tools(Some(t)) => write!(f, "/tools {t}"),
            Command::Skills(None) => write!(f, "/skills"),
            Command::Skills(Some(s)) => write!(f, "/skills {s}"),
            Command::Status => write!(f, "/status"),
            Command::Usage => write!(f, "/usage"),
            Command::Profile(None) => write!(f, "/profile"),
            Command::Profile(Some(p)) => write!(f, "/profile {p}"),
            Command::Doctor => write!(f, "/doctor"),
            Command::Setup => write!(f, "/setup"),
            Command::Cron(None) => write!(f, "/cron"),
            Command::Cron(Some(c)) => write!(f, "/cron {c}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_slash_commands() {
        assert_eq!(Command::parse("/help"), Some(Command::Help));
        assert_eq!(Command::parse("/quit"), Some(Command::Quit));
        assert_eq!(Command::parse("/clear"), Some(Command::Clear));
        assert_eq!(Command::parse("/model gpt-4o"), Some(Command::Model(Some("gpt-4o".into()))));
        assert_eq!(Command::parse("/status"), Some(Command::Status));
        assert_eq!(Command::parse("hello world"), None);
    }

    #[test]
    fn test_parse_new_commands() {
        assert_eq!(Command::parse("/profile"), Some(Command::Profile(None)));
        assert_eq!(Command::parse("/profile work"), Some(Command::Profile(Some("work".into()))));
        assert_eq!(Command::parse("/doctor"), Some(Command::Doctor));
        assert_eq!(Command::parse("/setup"), Some(Command::Setup));
        assert_eq!(Command::parse("/cron"), Some(Command::Cron(None)));
        assert_eq!(Command::parse("/cron list"), Some(Command::Cron(Some("list".into()))));
    }
}
