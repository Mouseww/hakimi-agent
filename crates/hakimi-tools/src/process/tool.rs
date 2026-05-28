use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, redact_sensitive_text};
use serde_json::{Value as JsonValue, json};
use std::sync::Arc;
use crate::Tool;
use crate::process::manager::ProcessManager;

pub struct ProcessTool {
    manager: Arc<ProcessManager>,
}

impl ProcessTool {
    pub fn new(manager: Arc<ProcessManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for ProcessTool {
    fn name(&self) -> &str {
        "process"
    }

    fn toolset(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Manage background processes. Actions: 
         - 'start': spawns a command in background, returns session_id.
         - 'poll': reads new logs since last offset.
         - 'write': sends raw string to stdin.
         - 'submit': sends string + newline to stdin.
         - 'kill': terminates the process.
         - 'list': shows all background processes."
    }

    fn emoji(&self) -> &str {
        "⚙️"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "poll", "write", "submit", "kill", "list"]
                },
                "command": {
                    "type": "string",
                    "description": "Required for 'start'"
                },
                "session_id": {
                    "type": "string",
                    "description": "Required for all except 'start' and 'list'"
                },
                "data": {
                    "type": "string",
                    "description": "Required for 'write' and 'submit'"
                },
                "offset": {
                    "type": "integer",
                    "default": 0
                },
                "limit": {
                    "type": "integer",
                    "default": 5000
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap();

        match action {
            "start" => {
                let command = args.get("command").and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("command required".into()))?;
                let sid = self.manager.spawn(command, &ctx.workdir).await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Background process started. Session ID: {}", sid))
            }
            "poll" => {
                let sid = args.get("session_id").and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("session_id required".into()))?;
                let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5000) as usize;
                
                let logs = self.manager.read_logs(sid, offset, limit).await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(redact_sensitive_text(&logs))
            }
            "write" | "submit" => {
                let sid = args.get("session_id").and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("session_id required".into()))?;
                let data = args.get("data").and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("data required".into()))?;
                
                let mut input = data.to_string();
                if action == "submit" {
                    input.push('\n');
                }
                
                self.manager.write_stdin(sid, &input).await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Sent to stdin of {}", sid))
            }
            "kill" => {
                let sid = args.get("session_id").and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("session_id required".into()))?;
                self.manager.kill(sid).await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Process {} killed", sid))
            }
            _ => Ok("Action not yet fully implemented in proxy".into())
        }
    }
}
