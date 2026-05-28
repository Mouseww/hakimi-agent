use std::collections::HashMap;
use std::sync::LazyLock;

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::debug;

use crate::Tool;
use crate::shell_env::{apply_stable_path, bash_program};

/// Built-in tool for managing background processes.
pub struct ProcessTool;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub session_id: String,
    pub command: String,
    #[serde(skip)]
    pub child: Option<Child>,
}

// Global state for background processes
pub static PROCESSES: LazyLock<Mutex<HashMap<String, ProcessInfo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &str {
        "process"
    }

    fn toolset(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Manage background processes. Actions: 'start' spawns a background command and returns a session_id, 'status' checks if a process is running, 'log' retrieves stdout/stderr, 'kill' terminates a process, 'list' shows all running background processes."
    }

    fn emoji(&self) -> &str {
        "\u{2699}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform on background processes.",
                    "enum": ["start", "status", "log", "kill", "list"]
                },
                "command": {
                    "type": "string",
                    "description": "The shell command to start in the background. Required for 'start' action."
                },
                "session_id": {
                    "type": "string",
                    "description": "The session ID of a background process. Required for 'status', 'log', and 'kill' actions."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: action".into()))?;

        debug!(action = %action, "process operation");

        match action {
            "start" => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        HakimiError::Tool("'command' is required for 'start' action".into())
                    })?;

                let mut spawn_command = Command::new(bash_program());
                apply_stable_path(&mut spawn_command);
                let child = spawn_command
                    .arg("-c")
                    .arg(command)
                    .current_dir(&ctx.workdir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| {
                        HakimiError::Tool(format!("failed to spawn background process: {e}"))
                    })?;

                // Generate a unique session id
                let proc_session_id = format!("{}-{}", ctx.session_id, uuid::Uuid::new_v4());

                let info = ProcessInfo {
                    session_id: proc_session_id.clone(),
                    command: command.to_string(),
                    child: Some(child),
                };

                let mut processes = PROCESSES.lock().await;
                processes.insert(proc_session_id.clone(), info);

                Ok(format!(
                    "Background process started.\nSession ID: {}\nCommand: {}",
                    proc_session_id, command
                ))
            }
            "status" => {
                let session_id =
                    args.get("session_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            HakimiError::Tool("'session_id' is required for 'status' action".into())
                        })?;

                let mut processes = PROCESSES.lock().await;
                let info = processes.get_mut(session_id).ok_or_else(|| {
                    HakimiError::Tool(format!(
                        "no background process found with session_id '{}'",
                        session_id
                    ))
                })?;

                let status = if let Some(child) = &mut info.child {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            format!("exited with code {}", status.code().unwrap_or(-1))
                        }
                        Ok(None) => "running".to_string(),
                        Err(e) => format!("error checking status: {e}"),
                    }
                } else {
                    "no child handle (already collected)".to_string()
                };

                Ok(format!("Process '{}': {}", session_id, status))
            }
            "log" => {
                let session_id =
                    args.get("session_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            HakimiError::Tool("'session_id' is required for 'log' action".into())
                        })?;

                let mut processes = PROCESSES.lock().await;
                let info = processes.get_mut(session_id).ok_or_else(|| {
                    HakimiError::Tool(format!(
                        "no background process found with session_id '{}'",
                        session_id
                    ))
                })?;

                let mut output = String::new();

                if let Some(child) = &mut info.child {
                    // Read stdout
                    if let Some(stdout) = child.stdout.as_mut() {
                        let mut buf = Vec::new();
                        let _ = stdout.read_to_end(&mut buf).await;
                        let text = String::from_utf8_lossy(&buf);
                        if !text.is_empty() {
                            output.push_str("STDOUT:\n");
                            output.push_str(&text);
                            output.push('\n');
                        }
                    }

                    // Read stderr
                    if let Some(stderr) = child.stderr.as_mut() {
                        let mut buf = Vec::new();
                        let _ = stderr.read_to_end(&mut buf).await;
                        let text = String::from_utf8_lossy(&buf);
                        if !text.is_empty() {
                            output.push_str("STDERR:\n");
                            output.push_str(&text);
                            output.push('\n');
                        }
                    }

                    // Check if still running
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            output
                                .push_str(&format!("\nEXIT CODE: {}", status.code().unwrap_or(-1)));
                        }
                        Ok(None) => {
                            output.push_str("\nStatus: still running");
                        }
                        Err(e) => {
                            output.push_str(&format!("\nError checking status: {e}"));
                        }
                    }
                } else {
                    output = "No child handle available (already collected).".to_string();
                }

                if output.trim().is_empty() {
                    output = "(no output yet)".to_string();
                }

                Ok(output)
            }
            "kill" => {
                let session_id =
                    args.get("session_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            HakimiError::Tool("'session_id' is required for 'kill' action".into())
                        })?;

                let mut processes = PROCESSES.lock().await;
                let mut info = processes.remove(session_id).ok_or_else(|| {
                    HakimiError::Tool(format!(
                        "no background process found with session_id '{}'",
                        session_id
                    ))
                })?;

                if let Some(child) = &mut info.child {
                    child
                        .kill()
                        .await
                        .map_err(|e| HakimiError::Tool(format!("failed to kill process: {e}")))?;
                    Ok(format!("Process '{}' killed.", session_id))
                } else {
                    Ok(format!(
                        "Process '{}' removed (no child handle to kill).",
                        session_id
                    ))
                }
            }
            "list" => {
                let mut processes = PROCESSES.lock().await;
                if processes.is_empty() {
                    return Ok("No background processes running.".to_string());
                }

                let mut result = String::new();
                let mut to_remove = Vec::new();

                for (id, info) in processes.iter_mut() {
                    let status = if let Some(child) = &mut info.child {
                        match child.try_wait() {
                            Ok(Some(s)) => {
                                let code = s.code().unwrap_or(-1);
                                to_remove.push(id.clone());
                                format!("exited (code {})", code)
                            }
                            Ok(None) => "running".to_string(),
                            Err(e) => format!("error: {e}"),
                        }
                    } else {
                        "unknown".to_string()
                    };

                    result.push_str(&format!("[{}] {} - {}\n", id, info.command, status));
                }

                // Clean up exited processes
                for id in to_remove {
                    processes.remove(&id);
                }

                Ok(result)
            }
            _ => Err(HakimiError::Tool(format!(
                "invalid action '{}'. Must be 'start', 'status', 'log', 'kill', or 'list'.",
                action
            ))),
        }
    }
}
