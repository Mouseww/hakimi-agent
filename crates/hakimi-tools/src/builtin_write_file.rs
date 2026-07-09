use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, get_write_block_error};
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool that writes content to a file, creating parent directories as needed.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if they don't exist. Overwrites the file if it already exists."
    }

    fn emoji(&self) -> &str {
        "\u{270f}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to write."
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: path".into()))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: content".into()))?;

        let requested_path = PathBuf::from(path);
        let full_path = if requested_path.is_absolute() {
            requested_path
        } else {
            PathBuf::from(&ctx.workdir).join(requested_path)
        };

        debug!(path = %full_path.display(), bytes = content.len(), "writing file");

        if let Some(error) = get_write_block_error(&full_path) {
            return Err(HakimiError::ToolSimple(error));
        }

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                debug!(dir = %parent.display(), error = %e, "failed to create parent directories");
                HakimiError::ToolSimple(format!(
                    "failed to create directories '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        fs::write(&full_path, content).await.map_err(|e| {
            debug!(path = %full_path.display(), error = %e, "failed to write file");
            HakimiError::ToolSimple(format!("failed to write '{}': {}", full_path.display(), e))
        })?;

        let line_count = content.lines().count();
        Ok(format!(
            "Successfully wrote {bytes} bytes ({lines} lines) to {path}",
            bytes = content.len(),
            lines = line_count,
            path = full_path.display(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;
    use std::ffi::OsString;
    use std::path::Path;
    use tokio::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    struct WriteSafeRootEnv {
        hakimi: Option<OsString>,
        hermes: Option<OsString>,
    }

    impl WriteSafeRootEnv {
        fn set(root: &Path) -> Self {
            let previous = Self {
                hakimi: std::env::var_os("HAKIMI_WRITE_SAFE_ROOT"),
                hermes: std::env::var_os("HERMES_WRITE_SAFE_ROOT"),
            };
            // SAFETY: tests serialize environment mutations with ENV_LOCK and restore them before release.
            unsafe {
                std::env::set_var("HAKIMI_WRITE_SAFE_ROOT", root.as_os_str());
                std::env::remove_var("HERMES_WRITE_SAFE_ROOT");
            }
            previous
        }
    }

    impl Drop for WriteSafeRootEnv {
        fn drop(&mut self) {
            // SAFETY: tests serialize environment mutations with ENV_LOCK and restore them before release.
            unsafe {
                match &self.hakimi {
                    Some(value) => std::env::set_var("HAKIMI_WRITE_SAFE_ROOT", value),
                    None => std::env::remove_var("HAKIMI_WRITE_SAFE_ROOT"),
                }
                match &self.hermes {
                    Some(value) => std::env::set_var("HERMES_WRITE_SAFE_ROOT", value),
                    None => std::env::remove_var("HERMES_WRITE_SAFE_ROOT"),
                }
            }
        }
    }

    fn test_ctx(workdir: &str) -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: workdir.to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_write_file_allows_write_inside_safe_root() {
        let _guard = ENV_LOCK.lock().await;
        let dir = std::env::temp_dir().join("hakimi_test_write_safe_root_inside");
        let safe_root = dir.join("workspace");
        let _env = WriteSafeRootEnv::set(&safe_root);
        let _ = fs::create_dir_all(&safe_root).await;

        let ctx = test_ctx(&safe_root.to_string_lossy());
        let args = json!({
            "path": "nested/file.txt",
            "content": "hello"
        });

        let result = WriteFileTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Successfully wrote"));
        assert_eq!(
            fs::read_to_string(safe_root.join("nested").join("file.txt"))
                .await
                .unwrap(),
            "hello"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_write_file_denies_write_outside_safe_root() {
        let _guard = ENV_LOCK.lock().await;
        let dir = std::env::temp_dir().join("hakimi_test_write_safe_root_outside");
        let safe_root = dir.join("workspace");
        let outside = dir.join("outside").join("file.txt");
        let _env = WriteSafeRootEnv::set(&safe_root);
        let _ = fs::create_dir_all(&safe_root).await;

        let ctx = test_ctx(&safe_root.to_string_lossy());
        let args = json!({
            "path": outside.to_string_lossy(),
            "content": "blocked"
        });

        let err = WriteFileTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("outside the configured write safe root"));
        assert!(!outside.exists());

        let _ = fs::remove_dir_all(&dir).await;
    }
}
