/// Metadata for a user-facing slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandSpec {
    /// Canonical command name without the leading slash.
    pub name: &'static str,
    /// Alternate names without the leading slash.
    pub aliases: &'static [&'static str],
    /// Optional argument hint used by completion/help surfaces.
    pub args_hint: &'static str,
    /// Short user-facing summary.
    pub summary: &'static str,
    /// Coarse grouping for menus and completion surfaces.
    pub category: &'static str,
}

impl SlashCommandSpec {
    fn matches_name(&self, value: &str) -> bool {
        self.name.eq_ignore_ascii_case(value)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(value))
    }

    fn matches_prefix(&self, value: &str) -> bool {
        self.name.starts_with(value) || self.aliases.iter().any(|alias| alias.starts_with(value))
    }
}

/// Result of completing a slash command prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandCompletion {
    /// Replacement text for the command token, including the leading slash.
    pub replacement: Option<String>,
    /// Matching command specs, ordered by catalog order.
    pub matches: Vec<&'static SlashCommandSpec>,
}

const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec {
        name: "help",
        aliases: &["h", "?"],
        args_hint: "",
        summary: "Show command reference",
        category: "chat",
    },
    SlashCommandSpec {
        name: "stop",
        aliases: &[],
        args_hint: "",
        summary: "Cancel the active task or stream",
        category: "chat",
    },
    SlashCommandSpec {
        name: "restart",
        aliases: &[],
        args_hint: "",
        summary: "Restart the managed gateway service",
        category: "system",
    },
    SlashCommandSpec {
        name: "clear",
        aliases: &["cls"],
        args_hint: "",
        summary: "Clear conversation state",
        category: "chat",
    },
    SlashCommandSpec {
        name: "model",
        aliases: &["m"],
        args_hint: "[name]",
        summary: "Show or switch the active model",
        category: "agent",
    },
    SlashCommandSpec {
        name: "config",
        aliases: &["cfg"],
        args_hint: "[key]",
        summary: "Open or display configuration",
        category: "agent",
    },
    SlashCommandSpec {
        name: "resume",
        aliases: &["r"],
        args_hint: "[session]",
        summary: "Resume a previous session",
        category: "chat",
    },
    SlashCommandSpec {
        name: "history",
        aliases: &["hist"],
        args_hint: "[N]",
        summary: "Review recent conversation messages",
        category: "chat",
    },
    SlashCommandSpec {
        name: "tools",
        aliases: &["t"],
        args_hint: "[query]",
        summary: "List or describe available tools",
        category: "agent",
    },
    SlashCommandSpec {
        name: "skills",
        aliases: &["s"],
        args_hint: "[command]",
        summary: "List or manage loaded skills",
        category: "agent",
    },
    SlashCommandSpec {
        name: "status",
        aliases: &[],
        args_hint: "",
        summary: "Show current session status",
        category: "chat",
    },
    SlashCommandSpec {
        name: "usage",
        aliases: &["u"],
        args_hint: "",
        summary: "Show token usage, cost, and rate limits",
        category: "chat",
    },
    SlashCommandSpec {
        name: "profile",
        aliases: &["p"],
        args_hint: "[command]",
        summary: "Manage isolated profiles",
        category: "agent",
    },
    SlashCommandSpec {
        name: "doctor",
        aliases: &[],
        args_hint: "",
        summary: "Run setup and runtime diagnostics",
        category: "operations",
    },
    SlashCommandSpec {
        name: "setup",
        aliases: &[],
        args_hint: "",
        summary: "Run the setup wizard",
        category: "operations",
    },
    SlashCommandSpec {
        name: "cron",
        aliases: &[],
        args_hint: "[command]",
        summary: "Manage scheduled jobs",
        category: "operations",
    },
    SlashCommandSpec {
        name: "plugins",
        aliases: &["plugin"],
        args_hint: "[command]",
        summary: "Manage plugins",
        category: "agent",
    },
    SlashCommandSpec {
        name: "update",
        aliases: &[],
        args_hint: "",
        summary: "Update Hakimi",
        category: "system",
    },
    SlashCommandSpec {
        name: "auth",
        aliases: &[],
        args_hint: "[provider]",
        summary: "Show authentication status",
        category: "system",
    },
    SlashCommandSpec {
        name: "backup",
        aliases: &[],
        args_hint: "[output]",
        summary: "Back up state, memory, and sessions",
        category: "operations",
    },
    SlashCommandSpec {
        name: "copy",
        aliases: &["cp"],
        args_hint: "[N]",
        summary: "Copy a recent assistant response",
        category: "chat",
    },
    SlashCommandSpec {
        name: "browser",
        aliases: &["b"],
        args_hint: "[command]",
        summary: "Control browser sessions",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "checkpoints",
        aliases: &["ckpt"],
        args_hint: "[command]",
        summary: "Manage file checkpoints",
        category: "operations",
    },
    SlashCommandSpec {
        name: "dump",
        aliases: &[],
        args_hint: "[output]",
        summary: "Export a session database dump",
        category: "operations",
    },
    SlashCommandSpec {
        name: "gateway",
        aliases: &["gw"],
        args_hint: "[command]",
        summary: "Manage gateway runtime state",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "goals",
        aliases: &[],
        args_hint: "[command]",
        summary: "Manage agent goals",
        category: "agent",
    },
    SlashCommandSpec {
        name: "hooks",
        aliases: &[],
        args_hint: "[command]",
        summary: "Manage shell hooks",
        category: "operations",
    },
    SlashCommandSpec {
        name: "kanban",
        aliases: &["kb"],
        args_hint: "[command]",
        summary: "Manage Kanban tasks",
        category: "agent",
    },
    SlashCommandSpec {
        name: "logs",
        aliases: &["l"],
        args_hint: "[lines]",
        summary: "View recent logs",
        category: "operations",
    },
    SlashCommandSpec {
        name: "mcp",
        aliases: &[],
        args_hint: "[command]",
        summary: "Manage MCP servers",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "memory",
        aliases: &["mem"],
        args_hint: "[command]",
        summary: "View or clear persistent memory",
        category: "agent",
    },
    SlashCommandSpec {
        name: "pairing",
        aliases: &["pair"],
        args_hint: "[command]",
        summary: "Start gateway pairing",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "platforms",
        aliases: &[],
        args_hint: "[command]",
        summary: "List connected gateway platforms",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "providers",
        aliases: &[],
        args_hint: "[provider]",
        summary: "List supported LLM providers",
        category: "agent",
    },
    SlashCommandSpec {
        name: "skin",
        aliases: &["theme"],
        args_hint: "[name]",
        summary: "Configure CLI skin or theme",
        category: "system",
    },
    SlashCommandSpec {
        name: "tips",
        aliases: &["tip"],
        args_hint: "",
        summary: "Show daily tips",
        category: "system",
    },
    SlashCommandSpec {
        name: "tools_config",
        aliases: &["tc"],
        args_hint: "[command]",
        summary: "Configure tool sets",
        category: "agent",
    },
    SlashCommandSpec {
        name: "uninstall",
        aliases: &[],
        args_hint: "[target]",
        summary: "Uninstall system components",
        category: "system",
    },
    SlashCommandSpec {
        name: "voice",
        aliases: &["v"],
        args_hint: "[on|off]",
        summary: "Control voice mode",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "webhook",
        aliases: &["wh"],
        args_hint: "[command]",
        summary: "Manage webhook endpoints",
        category: "integrations",
    },
    SlashCommandSpec {
        name: "quit",
        aliases: &["exit"],
        args_hint: "",
        summary: "Exit the local interactive UI",
        category: "chat",
    },
];

/// Return the shared slash command catalog used by CLI, gateway, and TUI surfaces.
pub fn slash_command_catalog() -> &'static [SlashCommandSpec] {
    SLASH_COMMANDS
}

/// Resolve a command or alias to its canonical command name.
pub fn canonical_slash_command(value: &str) -> Option<&'static str> {
    let normalized = value.trim().trim_start_matches('/').to_ascii_lowercase();
    SLASH_COMMANDS
        .iter()
        .find(|spec| spec.matches_name(&normalized))
        .map(|spec| spec.name)
}

/// Complete a slash command token such as `/hist` or `/mo`.
pub fn complete_slash_command_prefix(prefix: &str) -> SlashCommandCompletion {
    let raw = prefix.trim();
    let normalized = raw.trim_start_matches('/').to_ascii_lowercase();
    if raw.is_empty() || !raw.starts_with('/') {
        return SlashCommandCompletion {
            replacement: None,
            matches: Vec::new(),
        };
    }

    if let Some(exact) = SLASH_COMMANDS
        .iter()
        .find(|spec| spec.matches_name(&normalized))
    {
        return SlashCommandCompletion {
            replacement: Some(replacement_for(exact)),
            matches: vec![exact],
        };
    }

    let matches: Vec<_> = SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.matches_prefix(&normalized))
        .collect();

    let replacement = match matches.as_slice() {
        [] => None,
        [single] => Some(replacement_for(single)),
        _ => common_command_prefix(&matches, &normalized).map(|common| format!("/{common}")),
    };

    SlashCommandCompletion {
        replacement,
        matches,
    }
}

fn replacement_for(spec: &SlashCommandSpec) -> String {
    let suffix = if spec.args_hint.is_empty() { "" } else { " " };
    format!("/{}{}", spec.name, suffix)
}

fn common_command_prefix(
    matches: &[&'static SlashCommandSpec],
    normalized: &str,
) -> Option<String> {
    let first = matches.first()?.name;
    let mut end = first.len();
    for spec in matches.iter().skip(1) {
        end = common_prefix_len(&first[..end], spec.name);
        if end == 0 {
            break;
        }
    }
    let common = &first[..end];
    (common.len() > normalized.len()).then(|| common.to_string())
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    for (left, right) in a.bytes().zip(b.bytes()) {
        if left != right {
            break;
        }
        len += 1;
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_completion_expands_single_prefix_and_alias() {
        let model = complete_slash_command_prefix("/mod");
        assert_eq!(model.replacement, Some("/model ".to_string()));
        assert_eq!(model.matches[0].name, "model");

        let copy = complete_slash_command_prefix("/cp");
        assert_eq!(copy.replacement, Some("/copy ".to_string()));
        assert_eq!(copy.matches[0].name, "copy");

        let browser = complete_slash_command_prefix("/b");
        assert_eq!(browser.replacement, Some("/browser ".to_string()));
        assert_eq!(browser.matches[0].name, "browser");
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

    #[test]
    fn canonical_slash_command_resolves_aliases() {
        assert_eq!(canonical_slash_command("/hist"), Some("history"));
        assert_eq!(canonical_slash_command("cp"), Some("copy"));
        assert_eq!(canonical_slash_command("/missing"), None);
    }
}
