//! Plugin system for extending Hakimi Agent with user-defined tools.
//!
//! Plugins are defined by a YAML manifest file and an executable command.
//! When a plugin tool is invoked, the command is executed with the tool
//! arguments serialized as JSON on stdin. The command should write its
//! result as a JSON object to stdout:
//!
//! ```json
//! {"result": "tool output text"}
//! ```
//!
//! Or for errors:
//!
//! ```json
//! {"error": "something went wrong"}
//! ```
//!
//! Plugins are discovered from `~/.hakimi/plugins/` (each subdirectory
//! must contain a `manifest.yaml` file).

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, redact_sensitive_text};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::Tool;

// ---------------------------------------------------------------------------
// Plugin manifest
// ---------------------------------------------------------------------------

/// A plugin manifest loaded from a `manifest.yaml` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique tool name (used for dispatch).
    pub name: String,

    /// Toolset / category.
    #[serde(default = "default_toolset")]
    pub toolset: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// Emoji icon.
    #[serde(default = "default_emoji")]
    pub emoji: String,

    /// Path to the executable command.
    /// Can be absolute or relative to the plugin directory.
    pub command: String,

    /// Default timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// JSON Schema describing the tool's input parameters.
    #[serde(default = "default_schema")]
    pub schema: JsonValue,

    /// Optional environment variables to set when running the plugin.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

fn default_toolset() -> String {
    "plugin".to_string()
}

fn default_emoji() -> String {
    "🧩".to_string()
}

fn default_timeout() -> u64 {
    60
}

fn default_schema() -> JsonValue {
    json!({"type": "object", "properties": {}})
}

// ---------------------------------------------------------------------------
// CommandPluginTool
// ---------------------------------------------------------------------------

/// A tool backed by an external command (loaded from a plugin manifest).
pub struct CommandPluginTool {
    manifest: PluginManifest,
    /// Absolute path to the command (resolved during loading).
    command_path: String,
}

impl CommandPluginTool {
    /// Create a new plugin tool from a manifest and resolved command path.
    pub fn new(manifest: PluginManifest, command_path: String) -> Self {
        Self {
            manifest,
            command_path,
        }
    }
}

/// Expected JSON response from a plugin command.
#[derive(Debug, Deserialize)]
struct PluginResponse {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[async_trait]
impl Tool for CommandPluginTool {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn toolset(&self) -> &str {
        &self.manifest.toolset
    }

    fn description(&self) -> &str {
        &self.manifest.description
    }

    fn emoji(&self) -> &str {
        &self.manifest.emoji
    }

    fn schema(&self) -> JsonValue {
        self.manifest.schema.clone()
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(256 * 1024)
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        debug!(
            plugin = %self.manifest.name,
            command = %self.command_path,
            "executing plugin tool"
        );

        let args_json = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());

        let mut cmd = Command::new(&self.command_path);
        cmd.current_dir(&ctx.workdir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Set plugin-specific environment variables.
        for (key, val) in &self.manifest.env {
            cmd.env(key, val);
        }

        // Pass context as environment variables.
        cmd.env("HAKIMI_WORKDIR", &ctx.workdir);
        cmd.env("HAKIMI_SESSION_ID", &ctx.session_id);
        if let Some(ref user_id) = ctx.user_id {
            cmd.env("HAKIMI_USER_ID", user_id);
        }

        let timeout = std::time::Duration::from_secs(self.manifest.timeout);

        let mut child = cmd.spawn().map_err(|e| {
            HakimiError::ToolSimple(format!(
                "failed to spawn plugin '{}': {}",
                self.manifest.name, e
            ))
        })?;

        // Write arguments to stdin.
        if let Some(ref mut stdin) = child.stdin {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(args_json.as_bytes())
                .await
                .map_err(|e| HakimiError::ToolSimple(format!("failed to write to plugin stdin: {e}")))?;
            // Close stdin to signal end of input.
            drop(child.stdin.take());
        }

        // Wait for the command with timeout.
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                HakimiError::ToolSimple(format!(
                    "plugin '{}' timed out after {}s",
                    self.manifest.name, self.manifest.timeout
                ))
            })?
            .map_err(|e| {
                HakimiError::ToolSimple(format!(
                    "plugin '{}' execution failed: {e}",
                    self.manifest.name
                ))
            })?;

        if !output.status.success() {
            let stderr = redact_sensitive_text(&String::from_utf8_lossy(&output.stderr));
            let code = output.status.code().unwrap_or(-1);
            return Err(HakimiError::ToolSimple(format!(
                "plugin '{}' exited with code {}: {}",
                self.manifest.name, code, stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();

        // Try to parse as structured JSON response.
        if let Ok(resp) = serde_json::from_str::<PluginResponse>(stdout) {
            if let Some(error) = resp.error {
                return Err(HakimiError::ToolSimple(format!(
                    "plugin '{}' returned error: {}",
                    self.manifest.name,
                    redact_sensitive_text(&error)
                )));
            }
            return Ok(redact_sensitive_text(&resp.result.unwrap_or_default()));
        }

        // If not valid JSON, return the raw stdout as the result.
        Ok(redact_sensitive_text(stdout))
    }
}

// ---------------------------------------------------------------------------
// PluginManager
// ---------------------------------------------------------------------------

/// Manages discovery and loading of plugins from the filesystem.
pub struct PluginManager {
    plugin_dir: std::path::PathBuf,
}

impl PluginManager {
    /// Create a new plugin manager that scans the given directory.
    pub fn new(plugin_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            plugin_dir: plugin_dir.into(),
        }
    }

    /// Create a plugin manager using the default location (`~/.hakimi/plugins/`).
    pub fn default_location() -> Self {
        let plugin_dir = dirs::home_dir()
            .map(|h| h.join(".hakimi").join("plugins"))
            .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/plugins"));
        Self::new(plugin_dir)
    }

    /// Discover and load all plugins from the plugin directory.
    ///
    /// Each plugin is a subdirectory containing a `manifest.yaml` file.
    /// Returns a list of loaded `CommandPluginTool`s.
    pub async fn discover(&self) -> Vec<CommandPluginTool> {
        let mut tools = Vec::new();

        if !self.plugin_dir.exists() {
            debug!(path = %self.plugin_dir.display(), "plugin directory does not exist, skipping");
            return tools;
        }

        let entries = match tokio::fs::read_dir(&self.plugin_dir).await {
            Ok(e) => e,
            Err(e) => {
                warn!(path = %self.plugin_dir.display(), error = %e, "failed to read plugin directory");
                return tools;
            }
        };

        let mut entries = entries;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("manifest.yaml");
            if !manifest_path.exists() {
                // Also check for manifest.yml
                let alt_path = path.join("manifest.yml");
                if !alt_path.exists() {
                    continue;
                }
                match self.load_plugin(&alt_path, &path).await {
                    Ok(tool) => tools.push(tool),
                    Err(e) => warn!(path = %path.display(), error = %e, "failed to load plugin"),
                }
            } else {
                match self.load_plugin(&manifest_path, &path).await {
                    Ok(tool) => tools.push(tool),
                    Err(e) => warn!(path = %path.display(), error = %e, "failed to load plugin"),
                }
            }
        }

        info!(count = tools.len(), "plugins discovered");
        tools
    }

    /// Load a single plugin from its manifest file.
    async fn load_plugin(
        &self,
        manifest_path: &std::path::Path,
        plugin_dir: &std::path::Path,
    ) -> std::result::Result<CommandPluginTool, String> {
        let contents = tokio::fs::read_to_string(manifest_path)
            .await
            .map_err(|e| format!("failed to read manifest: {e}"))?;

        let manifest: PluginManifest = serde_yaml::from_str(&contents)
            .map_err(|e| format!("failed to parse manifest: {e}"))?;

        // Resolve command path.
        let command_path = if manifest.command.starts_with('/') {
            // Absolute path.
            manifest.command.clone()
        } else {
            // Relative to plugin directory.
            plugin_dir
                .join(&manifest.command)
                .to_string_lossy()
                .to_string()
        };

        // Verify the command exists.
        if !std::path::Path::new(&command_path).exists() {
            return Err(format!("plugin command does not exist: {}", command_path));
        }

        info!(
            name = %manifest.name,
            command = %command_path,
            "loaded plugin"
        );

        Ok(CommandPluginTool::new(manifest, command_path))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_plugin_manifest_deserialization() {
        let yaml = r#"
name: my_tool
toolset: custom
description: A custom tool
emoji: "🔧"
command: /usr/bin/echo
timeout: 30
schema:
  type: object
  properties:
    input:
      type: string
  required: [input]
"#;
        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "my_tool");
        assert_eq!(manifest.toolset, "custom");
        assert_eq!(manifest.command, "/usr/bin/echo");
        assert_eq!(manifest.timeout, 30);
        assert_eq!(manifest.emoji, "🔧");
    }

    #[test]
    fn test_plugin_manifest_defaults() {
        let yaml = r#"
name: minimal_tool
command: /bin/echo
"#;
        let manifest: PluginManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "minimal_tool");
        assert_eq!(manifest.toolset, "plugin");
        assert_eq!(manifest.emoji, "🧩");
        assert_eq!(manifest.timeout, 60);
        assert_eq!(manifest.description, "");
    }

    #[test]
    fn test_command_plugin_tool_properties() {
        let manifest = PluginManifest {
            name: "test_tool".to_string(),
            toolset: "testing".to_string(),
            description: "A test tool".to_string(),
            emoji: "🧪".to_string(),
            command: "/bin/echo".to_string(),
            timeout: 10,
            schema: json!({"type": "object"}),
            env: Default::default(),
        };

        let tool = CommandPluginTool::new(manifest, "/bin/echo".to_string());
        assert_eq!(tool.name(), "test_tool");
        assert_eq!(tool.toolset(), "testing");
        assert_eq!(tool.description(), "A test tool");
        assert_eq!(tool.emoji(), "🧪");
    }

    #[tokio::test]
    async fn test_plugin_tool_execution() {
        // Create a simple plugin script.
        let dir = tempfile::tempdir().unwrap();
        let script_path = create_test_plugin_script(dir.path()).await;

        let manifest = PluginManifest {
            name: "echo_plugin".to_string(),
            toolset: "test".to_string(),
            description: "Echoes input".to_string(),
            emoji: "🔊".to_string(),
            command: script_path.to_string_lossy().to_string(),
            timeout: 5,
            schema: json!({"type": "object", "properties": {"msg": {"type": "string"}}}),
            env: Default::default(),
        };

        let tool = CommandPluginTool::new(manifest, script_path.to_string_lossy().to_string());
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: dir.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };

        let result = tool.execute(&json!({"msg": "hello"}), &ctx).await.unwrap();
        assert!(result.contains("echo:"));
    }

    #[cfg(unix)]
    async fn create_test_plugin_script(dir: &Path) -> PathBuf {
        let script_path = dir.join("echo-plugin.sh");
        tokio::fs::write(
            &script_path,
            r#"#!/bin/sh
read INPUT
printf '{"result":"echo: %s"}\n' "$INPUT"
"#,
        )
        .await
        .unwrap();

        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();
        script_path
    }

    #[cfg(windows)]
    async fn create_test_plugin_script(dir: &Path) -> PathBuf {
        let script_path = dir.join("echo-plugin.cmd");
        tokio::fs::write(
            &script_path,
            "@echo off\r\nset /p INPUT=\r\necho {\"result\":\"echo: %INPUT%\"}\r\n",
        )
        .await
        .unwrap();
        script_path
    }
}
