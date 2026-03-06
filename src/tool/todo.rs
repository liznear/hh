use crate::core::agent::{StateOp, StatePatch};
use crate::tool::{Tool, ToolResult, ToolSchema, parse_tool_args};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::{TodoItem, TodoStatus};

pub struct TodoWriteTool;
pub struct TodoReadTool;

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

impl TodoCounts {
    fn from_todos(todos: &[TodoItem]) -> Self {
        let mut counts = Self {
            total: todos.len(),
            pending: 0,
            in_progress: 0,
            completed: 0,
            cancelled: 0,
        };

        for item in todos {
            match item.status {
                TodoStatus::Pending => counts.pending += 1,
                TodoStatus::InProgress => counts.in_progress += 1,
                TodoStatus::Completed => counts.completed += 1,
                TodoStatus::Cancelled => counts.cancelled += 1,
            }
        }

        counts
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "todo_read".to_string(),
            description: "Read canonical todo list state".to_string(),
            capability: Some("todo_read".to_string()),
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        if !args.is_object() {
            return ToolResult::error("invalid todo_read args: expected object");
        }

        let output = TodoWriteOutput {
            todos: Vec::new(),
            counts: TodoCounts::from_todos(&[]),
        };

        ToolResult::ok_json_typed_serializable(
            "todo list snapshot",
            "application/vnd.hh.todo+json",
            &output,
        )
    }
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
        let parsed: TodoWriteArgs = match parse_tool_args(args, "todo_write") {
            Ok(value) => value,
            Err(err) => return err,
        };

        for item in &parsed.todos {
            if item.content.trim().is_empty() {
                return ToolResult::error("todo content must not be empty");
            }
        }

        let todos = parsed.todos;
        let output = TodoWriteOutput {
            counts: TodoCounts::from_todos(&todos),
            todos,
        };

        ToolResult::ok_json_typed_serializable(
            "todo list updated",
            "application/vnd.hh.todo+json",
            &output,
        )
    }

    fn state_patch(&self, _args: &Value, result: &ToolResult) -> StatePatch {
        if result.is_error {
            return StatePatch::none();
        }

        let Some(payload) = result.payload.as_object() else {
            return StatePatch::none();
        };

        let Some(todos) = payload.get("todos") else {
            return StatePatch::none();
        };

        let Ok(items) = serde_json::from_value::<Vec<TodoItem>>(todos.clone()) else {
            return StatePatch::none();
        };

        StatePatch::with_op(StateOp::SetTodoItems { items })
    }
}
