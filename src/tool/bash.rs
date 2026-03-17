use crate::tool::{Tool, ToolResult, ToolSchema, parse_tool_args};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

pub struct BashTool {
    denylist: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BashArgs {
    command: String,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    60_000
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            denylist: vec![
                "rm -rf /".to_string(),
                "mkfs".to_string(),
                "shutdown".to_string(),
                "reboot".to_string(),
            ],
        }
    }

    fn denied(&self, command: &str) -> bool {
        self.denylist.iter().any(|needle| command.contains(needle))
    }
}

#[async_trait]
impl Tool for BashTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".to_string(),
            description: "Run a shell command".to_string(),
            capability: Some("bash".to_string()),
            mutating: Some(true),
            blocking: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "timeout_ms": {"type": "integer", "minimum": 1}
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let parsed: BashArgs = match parse_tool_args(args, "bash") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let command = parsed.command;
        let timeout_ms = parsed.timeout_ms;

        if self.denied(&command) {
            return ToolResult::err_json(
                "blocked",
                json!({
                    "status": "blocked",
                    "ok": false,
                    "command": command,
                    "error": "command blocked by denylist"
                }),
            );
        }

        let fut = Command::new("sh").arg("-lc").arg(&command).output();
        let output = match timeout(Duration::from_millis(timeout_ms), fut).await {
            Ok(Ok(out)) => out,
            Ok(Err(err)) => {
                return ToolResult::err_json(
                    "spawn_error",
                    json!({
                        "status": "spawn_error",
                        "ok": false,
                        "command": command,
                        "error": err.to_string()
                    }),
                );
            }
            Err(_) => {
                return ToolResult::err_json(
                    "timeout",
                    json!({
                        "status": "timeout",
                        "ok": false,
                        "command": command,
                        "timeout_ms": timeout_ms,
                        "error": format!("command timed out after {} ms", timeout_ms)
                    }),
                );
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = if stderr.trim().is_empty() {
            stdout.to_string()
        } else if stdout.trim().is_empty() {
            stderr.to_string()
        } else {
            format!("{}\n{}", stdout, stderr)
        };

        let is_error = !output.status.success();
        let status_text = if is_error { "error" } else { "success" };
        let exit_code = output.status.code();

        let payload = json!({
            "status": status_text,
            "ok": !is_error,
            "command": command,
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "output": combined
        });

        if is_error {
            ToolResult::err_json("command_failed", payload)
        } else {
            ToolResult::ok_json("command_succeeded", payload)
        }
    }
}
