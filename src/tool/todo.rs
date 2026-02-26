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
            capability: Some("todo_write".to_string()),
            mutating: Some(true),
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
            Err(err) => return ToolResult::error(format!("invalid todo_write args: {err}")),
        };

        for item in &parsed.todos {
            if item.content.trim().is_empty() {
                return ToolResult::error("todo content must not be empty");
            }
            if !matches!(
                item.status.as_str(),
                "pending" | "in_progress" | "completed" | "cancelled"
            ) {
                return ToolResult::error(format!("invalid todo status: {}", item.status));
            }
            if !matches!(item.priority.as_str(), "high" | "medium" | "low") {
                return ToolResult::error(format!("invalid todo priority: {}", item.priority));
            }
        }

        let mut counts = TodoCounts {
            total: parsed.todos.len(),
            pending: 0,
            in_progress: 0,
            completed: 0,
            cancelled: 0,
        };
        for item in &parsed.todos {
            match item.status.as_str() {
                "pending" => counts.pending += 1,
                "in_progress" => counts.in_progress += 1,
                "completed" => counts.completed += 1,
                "cancelled" => counts.cancelled += 1,
                _ => {}
            }
        }

        let output = TodoWriteOutput {
            todos: parsed.todos,
            counts,
        };

        ToolResult::ok_json_typed_serializable(
            "todo list updated",
            "application/vnd.hh.todo+json",
            &output,
        )
    }
}
