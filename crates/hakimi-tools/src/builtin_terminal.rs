use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tokio::process::Command;
use tracing::debug;

use crate::Tool;
use crate::shell_env::{apply_stable_path, bash_program, diagnose_shell_failure};

/// Default timeout for terminal commands (seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// Built-in tool that executes shell commands.
pub struct TerminalTool;

#[async_trait]
impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn toolset(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command via bash and return stdout + stderr. Supports optional timeout and working directory."
    }

    fn emoji(&self) -> &str {
        "\u{1f4bb}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                },
                "background": {
                    "type": "boolean",
                    "description": "Run the command in the background."
                },
                "pty": {
                    "type": "boolean",
                    "description": "Run in pseudo-terminal (PTY) mode for interactive CLI tools like Codex or REPL. Default: false."
                },
                "notify_on_complete": {
                    "type": "boolean",
                    "description": "Notify when background process completes."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Maximum time in seconds to wait for the command to finish. Defaults to 180.",
                    "minimum": 1,
                    "maximum": 600
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command. Defaults to the tool context workdir."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: command".into()))?;

        // -----------------------------------------------------------------------
        // HEAVY TASK DETECTION (Anti-Deadlock)
        // Prevent machine hangs by redirecting heavy build/test tasks.
        // -----------------------------------------------------------------------
        let heavy_patterns = [
            "cargo build",
            "cargo test",
            "cargo clippy",
            "cargo check",
            "npm install",
            "npm build",
            "docker build",
            "make ",
            "cmake ",
        ];

        let background = args
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let notify_on_complete = args
            .get("notify_on_complete")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let pty = args.get("pty").and_then(|v| v.as_bool()).unwrap_or(false);

        let timeout_secs = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(600);

        let workdir = args
            .get("workdir")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.workdir);

        let is_heavy = heavy_patterns.iter().any(|p| command.contains(p));
        if is_heavy && !command.contains("--force-local") && !background {
            return Ok(format!(
                "WARNING: Detected heavy task: `{}`\n\n\
                 Executing heavy build/test tasks on the main gateway machine synchronously often leads to system freezes (deadlocks). \n\n\
                 ACTION REQUIRED:\n\
                 1. Use `background: true` to run this task asynchronously in a sandbox without blocking the agent loop.\n\
                 2. Or use the `--force-local` flag if you absolutely must run synchronously (NOT RECOMMENDED).",
                command
            ));
        }

        debug!(command = %command, background = background, notify_on_complete = notify_on_complete, timeout = timeout_secs, workdir = %workdir, "executing terminal command");

        let mut final_command = command.to_string();
        if pty {
            final_command = format!(
                "script -q -e -c '{}' /dev/null",
                final_command.replace("'", "'\\''")
            );
        }

        if background {
            let proc_session_id = format!("{}-{}", ctx.session_id, uuid::Uuid::new_v4());
            let log_dir = std::path::PathBuf::from("/tmp/hakimi_sandbox");
            tokio::fs::create_dir_all(&log_dir)
                .await
                .unwrap_or_default();
            let log_path = log_dir.join(format!("{}.log", proc_session_id));
            let log_file = std::fs::File::create(&log_path)
                .map_err(|e| HakimiError::Tool(format!("failed to create log file: {e}")))?;

            let mut spawn_command = Command::new(bash_program());
            apply_stable_path(&mut spawn_command);
            let child = spawn_command
                .arg("-c")
                .arg(&final_command)
                .current_dir(workdir)
                .stdout(std::process::Stdio::from(log_file.try_clone().unwrap()))
                .stderr(std::process::Stdio::from(log_file))
                .spawn()
                .map_err(|e| {
                    HakimiError::Tool(format!("failed to spawn background process: {e}"))
                })?;

            let info = crate::builtin_process::ProcessInfo {
                session_id: proc_session_id.clone(),
                command: command.to_string(),
                child: Some(child),
            };

            let mut processes = crate::builtin_process::PROCESSES.lock().await;
            processes.insert(proc_session_id.clone(), info);

            if notify_on_complete {
                let sid = proc_session_id.clone();
                let cmd = command.to_string();
                let ctx_session = ctx.session_id.clone();
                tokio::spawn(async move {
                    // Poll for completion
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        let mut exited = false;
                        let mut exit_code = None;
                        {
                            let mut procs = crate::builtin_process::PROCESSES.lock().await;
                            if let Some(info) = procs.get_mut(&sid)
                                && let Some(ref mut c) = info.child
                                && let Ok(Some(status)) = c.try_wait()
                            {
                                exited = true;
                                exit_code = status.code();
                            }
                        }
                        if exited {
                            let message = format!(
                                "Background process `{}` (Session: {}) finished with exit code {:?}.",
                                cmd, sid, exit_code
                            );
                            let queued = crate::builtin_send_message::QueuedMessage {
                                target: "origin".to_string(), // Gateway handles 'origin' routing
                                message,
                                session_id: ctx_session,
                                queued_at: chrono::Utc::now().to_rfc3339(),
                            };
                            if let Ok(mut q) = crate::builtin_send_message::MESSAGE_QUEUE.lock() {
                                q.push_back(queued);
                            }
                            break;
                        }
                    }
                });
            }

            return Ok(json!({
                "output": "Background process started",
                "session_id": proc_session_id,
                "pid": 0,
                "exit_code": 0,
                "error": null
            })
            .to_string());
        }

        let output = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async {
            let mut shell = Command::new(bash_program());
            apply_stable_path(&mut shell);
            shell.arg("-c").arg(&final_command).current_dir(workdir);
            shell.output().await
        })
        .await
        .map_err(|_| {
            debug!(command = %command, timeout = timeout_secs, "command timed out");
            HakimiError::Tool(format!(
                "command timed out after {timeout_secs}s: {command}"
            ))
        })?
        .map_err(|e| {
            debug!(command = %command, error = %e, "failed to spawn command");
            HakimiError::Tool(format!("failed to execute command: {e}"))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        if !stdout.is_empty() {
            result.push_str("STDOUT:\n");
            result.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("STDERR:\n");
            result.push_str(&stderr);
        }

        if let Some(code) = output.status.code() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&format!("EXIT CODE: {code}"));
            if let Some(diagnostic) = diagnose_shell_failure(&stderr, Some(code), workdir) {
                result.push('\n');
                result.push_str("DIAGNOSTIC:\n");
                result.push_str(&diagnostic);
            }
        } else {
            result.push_str("\nEXIT CODE: (terminated by signal)");
        }

        if result.trim().is_empty() {
            result = "(no output)".to_string();
        }

        Ok(result)
    }
}
