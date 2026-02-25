use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub struct TodoWriteTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    content: String,
    status: String,
    priority: String,
}

#[derive(Debug, Deserialize)]
struct TodoWriteArgs {
    todos: Vec<TodoItem>,
}

#[derive(Debug, Serialize)]
struct TodoCounts {
    total: usize,
    pending: usize,
    in_progress: usize,
    completed: usize,
    cancelled: usize,
}

#[derive(Debug, Serialize)]
struct TodoWriteOutput {
    todos: Vec<TodoItem>,
    counts: TodoCounts,
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "todo_write".to_string(),
            description: "Set canonical todo list state".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {"type": "string"},
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed", "cancelled"]
                                },
                                "priority": {
                                    "type": "string",
                                    "enum": ["high", "medium", "low"]
                                }
                            },
                            "required": ["content", "status", "priority"]
                        }
                    }
                },
                "required": ["todos"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let parsed: TodoWriteArgs = match serde_json::from_value(args) {
            Ok(value) => value,
            Err(err) => return tool_err(format!("invalid todo_write args: {err}")),
        };

        for item in &parsed.todos {
            if item.content.trim().is_empty() {
                return tool_err("todo content must not be empty");
            }
            if !matches!(
                item.status.as_str(),
                "pending" | "in_progress" | "completed" | "cancelled"
            ) {
                return tool_err(format!("invalid todo status: {}", item.status));
            }
            if !matches!(item.priority.as_str(), "high" | "medium" | "low") {
                return tool_err(format!("invalid todo priority: {}", item.priority));
            }
        }

        let counts = TodoCounts {
            total: parsed.todos.len(),
            pending: parsed
                .todos
                .iter()
                .filter(|item| item.status == "pending")
                .count(),
            in_progress: parsed
                .todos
                .iter()
                .filter(|item| item.status == "in_progress")
                .count(),
            completed: parsed
                .todos
                .iter()
                .filter(|item| item.status == "completed")
                .count(),
            cancelled: parsed
                .todos
                .iter()
                .filter(|item| item.status == "cancelled")
                .count(),
        };

        let output = TodoWriteOutput {
            todos: parsed.todos,
            counts,
        };

        match serde_json::to_string(&output) {
            Ok(serialized) => tool_ok(serialized),
            Err(err) => tool_err(format!("failed to serialize todo output: {err}")),
        }
    }
}

fn tool_ok(output: impl Into<String>) -> ToolResult {
    ToolResult {
        is_error: false,
        output: output.into(),
    }
}

fn tool_err(err: impl ToString) -> ToolResult {
    ToolResult {
        is_error: true,
        output: err.to_string(),
    }
}
