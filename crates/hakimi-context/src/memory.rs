use async_trait::async_trait;
use hakimi_common::{Result, ToolDefinition};
use serde_json::Value as JsonValue;
use tracing::{debug, info, instrument, warn};

/// Trait for providing memory / long-term context to the agent.
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    /// Human-readable name of this memory provider.
    fn name(&self) -> &str;

    /// Whether this provider is available (e.g. the memory directory exists).
    fn is_available(&self) -> bool;

    /// Return a block of text suitable for inclusion in the system prompt.
    fn system_prompt_block(&self) -> String;

    /// Prefetch memory entries relevant to the given query.
    async fn prefetch(&self, query: &str) -> String;

    /// Return tool definitions this provider exposes.
    fn get_tool_definitions(&self) -> Vec<ToolDefinition>;

    /// Handle a tool call routed to this provider.
    async fn handle_tool_call(&self, name: &str, args: &JsonValue) -> Result<String>;
}

/// A memory provider backed by files in `~/.hakimi/memory/`.
///
/// Each file in the directory is treated as a separate memory entry.
/// Files are read into the system prompt and searched during prefetch.
pub struct FileMemoryProvider {
    memory_dir: std::path::PathBuf,
}

impl FileMemoryProvider {
    /// Create a new file-backed memory provider.
    ///
    /// `memory_dir` is the exact directory containing the memory files.
    pub fn new(memory_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            memory_dir: memory_dir.into(),
        }
    }

    /// Finalize the current session by archiving working memory and clearing it.
    ///
    /// This method:
    /// 1. Reads `working_memory.md`
    /// 2. If non-empty, appends content to `memory.md` with a timestamp
    /// 3. Clears `working_memory.md`
    /// 4. Logs the operation
    ///
    /// This should be called when a session ends (e.g., on `/new` command).
    pub fn finalize_session(&self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let working_path = self.memory_dir.join("working_memory.md");
        let memory_path = self.memory_dir.join("memory.md");

        // 1. Read working memory
        let working_content = match std::fs::read_to_string(&working_path) {
            Ok(c) => c.trim().to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };

        // 2. If non-empty, archive to memory.md
        if !working_content.is_empty() {
            let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
            let archive_section = format!(
                "\n\n---\n[Session ended: {}]\n{}",
                timestamp, working_content
            );

            let mut memory_content = std::fs::read_to_string(&memory_path).unwrap_or_default();
            memory_content.push_str(&archive_section);
            std::fs::write(&memory_path, memory_content)?;

            info!(
                chars = working_content.chars().count(),
                "Archived working memory to memory.md"
            );
        }

        // 3. Clear working_memory.md
        std::fs::write(&working_path, "")?;

        Ok(())
    }
}

#[async_trait]
impl MemoryProvider for FileMemoryProvider {
    fn name(&self) -> &str {
        "file-memory"
    }

    fn is_available(&self) -> bool {
        self.memory_dir.exists() && self.memory_dir.is_dir()
    }

    #[instrument(skip(self), fields(provider = "file-memory"))]
    fn system_prompt_block(&self) -> String {
        debug!("Loading memory files into system prompt");
        if !self.is_available() {
            debug!("Memory directory not available");
            return String::new();
        }

        let mut blocks = Vec::new();
        let entries = match std::fs::read_dir(&self.memory_dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Failed to read memory directory");
                return String::new();
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            let title = match name.to_lowercase().as_str() {
                "user" => "USER PROFILE (who the user is)",
                "memory" => "MEMORY (your personal notes)",
                "working_memory" | "working" => "WORKING MEMORY (current session)",
                _ => name,
            };

            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let content = content.trim();
                    if content.is_empty() {
                        continue;
                    }
                    let chars = content.chars().count();
                    blocks.push(format!(
                        "══════════════════════════════════════════════\n\
                        {title} [{chars} chars]\n\
                        ══════════════════════════════════════════════\n\
                        {content}"
                    ));
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to read memory file");
                }
            }
        }

        if blocks.is_empty() {
            debug!("No memory files loaded");
            String::new()
        } else {
            debug!(
                files_loaded = blocks.len(),
                "Memory files loaded successfully"
            );
            blocks.join("\n\n")
        }
    }

    #[instrument(skip(self), fields(provider = "file-memory", query))]
    async fn prefetch(&self, query: &str) -> String {
        debug!("Starting memory prefetch");
        if !self.is_available() {
            debug!("Memory directory not available");
            return String::new();
        }

        // Simple keyword-matching prefetch: return all memory entries whose
        // filename or content contains any word from the query.
        let query_lower = query.to_lowercase();
        let words: Vec<&str> = query_lower.split_whitespace().collect();

        let entries = match std::fs::read_dir(&self.memory_dir) {
            Ok(e) => e,
            Err(_) => return String::new(),
        };

        let mut matches = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let content_lower = content.to_lowercase();
                    let matched = words
                        .iter()
                        .any(|w| !w.is_empty() && (name.contains(w) || content_lower.contains(w)));
                    if matched {
                        matches.push(format!(
                            "[{}]\n{}",
                            path.file_stem().and_then(|n| n.to_str()).unwrap_or("?"),
                            content
                        ));
                    }
                }
                Err(_) => continue,
            }
        }

        debug!(query = query, matches = matches.len(), "Memory prefetch");
        matches.join("\n\n")
    }

    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "memory_save".to_string(),
                description: "Save a piece of information to long-term memory. The memory is stored as a file in ~/.hakimi/memory/.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "A short name / identifier for this memory entry"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to remember"
                        }
                    },
                    "required": ["name", "content"]
                }),
                toolset: "memory".to_string(),
            },
            ToolDefinition {
                name: "memory_search".to_string(),
                description: "Search long-term memory for entries matching a query.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
                toolset: "memory".to_string(),
            },
            ToolDefinition {
                name: "memory_list".to_string(),
                description: "List all long-term memory entries.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                toolset: "memory".to_string(),
            },
        ]
    }

    #[instrument(skip(self, args), fields(provider = "file-memory", tool = name))]
    async fn handle_tool_call(&self, name: &str, args: &JsonValue) -> Result<String> {
        debug!("Executing memory tool call");
        match name {
            "memory_save" => {
                let entry_name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    hakimi_common::HakimiError::ToolSimple("missing 'name' argument".into())
                })?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        hakimi_common::HakimiError::ToolSimple("missing 'content' argument".into())
                    })?;

                debug!(
                    entry_name,
                    content_len = content.len(),
                    "Saving memory entry"
                );

                // Ensure memory directory exists
                if !self.memory_dir.exists() {
                    std::fs::create_dir_all(&self.memory_dir).map_err(|e| {
                        hakimi_common::HakimiError::ToolSimple(format!(
                            "failed to create memory dir: {e}"
                        ))
                    })?;
                }

                // Sanitize filename
                let safe_name: String = entry_name
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                let path = self.memory_dir.join(format!("{safe_name}.md"));
                std::fs::write(&path, content).map_err(|e| {
                    hakimi_common::HakimiError::ToolSimple(format!("failed to write memory: {e}"))
                })?;

                debug!(path = %path.display(), "Saved memory entry");
                Ok(format!("Saved memory entry '{}'", entry_name))
            }
            "memory_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                    hakimi_common::HakimiError::ToolSimple("missing 'query' argument".into())
                })?;
                debug!(query, "Searching memory");
                let result = self.prefetch(query).await;
                if result.is_empty() {
                    debug!("No memory matches found");
                    Ok("No matching memory entries found.".to_string())
                } else {
                    debug!(result_len = result.len(), "Memory search completed");
                    Ok(result)
                }
            }
            "memory_list" => {
                debug!("Listing memory entries");
                if !self.is_available() {
                    return Ok("No memory directory found.".to_string());
                }
                let entries = match std::fs::read_dir(&self.memory_dir) {
                    Ok(e) => e,
                    Err(e) => {
                        return Ok(format!("Error reading memory directory: {e}"));
                    }
                };
                let names: Vec<String> = entries
                    .flatten()
                    .filter(|e| e.path().is_file())
                    .filter_map(|e| {
                        e.path()
                            .file_stem()
                            .and_then(|n| n.to_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                if names.is_empty() {
                    debug!("No memory entries found");
                    Ok("No memory entries.".to_string())
                } else {
                    debug!(entries_count = names.len(), "Memory list completed");
                    Ok(format!("Memory entries:\n{}", names.join("\n")))
                }
            }
            other => Err(hakimi_common::HakimiError::ToolSimple(format!(
                "Unknown memory tool: {other}"
            ))),
        }
    }
}

/// A memory provider backed by a user profile file at `~/.hakimi/user_profile`.
///
/// This is a single-file memory that stores user preferences, identity, etc.
pub struct UserMemoryProvider {
    profile_path: std::path::PathBuf,
}

impl UserMemoryProvider {
    /// Create a new user-profile memory provider.
    pub fn new(home: &str) -> Self {
        Self {
            profile_path: std::path::Path::new(home)
                .join(".hakimi")
                .join("user_profile"),
        }
    }
}

#[async_trait]
impl MemoryProvider for UserMemoryProvider {
    fn name(&self) -> &str {
        "user-profile"
    }

    fn is_available(&self) -> bool {
        self.profile_path.exists() && self.profile_path.is_file()
    }

    fn system_prompt_block(&self) -> String {
        if !self.is_available() {
            return String::new();
        }

        match std::fs::read_to_string(&self.profile_path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    String::new()
                } else {
                    format!("User profile:\n{content}")
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to read user profile");
                String::new()
            }
        }
    }

    async fn prefetch(&self, _query: &str) -> String {
        // User profile is fully included in the system prompt,
        // so prefetch just returns the full content.
        self.system_prompt_block()
    }

    fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "update_user_profile".to_string(),
            description: "Update the user profile stored at ~/.hakimi/user_profile. \
                          Use this to remember user preferences, identity details, etc."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The full content of the user profile to save"
                    }
                },
                "required": ["content"]
            }),
            toolset: "memory".to_string(),
        }]
    }

    async fn handle_tool_call(&self, name: &str, args: &JsonValue) -> Result<String> {
        match name {
            "update_user_profile" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        hakimi_common::HakimiError::ToolSimple("missing 'content' argument".into())
                    })?;

                if let Some(parent) = self.profile_path.parent()
                    && !parent.exists()
                {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        hakimi_common::HakimiError::ToolSimple(format!(
                            "failed to create profile dir: {e}"
                        ))
                    })?;
                }

                std::fs::write(&self.profile_path, content).map_err(|e| {
                    hakimi_common::HakimiError::ToolSimple(format!("failed to write profile: {e}"))
                })?;

                debug!(path = %self.profile_path.display(), "Updated user profile");
                Ok("User profile updated.".to_string())
            }
            other => Err(hakimi_common::HakimiError::ToolSimple(format!(
                "Unknown user profile tool: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_finalize_session_empty_working_memory() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FileMemoryProvider::new(temp_dir.path());

        // Working memory doesn't exist initially
        let result = provider.finalize_session();
        assert!(result.is_ok(), "finalize_session should succeed");

        // Working memory file should be created but empty
        let working_path = temp_dir.path().join("working_memory.md");
        assert_eq!(
            std::fs::read_to_string(&working_path).unwrap(),
            "",
            "working_memory.md should be empty after finalization"
        );

        // Memory file should not be created if working memory was empty
        let memory_path = temp_dir.path().join("memory.md");
        assert!(
            !memory_path.exists() || std::fs::read_to_string(&memory_path).unwrap().is_empty(),
            "memory.md should not contain archived content if working memory was empty"
        );
    }

    #[test]
    fn test_finalize_session_with_content() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FileMemoryProvider::new(temp_dir.path());

        // Create working memory with content
        let working_path = temp_dir.path().join("working_memory.md");
        std::fs::write(&working_path, "Temporary note from session\nAnother line")
            .unwrap();

        // Finalize the session
        let result = provider.finalize_session();
        assert!(result.is_ok(), "finalize_session should succeed");

        // Working memory should now be empty
        assert_eq!(
            std::fs::read_to_string(&working_path).unwrap(),
            "",
            "working_memory.md should be cleared after finalization"
        );

        // Memory should contain archived content with timestamp
        let memory_path = temp_dir.path().join("memory.md");
        let memory_content = std::fs::read_to_string(&memory_path)
            .expect("memory.md should exist after archiving");

        assert!(
            memory_content.contains("Temporary note from session"),
            "memory.md should contain archived working memory content"
        );
        assert!(
            memory_content.contains("Another line"),
            "memory.md should contain all archived content"
        );
        assert!(
            memory_content.contains("[Session ended:"),
            "memory.md should contain session end timestamp"
        );
        assert!(
            memory_content.contains("---"),
            "memory.md should contain separator"
        );
    }

    #[test]
    fn test_finalize_session_multiple_times() {
        let temp_dir = TempDir::new().unwrap();
        let provider = FileMemoryProvider::new(temp_dir.path());
        let working_path = temp_dir.path().join("working_memory.md");
        let memory_path = temp_dir.path().join("memory.md");

        // First session
        std::fs::write(&working_path, "Session 1 notes").unwrap();
        provider.finalize_session().unwrap();

        // Second session
        std::fs::write(&working_path, "Session 2 notes").unwrap();
        provider.finalize_session().unwrap();

        // Memory should contain both archived sessions
        let memory_content = std::fs::read_to_string(&memory_path).unwrap();
        assert!(
            memory_content.contains("Session 1 notes"),
            "memory.md should contain first session"
        );
        assert!(
            memory_content.contains("Session 2 notes"),
            "memory.md should contain second session"
        );

        // Should have two session end markers
        let session_end_count = memory_content.matches("[Session ended:").count();
        assert_eq!(
            session_end_count, 2,
            "memory.md should have two session end markers"
        );

        // Working memory should still be empty
        assert_eq!(
            std::fs::read_to_string(&working_path).unwrap(),
            "",
            "working_memory.md should be empty after second finalization"
        );
    }
}
