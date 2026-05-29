use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, redact_sensitive_text};
use serde_json::{Value as JsonValue, json};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::debug;

use crate::Tool;
use crate::shell_env::{apply_stable_path, bash_program, diagnose_shell_failure};

/// Default timeout for terminal commands (seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 180;
const SHELL_HOOK_TIMEOUT_SECS: u64 = 60;
const PRE_TOOL_HOOK_ENV: &str = "HAKIMI_PRE_TOOL_HOOK";
const POST_TOOL_HOOK_ENV: &str = "HAKIMI_POST_TOOL_HOOK";
const DEFAULT_HOOK_BLOCK_MESSAGE: &str = "Blocked by shell hook.";

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

        let workdir = resolve_terminal_workdir(args, ctx);

        if let Some(ShellHookOutcome::Block(message)) =
            run_configured_shell_hook("pre_tool_call", args, ctx, workdir, None).await
        {
            return Err(HakimiError::Tool(redact_sensitive_text(&message)));
        }

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
                .stdout(Stdio::from(log_file.try_clone().unwrap()))
                .stderr(Stdio::from(log_file))
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

            let result = json!({
                "output": "Background process started",
                "session_id": proc_session_id,
                "pid": 0,
                "exit_code": 0,
                "error": null
            })
            .to_string();
            let _ = run_configured_shell_hook("post_tool_call", args, ctx, workdir, Some(&result))
                .await;
            return Ok(result);
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
            result.push_str(&redact_sensitive_text(&stdout));
        }

        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("STDERR:\n");
            result.push_str(&redact_sensitive_text(&stderr));
        }

        if let Some(code) = output.status.code() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&format!("EXIT CODE: {code}"));
            if let Some(diagnostic) = diagnose_shell_failure(&stderr, Some(code), workdir) {
                result.push('\n');
                result.push_str("DIAGNOSTIC:\n");
                result.push_str(&redact_sensitive_text(&diagnostic));
            }
        } else {
            result.push_str("\nEXIT CODE: (terminated by signal)");
        }

        if result.trim().is_empty() {
            result = "(no output)".to_string();
        }

        let _ =
            run_configured_shell_hook("post_tool_call", args, ctx, workdir, Some(&result)).await;

        Ok(result)
    }
}

fn resolve_terminal_workdir<'a>(args: &'a JsonValue, ctx: &'a ToolContext) -> &'a str {
    args.get("workdir")
        .and_then(|v| v.as_str())
        .filter(|workdir| !workdir.trim().is_empty())
        .unwrap_or(&ctx.workdir)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ShellHookOutcome {
    Block(String),
}

async fn run_configured_shell_hook(
    event: &str,
    args: &JsonValue,
    ctx: &ToolContext,
    workdir: &str,
    tool_output: Option<&str>,
) -> Option<ShellHookOutcome> {
    let env_name = match event {
        "pre_tool_call" => PRE_TOOL_HOOK_ENV,
        "post_tool_call" => POST_TOOL_HOOK_ENV,
        _ => return None,
    };
    let hook_command = std::env::var(env_name).ok()?;
    let hook_command = hook_command.trim();
    if hook_command.is_empty() {
        return None;
    }

    run_shell_hook_command(event, hook_command, args, ctx, workdir, tool_output).await
}

async fn run_shell_hook_command(
    event: &str,
    hook_command: &str,
    args: &JsonValue,
    ctx: &ToolContext,
    workdir: &str,
    tool_output: Option<&str>,
) -> Option<ShellHookOutcome> {
    let payload = shell_hook_payload(event, args, ctx, workdir, tool_output);
    let payload = serde_json::to_vec(&payload).ok()?;
    let stdout = execute_shell_hook_command(hook_command, &payload, workdir).await?;
    parse_shell_hook_response(event, &stdout)
}

fn shell_hook_payload(
    event: &str,
    args: &JsonValue,
    ctx: &ToolContext,
    workdir: &str,
    tool_output: Option<&str>,
) -> JsonValue {
    let mut payload = json!({
        "hook_event_name": event,
        "tool_name": "terminal",
        "tool_input": args,
        "session_id": ctx.session_id.as_str(),
        "cwd": workdir,
        "extra": {
            "task_id": ctx.task_id.as_deref(),
        },
    });

    if let Some(output) = tool_output {
        payload["tool_output"] = json!(output);
    }

    payload
}

async fn execute_shell_hook_command(
    hook_command: &str,
    payload: &[u8],
    workdir: &str,
) -> Option<String> {
    let parts = split_hook_command_line(hook_command).ok()?;
    let (program, args) = parts.split_first()?;
    let mut command = Command::new(program);
    apply_stable_path(&mut command);
    command
        .args(args)
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(payload).await.is_err() {
            return None;
        }
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(SHELL_HOOK_TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    .ok()?
    .ok()?;

    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_shell_hook_response(event: &str, stdout: &str) -> Option<ShellHookOutcome> {
    let stdout = stdout.trim();
    if stdout.is_empty() {
        return None;
    }

    let response: JsonValue = serde_json::from_str(stdout).ok()?;
    if event != "pre_tool_call" {
        return None;
    }

    if response.get("action").and_then(|v| v.as_str()) == Some("block") {
        return Some(ShellHookOutcome::Block(shell_hook_block_message(
            &response, "message", "reason",
        )));
    }

    if response.get("decision").and_then(|v| v.as_str()) == Some("block") {
        return Some(ShellHookOutcome::Block(shell_hook_block_message(
            &response, "reason", "message",
        )));
    }

    None
}

fn shell_hook_block_message(response: &JsonValue, primary: &str, fallback: &str) -> String {
    response
        .get(primary)
        .and_then(|v| v.as_str())
        .or_else(|| response.get(fallback).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or(DEFAULT_HOOK_BLOCK_MESSAGE)
        .to_string()
}

fn split_hook_command_line(input: &str) -> std::result::Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push('\\');
                }
            }
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push('\\');
                }
            }
            (None, c) => current.push(c),
        }
    }

    if quote.is_some() {
        return Err("unterminated quote in hook command".to_string());
    }

    if !current.is_empty() {
        parts.push(current);
    }

    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tool;

    fn test_context(workdir: String) -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            task_id: Some("test-task".to_string()),
            workdir,
            ..Default::default()
        }
    }

    #[test]
    fn empty_workdir_falls_back_to_context_workdir() {
        let ctx = test_context("/tmp/hakimi-context".to_string());

        assert_eq!(
            resolve_terminal_workdir(&json!({ "workdir": "" }), &ctx),
            "/tmp/hakimi-context"
        );
        assert_eq!(
            resolve_terminal_workdir(&json!({ "workdir": "   " }), &ctx),
            "/tmp/hakimi-context"
        );
    }

    #[tokio::test]
    async fn terminal_executes_with_context_workdir_when_workdir_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = test_context(temp.path().to_string_lossy().to_string());
        let result = TerminalTool
            .execute(
                &json!({
                    "command": "test -d . && printf ok",
                    "workdir": "",
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.contains("STDOUT:\nok"));
        assert!(result.contains("EXIT CODE: 0"));
    }

    #[tokio::test]
    async fn terminal_redacts_secret_output() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = test_context(temp.path().to_string_lossy().to_string());
        let token = format!("{}{}", "sk-proj-", "abcdefghijklmnopqrstuvwxyz123456");
        let result = TerminalTool
            .execute(
                &json!({
                    "command": format!("printf 'OPENAI_API_KEY={token}'"),
                    "workdir": "",
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.contains(&token));
        assert!(result.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn shell_hook_response_blocks_claude_and_canonical_shapes() {
        assert_eq!(
            parse_shell_hook_response("pre_tool_call", r#"{"decision":"block","reason":"nope"}"#),
            Some(ShellHookOutcome::Block("nope".to_string()))
        );
        assert_eq!(
            parse_shell_hook_response("pre_tool_call", r#"{"action":"block","message":"stop"}"#),
            Some(ShellHookOutcome::Block("stop".to_string()))
        );
        assert_eq!(
            parse_shell_hook_response("post_tool_call", r#"{"action":"block","message":"stop"}"#),
            None
        );
    }

    #[test]
    fn shell_hook_payload_matches_hermes_terminal_shape() {
        let ctx = test_context("/tmp/project".to_string());
        let payload = shell_hook_payload(
            "pre_tool_call",
            &json!({ "command": "echo hi" }),
            &ctx,
            "/tmp/project",
            None,
        );

        assert_eq!(payload["hook_event_name"], "pre_tool_call");
        assert_eq!(payload["tool_name"], "terminal");
        assert_eq!(payload["tool_input"], json!({ "command": "echo hi" }));
        assert_eq!(payload["session_id"], "test-session");
        assert_eq!(payload["cwd"], "/tmp/project");
        assert_eq!(payload["extra"]["task_id"], "test-task");
    }

    #[test]
    fn split_hook_command_line_preserves_quoted_paths_and_args() {
        assert_eq!(
            split_hook_command_line(r#""/tmp/hook script.sh" --flag "two words""#).unwrap(),
            vec!["/tmp/hook script.sh", "--flag", "two words"]
        );
        assert!(split_hook_command_line(r#""/tmp/hook"#).is_err());
    }

    #[tokio::test]
    async fn shell_hook_command_can_block_terminal_pre_call() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("block.sh");
        std::fs::write(
            &script,
            "#!/usr/bin/env bash\ncat >/dev/null\nprintf '{\"decision\":\"block\",\"reason\":\"blocked-by-hook\"}'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }
        let ctx = test_context(temp.path().to_string_lossy().to_string());
        let hook_command = script.to_string_lossy().to_string();

        let outcome = run_shell_hook_command(
            "pre_tool_call",
            &hook_command,
            &json!({ "command": "rm -rf /" }),
            &ctx,
            &ctx.workdir,
            None,
        )
        .await;

        assert_eq!(
            outcome,
            Some(ShellHookOutcome::Block("blocked-by-hook".to_string()))
        );
    }
}
