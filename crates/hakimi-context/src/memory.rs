use async_trait::async_trait;
use hakimi_common::{Result, ToolDefinition};
use serde_json::Value as JsonValue;
use tracing::{debug, warn};

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

/// A memory provider backed by files in `~/.hermes/memory/`.
///
/// Each file in the directory is treated as a separate memory entry.
/// Files are read into the system prompt and searched during prefetch.
pub struct FileMemoryProvider {
    memory_dir: std::path::PathBuf,
}

impl FileMemoryProvider {
    /// Create a new file-backed memory provider.
    ///
    /// `home` is the user's home directory (e.g. `/root`).
    pub fn new(home: &str) -> Self {
        Self {
            memory_dir: std::path::Path::new(home).join(".hermes").join("memory"),
        }
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

    fn system_prompt_block(&self) -> String {
        if !self.is_available() {
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
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    blocks.push(format!("[{name}]\n{content}"));
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to read memory file");
                }
            }
        }

        if blocks.is_empty() {
            String::new()
        } else {
            format!("Long-term memory:\n\n{}", blocks.join("\n\n"))
        }
    }

    async fn prefetch(&self, query: &str) -> String {
        if !self.is_available() {
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
                    let matched = words.iter().any(|w| {
                        !w.is_empty() && (name.contains(w) || content_lower.contains(w))
                    });
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
                description: "Save a piece of information to long-term memory. The memory is stored as a file in ~/.hermes/memory/.".to_string(),
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
            },
            ToolDefinition {
                name: "memory_list".to_string(),
                description: "List all long-term memory entries.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        ]
    }

    async fn handle_tool_call(&self, name: &str, args: &JsonValue) -> Result<String> {
        match name {
            "memory_save" => {
                let entry_name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        hakimi_common::HakimiError::Tool("missing 'name' argument".into())
                    })?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        hakimi_common::HakimiError::Tool("missing 'content' argument".into())
                    })?;

                // Ensure memory directory exists
                if !self.memory_dir.exists() {
                    std::fs::create_dir_all(&self.memory_dir).map_err(|e| {
                        hakimi_common::HakimiError::Tool(format!(
                            "failed to create memory dir: {e}"
                        ))
                    })?;
                }

                // Sanitize filename
                let safe_name: String = entry_name
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                    .collect();
                let path = self.memory_dir.join(format!("{safe_name}.md"));
                std::fs::write(&path, content).map_err(|e| {
                    hakimi_common::HakimiError::Tool(format!("failed to write memory: {e}"))
                })?;

                debug!(path = %path.display(), "Saved memory entry");
                Ok(format!("Saved memory entry '{}'", entry_name))
            }
            "memory_search" => {
                let query = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        hakimi_common::HakimiError::Tool("missing 'query' argument".into())
                    })?;
                let result = self.prefetch(query).await;
                if result.is_empty() {
                    Ok("No matching memory entries found.".to_string())
                } else {
                    Ok(result)
                }
            }
            "memory_list" => {
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
                    Ok("No memory entries.".to_string())
                } else {
                    Ok(format!("Memory entries:\n{}", names.join("\n")))
                }
            }
            other => Err(hakimi_common::HakimiError::Tool(format!(
                "Unknown memory tool: {other}"
            ))),
        }
    }
}

/// A memory provider backed by a user profile file at `~/.hermes/user_profile`.
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
                .join(".hermes")
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
            description: "Update the user profile stored at ~/.hermes/user_profile. \
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
        }]
    }

    async fn handle_tool_call(&self, name: &str, args: &JsonValue) -> Result<String> {
        match name {
            "update_user_profile" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        hakimi_common::HakimiError::Tool("missing 'content' argument".into())
                    })?;

                if let Some(parent) = self.profile_path.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            hakimi_common::HakimiError::Tool(format!(
                                "failed to create profile dir: {e}"
                            ))
                        })?;
                    }
                }

                std::fs::write(&self.profile_path, content).map_err(|e| {
                    hakimi_common::HakimiError::Tool(format!("failed to write profile: {e}"))
                })?;

                debug!(path = %self.profile_path.display(), "Updated user profile");
                Ok("User profile updated.".to_string())
            }
            other => Err(hakimi_common::HakimiError::Tool(format!(
                "Unknown user profile tool: {other}"
            ))),
        }
    }
}
