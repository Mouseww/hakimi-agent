use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, redact_sensitive_text};
use serde_json::{Value as JsonValue, json};
use std::borrow::Cow;
use tokio::process::Command;
use tracing::debug;

use crate::Tool;

/// Default timeout for code execution (seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Built-in tool for executing code snippets in various languages.
pub struct CodeExecTool;

#[async_trait]
impl Tool for CodeExecTool {
    fn name(&self) -> &str {
        "code_exec"
    }

    fn toolset(&self) -> &str {
        "code"
    }

    fn description(&self) -> &str {
        "Execute a code snippet in Python, JavaScript (Node.js), or Bash. Writes the code to a temp file and runs it with the appropriate interpreter. Returns stdout, stderr, and exit code."
    }

    fn emoji(&self) -> &str {
        "\u{1f40d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "description": "Programming language for the code snippet. Defaults to 'python'.",
                    "enum": ["python", "javascript", "bash"]
                },
                "code": {
                    "type": "string",
                    "description": "The code snippet to execute."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Maximum time in seconds to wait for execution. Defaults to 300.",
                    "minimum": 1,
                    "maximum": 600
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let code = args
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: code".into()))?;

        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("python");

        let timeout_secs = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(600);

        debug!(language = %language, timeout = timeout_secs, "executing code snippet");

        // Determine interpreter and file extension
        let (interpreter, extension) = match language {
            "python" => (resolve_python_interpreter(), ".py"),
            "javascript" => (Cow::Borrowed("node"), ".js"),
            "bash" => (resolve_bash_interpreter(), ".sh"),
            _ => {
                return Err(HakimiError::ToolSimple(format!(
                    "unsupported language '{}'. Supported: python, javascript, bash.",
                    language
                )));
            }
        };

        // Write code to a temp file
        let temp_dir = std::path::PathBuf::from(&ctx.workdir).join(".hakimi_tmp");
        tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| {
            HakimiError::ToolSimple(format!("failed to create temp directory: {e}"))
        })?;

        let temp_file = temp_dir.join(format!(
            "snippet_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            extension
        ));
        tokio::fs::write(&temp_file, code)
            .await
            .map_err(|e| HakimiError::ToolSimple(format!("failed to write temp file: {e}")))?;

        if language == "python" {
            let hermes_tools_py = r#"
import os, subprocess, json, re, shlex
from collections import defaultdict
import time

def read_file(path, offset=1, limit=500):
    try:
        with open(path, "r", encoding="utf-8") as f:
            lines = f.readlines()
        total_lines = len(lines)
        start = max(0, offset - 1)
        end = min(total_lines, start + limit)
        content = "".join(lines[start:end])
        return {"content": content, "total_lines": total_lines, "offset": offset, "limit": limit}
    except Exception as e:
        return {"error": str(e)}

def write_file(path, content):
    try:
        os.makedirs(os.path.dirname(os.path.abspath(path)) or ".", exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(content)
        return {"success": True, "path": path}
    except Exception as e:
        return {"error": str(e)}

def search_files(pattern, target="content", path=".", file_glob=None, limit=50):
    cmd = ["rg", "--json", "--max-count", str(limit)]
    if target == "files":
        cmd = ["rg", "--json", "--files"]
    if file_glob:
        cmd.extend(["-g", file_glob])
    cmd.extend(["--", pattern, path])
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, check=False)
        matches = []
        for line in res.stdout.strip().split("\n"):
            if not line: continue
            try:
                data = json.loads(line)
                if data["type"] == "match":
                    matches.append({"path": data["data"]["path"]["text"], "line": data["data"]["line_number"], "content": data["data"]["lines"]["text"]})
            except: pass
        return {"matches": matches}
    except Exception as e:
        return {"error": str(e)}

def patch(path, old_string, new_string, replace_all=False):
    try:
        with open(path, "r", encoding="utf-8") as f:
            content = f.read()
        if not replace_all and content.count(old_string) != 1:
            return {"error": "old_string is not unique"}
        new_content = content.replace(old_string, new_string) if replace_all else content.replace(old_string, new_string, 1)
        with open(path, "w", encoding="utf-8") as f:
            f.write(new_content)
        return {"success": True}
    except Exception as e:
        return {"error": str(e)}

def terminal(command, timeout=180, workdir=None):
    try:
        res = subprocess.run(command, shell=True, capture_output=True, text=True, timeout=timeout, cwd=workdir)
        return {"output": res.stdout + res.stderr, "exit_code": res.returncode}
    except subprocess.TimeoutExpired as e:
        return {"error": "timeout", "output": e.stdout.decode('utf-8') if e.stdout else ''}
    except Exception as e:
        return {"error": str(e)}

def json_parse(text):
    return json.loads(text, strict=False)

def shell_quote(s):
    return shlex.quote(s)

def retry(fn, max_attempts=3, delay=2):
    for i in range(max_attempts):
        try:
            return fn()
        except Exception as e:
            if i == max_attempts - 1: raise e
            time.sleep(delay)
"#;
            let ht_path = temp_dir.join("hermes_tools.py");
            let _ = tokio::fs::write(&ht_path, hermes_tools_py).await;
        }

        // Execute the code
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            Command::new(interpreter.as_ref())
                .env("PYTHONPATH", temp_dir.to_str().unwrap_or_default())
                .arg(&temp_file)
                .current_dir(&ctx.workdir)
                .output(),
        )
        .await;

        // Clean up temp file
        let _ = tokio::fs::remove_file(&temp_file).await;

        let output = result
            .map_err(|_| {
                HakimiError::ToolSimple(format!(
                    "code execution timed out after {}s ({})",
                    timeout_secs, language
                ))
            })?
            .map_err(|e| {
                HakimiError::ToolSimple(format!("failed to execute {} code: {e}", language))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        result.push_str(&format!("Language: {}\n", language));

        if !stdout.is_empty() {
            result.push_str("STDOUT:\n");
            result.push_str(&redact_sensitive_text(&stdout));
        }

        if !stderr.is_empty() {
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("STDERR:\n");
            result.push_str(&redact_sensitive_text(&stderr));
        }

        if let Some(code) = output.status.code() {
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&format!("EXIT CODE: {}", code));
        } else {
            result.push_str("\nEXIT CODE: (terminated by signal)");
        }

        if result.trim().is_empty() || result == format!("Language: {}", language) {
            result = format!("Language: {}\n(no output)", language);
        }

        Ok(result)
    }
}

fn resolve_python_interpreter() -> Cow<'static, str> {
    #[cfg(windows)]
    {
        for candidate in ["python", "python3"] {
            if command_exists(candidate) {
                return Cow::Borrowed(candidate);
            }
        }
    }
    Cow::Borrowed("python3")
}

fn resolve_bash_interpreter() -> Cow<'static, str> {
    #[cfg(windows)]
    {
        for candidate in [
            "C:/Program Files/Git/bin/bash.exe",
            "C:/msys64/usr/bin/bash.exe",
            "C:/msys64/mingw64/bin/bash.exe",
        ] {
            if std::path::Path::new(candidate).exists() {
                return Cow::Owned(candidate.to_string());
            }
        }
    }
    Cow::Borrowed("bash")
}

#[cfg(windows)]
fn command_exists(command: &str) -> bool {
    std::process::Command::new("where")
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;
    use tempfile::TempDir;

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

    fn make_test_ctx() -> (TempDir, ToolContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx(&dir.path().to_string_lossy());
        (dir, ctx)
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = CodeExecTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["code"].is_object());
        assert!(schema["properties"]["language"].is_object());
        assert!(schema["properties"]["timeout"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "code"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = CodeExecTool;
        assert_eq!(tool.name(), "code_exec");
        assert_eq!(tool.toolset(), "code");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "🐍");
    }

    #[tokio::test]
    async fn test_execute_python_code() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({
            "code": "print('hello from python')",
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("python"));
        assert!(result.contains("hello from python"));
        assert!(result.contains("EXIT CODE: 0"));
    }

    #[tokio::test]
    async fn test_execute_bash_code() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({
            "code": "echo 'hello from bash'",
            "language": "bash"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("bash"));
        assert!(result.contains("hello from bash"));
        assert!(result.contains("EXIT CODE: 0"));
    }

    #[tokio::test]
    async fn test_execute_python_default_language() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({
            "code": "print(2 + 2)"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("python"));
        assert!(result.contains("4"));
    }

    #[tokio::test]
    async fn test_execute_with_stderr() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({
            "code": "import sys; sys.stderr.write('error output\\n')",
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("STDERR"));
        assert!(result.contains("error output"));
    }

    #[tokio::test]
    async fn test_execute_redacts_secret_output() {
        let (_dir, ctx) = make_test_ctx();
        let token = format!("{}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz1234567890");
        let args = json!({
            "code": format!("print('Authorization: Bearer {token}')"),
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(!result.contains(&token));
        assert!(result.contains("Authorization: Bearer"));
    }

    #[tokio::test]
    async fn test_execute_nonzero_exit_code() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({
            "code": "import sys; sys.exit(42)",
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("EXIT CODE: 42"));
    }

    #[tokio::test]
    async fn test_unsupported_language_error() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({
            "code": "print('hello')",
            "language": "haskell"
        });
        let err = CodeExecTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("unsupported language"));
    }

    #[tokio::test]
    async fn test_missing_code_error() {
        let (_dir, ctx) = make_test_ctx();
        let args = json!({"language": "python"});
        let err = CodeExecTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("code"));
    }

    #[tokio::test]
    async fn test_temp_file_cleanup() {
        let dir = std::env::temp_dir().join("hakimi_test_code_exec_cleanup");
        let _ = tokio::fs::create_dir_all(&dir).await;

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "code": "print('temp file test')",
            "language": "python"
        });

        let _ = CodeExecTool.execute(&args, &ctx).await.unwrap();

        // Temp file should be cleaned up
        let temp_dir = dir.join(".hakimi_tmp");
        if temp_dir.exists() {
            let mut entries = tokio::fs::read_dir(&temp_dir).await.unwrap();
            let mut count = 0;
            while let Some(entry) = entries.next_entry().await.unwrap() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.starts_with("snippet_") {
                    count += 1;
                }
            }
            assert_eq!(count, 0, "temp file should have been cleaned up");
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
