use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

fn to_json_string(value: Value) -> String {
    serde_json::to_string(&value)
        .unwrap_or_else(|err| format!("{{\"status\":\"serialization_error\",\"error\":\"{err}\"}}"))
}

pub struct BashTool {
    denylist: Vec<String>,
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
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(60_000);

        if self.denied(command) {
            return ToolResult {
                is_error: true,
                output: to_json_string(json!({
                    "status": "blocked",
                    "ok": false,
                    "command": command,
                    "error": "command blocked by denylist"
                })),
            };
        }

        let fut = Command::new("sh").arg("-lc").arg(command).output();
        let output = match timeout(Duration::from_millis(timeout_ms), fut).await {
            Ok(Ok(out)) => out,
            Ok(Err(err)) => {
                return ToolResult {
                    is_error: true,
                    output: to_json_string(json!({
                        "status": "spawn_error",
                        "ok": false,
                        "command": command,
                        "error": err.to_string()
                    })),
                };
            }
            Err(_) => {
                return ToolResult {
                    is_error: true,
                    output: to_json_string(json!({
                        "status": "timeout",
                        "ok": false,
                        "command": command,
                        "timeout_ms": timeout_ms,
                        "error": format!("command timed out after {} ms", timeout_ms)
                    })),
                };
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

        ToolResult {
            is_error,
            output: to_json_string(json!({
                "status": status_text,
                "ok": !is_error,
                "command": command,
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr,
                "output": combined
            })),
        }
    }
}
