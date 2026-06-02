//! CLI command parsing for the Hakimi Agent.
//!
//! Provides the [`Command`] enum for REPL-level commands that start with `/`.

pub mod backup;
pub mod doctor;
pub mod entry;
pub mod knowledge;
pub mod onboarding;
pub mod profiles;
pub mod setup_wizard;
pub mod skills;
pub mod skin;

use std::fmt;

use hakimi_common::canonical_slash_command;
pub use hakimi_common::{
    SlashCommandCompletion, SlashCommandSpec, complete_slash_command_prefix, slash_command_catalog,
};

// ---------------------------------------------------------------------------
// Command enum
// ---------------------------------------------------------------------------

/// Slash-commands available in the interactive REPL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Show help / available commands.
    Help,
    /// Stop ongoing tasks or streaming.
    Stop,
    /// Restart the managed gateway service.
    Restart,
    /// Clear the terminal screen.
    Clear,
    /// Switch or display the current model.
    Model(Option<String>),
    /// Open or display configuration.
    Config(Option<String>),
    /// Resume a previous session by ID or title.
    Resume(Option<String>),
    /// Browse saved sessions.
    Sessions(Option<String>),
    /// Show recent local conversation history.
    History(Option<String>),
    /// Rewind recent user turns for editing.
    Undo(Option<String>),
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
    /// Plugin management.
    Plugins(Option<String>),
    /// Update system.
    Update,
    /// Authenticate / OAuth login
    Auth(Option<String>),
    /// Backup state/memory/sessions
    Backup(Option<String>),
    /// Copy a recent local assistant response to the system clipboard.
    Copy(Option<String>),
    /// Open/control browser
    Browser(Option<String>),
    /// Manage file checkpoints
    Checkpoints(Option<String>),
    /// Export database dump
    Dump(Option<String>),
    /// Gateway management
    Gateway(Option<String>),
    /// Manage agent goals
    Goals(Option<String>),
    /// Manage shell hooks
    Hooks(Option<String>),
    /// Manage Kanban boards
    Kanban(Option<String>),
    /// Manage knowledge graph entries.
    Knowledge(Option<String>),
    /// View logs
    Logs(Option<String>),
    /// Manage MCP servers
    Mcp(Option<String>),
    /// Manage memory
    Memory(Option<String>),
    /// Gateway pairing
    Pairing(Option<String>),
    /// Platform management
    Platforms(Option<String>),
    /// Provider management
    Providers(Option<String>),
    /// CLI Skin / Theme management
    Skin(Option<String>),
    /// Daily tips / tutorial
    Tips(Option<String>),
    /// Configure tool sets
    ToolsConfig(Option<String>),
    /// Uninstall system components
    Uninstall(Option<String>),
    /// Voice control
    Voice(Option<String>),
    /// Webhook management
    Webhook(Option<String>),
    /// Exit a local interactive surface.
    Quit,
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

        match canonical_slash_command(cmd)? {
            "help" => Some(Command::Help),
            "stop" => Some(Command::Stop),
            "restart" => Some(Command::Restart),
            "clear" => Some(Command::Clear),
            "model" => Some(Command::Model(arg.map(String::from))),
            "config" => Some(Command::Config(arg.map(String::from))),
            "resume" => Some(Command::Resume(arg.map(String::from))),
            "sessions" => Some(Command::Sessions(arg.map(String::from))),
            "history" => Some(Command::History(arg.map(String::from))),
            "undo" => Some(Command::Undo(arg.map(String::from))),
            "tools" => Some(Command::Tools(arg.map(String::from))),
            "skills" => Some(Command::Skills(arg.map(String::from))),
            "status" => Some(Command::Status),
            "usage" => Some(Command::Usage),
            "profile" => Some(Command::Profile(arg.map(String::from))),
            "doctor" => Some(Command::Doctor),
            "setup" => Some(Command::Setup),
            "cron" => Some(Command::Cron(arg.map(String::from))),
            "plugins" => Some(Command::Plugins(arg.map(String::from))),
            "update" => Some(Command::Update),
            "auth" => Some(Command::Auth(arg.map(String::from))),
            "backup" => Some(Command::Backup(arg.map(String::from))),
            "copy" => Some(Command::Copy(arg.map(String::from))),
            "browser" => Some(Command::Browser(arg.map(String::from))),
            "checkpoints" => Some(Command::Checkpoints(arg.map(String::from))),
            "dump" => Some(Command::Dump(arg.map(String::from))),
            "gateway" => Some(Command::Gateway(arg.map(String::from))),
            "goals" => Some(Command::Goals(arg.map(String::from))),
            "hooks" => Some(Command::Hooks(arg.map(String::from))),
            "kanban" => Some(Command::Kanban(arg.map(String::from))),
            "knowledge" => Some(Command::Knowledge(arg.map(String::from))),
            "logs" => Some(Command::Logs(arg.map(String::from))),
            "mcp" => Some(Command::Mcp(arg.map(String::from))),
            "memory" => Some(Command::Memory(arg.map(String::from))),
            "pairing" => Some(Command::Pairing(arg.map(String::from))),
            "platforms" => Some(Command::Platforms(arg.map(String::from))),
            "providers" => Some(Command::Providers(arg.map(String::from))),
            "skin" => Some(Command::Skin(arg.map(String::from))),
            "tips" => Some(Command::Tips(arg.map(String::from))),
            "tools_config" => Some(Command::ToolsConfig(arg.map(String::from))),
            "uninstall" => Some(Command::Uninstall(arg.map(String::from))),
            "voice" => Some(Command::Voice(arg.map(String::from))),
            "webhook" => Some(Command::Webhook(arg.map(String::from))),
            "quit" => Some(Command::Quit),
            _ => None,
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Help => write!(f, "/help"),
            Command::Stop => write!(f, "/stop"),
            Command::Restart => write!(f, "/restart"),
            Command::Clear => write!(f, "/clear"),
            Command::Model(None) => write!(f, "/model"),
            Command::Model(Some(m)) => write!(f, "/model {m}"),
            Command::Config(None) => write!(f, "/config"),
            Command::Config(Some(k)) => write!(f, "/config {k}"),
            Command::Resume(None) => write!(f, "/resume"),
            Command::Resume(Some(id)) => write!(f, "/resume {id}"),
            Command::Sessions(None) => write!(f, "/sessions"),
            Command::Sessions(Some(s)) => write!(f, "/sessions {s}"),
            Command::History(None) => write!(f, "/history"),
            Command::History(Some(h)) => write!(f, "/history {h}"),
            Command::Undo(None) => write!(f, "/undo"),
            Command::Undo(Some(n)) => write!(f, "/undo {n}"),
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
            Command::Plugins(None) => write!(f, "/plugins"),
            Command::Plugins(Some(p)) => write!(f, "/plugins {p}"),
            Command::Update => write!(f, "/update"),
            Command::Auth(None) => write!(f, "/auth"),
            Command::Auth(Some(a)) => write!(f, "/auth {a}"),
            Command::Backup(None) => write!(f, "/backup"),
            Command::Backup(Some(b)) => write!(f, "/backup {b}"),
            Command::Copy(None) => write!(f, "/copy"),
            Command::Copy(Some(c)) => write!(f, "/copy {c}"),
            Command::Browser(None) => write!(f, "/browser"),
            Command::Browser(Some(b)) => write!(f, "/browser {b}"),
            Command::Checkpoints(None) => write!(f, "/checkpoints"),
            Command::Checkpoints(Some(c)) => write!(f, "/checkpoints {c}"),
            Command::Dump(None) => write!(f, "/dump"),
            Command::Dump(Some(d)) => write!(f, "/dump {d}"),
            Command::Gateway(None) => write!(f, "/gateway"),
            Command::Gateway(Some(g)) => write!(f, "/gateway {g}"),
            Command::Goals(None) => write!(f, "/goals"),
            Command::Goals(Some(g)) => write!(f, "/goals {g}"),
            Command::Hooks(None) => write!(f, "/hooks"),
            Command::Hooks(Some(h)) => write!(f, "/hooks {h}"),
            Command::Kanban(None) => write!(f, "/kanban"),
            Command::Kanban(Some(k)) => write!(f, "/kanban {k}"),
            Command::Knowledge(None) => write!(f, "/knowledge"),
            Command::Knowledge(Some(k)) => write!(f, "/knowledge {k}"),
            Command::Logs(None) => write!(f, "/logs"),
            Command::Logs(Some(l)) => write!(f, "/logs {l}"),
            Command::Mcp(None) => write!(f, "/mcp"),
            Command::Mcp(Some(m)) => write!(f, "/mcp {m}"),
            Command::Memory(None) => write!(f, "/memory"),
            Command::Memory(Some(m)) => write!(f, "/memory {m}"),
            Command::Pairing(None) => write!(f, "/pairing"),
            Command::Pairing(Some(p)) => write!(f, "/pairing {p}"),
            Command::Platforms(None) => write!(f, "/platforms"),
            Command::Platforms(Some(p)) => write!(f, "/platforms {p}"),
            Command::Providers(None) => write!(f, "/providers"),
            Command::Providers(Some(p)) => write!(f, "/providers {p}"),
            Command::Skin(None) => write!(f, "/skin"),
            Command::Skin(Some(s)) => write!(f, "/skin {s}"),
            Command::Tips(None) => write!(f, "/tips"),
            Command::Tips(Some(t)) => write!(f, "/tips {t}"),
            Command::ToolsConfig(None) => write!(f, "/tools_config"),
            Command::ToolsConfig(Some(t)) => write!(f, "/tools_config {t}"),
            Command::Uninstall(None) => write!(f, "/uninstall"),
            Command::Uninstall(Some(u)) => write!(f, "/uninstall {u}"),
            Command::Voice(None) => write!(f, "/voice"),
            Command::Voice(Some(v)) => write!(f, "/voice {v}"),
            Command::Webhook(None) => write!(f, "/webhook"),
            Command::Webhook(Some(w)) => write!(f, "/webhook {w}"),
            Command::Quit => write!(f, "/quit"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_slash_commands() {
        assert_eq!(Command::parse("/help"), Some(Command::Help));
        assert_eq!(Command::parse("/stop"), Some(Command::Stop));
        assert_eq!(Command::parse("/restart"), Some(Command::Restart));
        assert_eq!(Command::parse("/clear"), Some(Command::Clear));
        assert_eq!(
            Command::parse("/model gpt-4o"),
            Some(Command::Model(Some("gpt-4o".into())))
        );
        assert_eq!(Command::parse("/status"), Some(Command::Status));
        assert_eq!(Command::parse("hello world"), None);
    }

    #[test]
    fn test_parse_new_commands() {
        assert_eq!(Command::parse("/profile"), Some(Command::Profile(None)));
        assert_eq!(
            Command::parse("/profile work"),
            Some(Command::Profile(Some("work".into())))
        );
        assert_eq!(Command::parse("/history"), Some(Command::History(None)));
        assert_eq!(Command::parse("/sessions"), Some(Command::Sessions(None)));
        assert_eq!(
            Command::parse("/sessions show abc123"),
            Some(Command::Sessions(Some("show abc123".into())))
        );
        assert_eq!(
            Command::parse("/sess 5"),
            Some(Command::Sessions(Some("5".into())))
        );
        assert_eq!(
            Command::parse("/history 3"),
            Some(Command::History(Some("3".into())))
        );
        assert_eq!(
            Command::parse("/hist 2"),
            Some(Command::History(Some("2".into())))
        );
        assert_eq!(Command::parse("/undo"), Some(Command::Undo(None)));
        assert_eq!(
            Command::parse("/rewind 2"),
            Some(Command::Undo(Some("2".into())))
        );
        assert_eq!(Command::parse("/doctor"), Some(Command::Doctor));
        assert_eq!(Command::parse("/setup"), Some(Command::Setup));
        assert_eq!(Command::parse("/copy"), Some(Command::Copy(None)));
        assert_eq!(
            Command::parse("/copy 2"),
            Some(Command::Copy(Some("2".into())))
        );
        assert_eq!(
            Command::parse("/cp 3"),
            Some(Command::Copy(Some("3".into())))
        );
        assert_eq!(Command::parse("/cron"), Some(Command::Cron(None)));
        assert_eq!(
            Command::parse("/cron list"),
            Some(Command::Cron(Some("list".into())))
        );
        assert_eq!(Command::parse("/plugins"), Some(Command::Plugins(None)));
        assert_eq!(
            Command::parse("/plugins list"),
            Some(Command::Plugins(Some("list".into())))
        );
        assert_eq!(
            Command::parse("/knowledge stats"),
            Some(Command::Knowledge(Some("stats".into())))
        );
        assert_eq!(
            Command::parse("/kg search alice"),
            Some(Command::Knowledge(Some("search alice".into())))
        );
        assert_eq!(Command::parse("/quit"), Some(Command::Quit));
        assert_eq!(Command::parse("/exit"), Some(Command::Quit));
    }

    #[test]
    fn slash_command_catalog_aliases_are_parseable() {
        for spec in slash_command_catalog() {
            assert!(Command::parse(&format!("/{}", spec.name)).is_some());
            for alias in spec.aliases {
                assert!(
                    Command::parse(&format!("/{alias}")).is_some(),
                    "alias `{alias}` should parse"
                );
            }
        }
    }

    #[test]
    fn slash_completion_expands_single_prefix_and_alias() {
        let model = complete_slash_command_prefix("/mod");
        assert_eq!(model.replacement, Some("/model ".to_string()));
        assert_eq!(model.matches[0].name, "model");

        let copy = complete_slash_command_prefix("/cp");
        assert_eq!(copy.replacement, Some("/copy ".to_string()));
        assert_eq!(copy.matches[0].name, "copy");
    }

    #[test]
    fn slash_completion_reports_ambiguous_prefix_without_guessing() {
        let completion = complete_slash_command_prefix("/c");
        assert!(completion.replacement.is_none());
        assert!(completion.matches.iter().any(|spec| spec.name == "clear"));
        assert!(completion.matches.iter().any(|spec| spec.name == "config"));
        assert!(completion.matches.iter().any(|spec| spec.name == "copy"));
        assert!(completion.matches.iter().any(|spec| spec.name == "cron"));
    }

    #[test]
    fn slash_completion_ignores_non_slash_input() {
        let completion = complete_slash_command_prefix("model");
        assert!(completion.replacement.is_none());
        assert!(completion.matches.is_empty());
    }
}
