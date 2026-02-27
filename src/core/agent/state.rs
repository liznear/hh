use crate::core::{Message, Role, TodoItem};
use crate::tool::ToolResult;
use serde::Deserialize;

#[derive(Debug, Default)]
pub struct AgentState {
    pub messages: Vec<Message>,
    pub todo_items: Vec<TodoItem>,
    pub step: usize,
}

impl AgentState {
    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    pub fn apply_tool_result(&mut self, tool_name: &str, result: &ToolResult) -> bool {
        if result.is_error || tool_name != "todo_write" {
            return false;
        }

        let Some(items) = parse_todos(result) else {
            return false;
        };

        if self.todo_items == items {
            return false;
        }

        self.todo_items = items;
        true
    }

    pub fn state_for_llm(&self) -> Option<Message> {
        if self.todo_items.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        lines.push("Runtime TODO state: use this as the canonical plan snapshot.".to_string());

        let total = self.todo_items.len();
        let pending = self
            .todo_items
            .iter()
            .filter(|item| {
                matches!(
                    item.status,
                    crate::core::TodoStatus::Pending | crate::core::TodoStatus::InProgress
                )
            })
            .count();
        lines.push(format!("{pending} pending out of {total} total tasks."));

        for item in &self.todo_items {
            let status = match item.status {
                crate::core::TodoStatus::Pending => "pending",
                crate::core::TodoStatus::InProgress => "in_progress",
                crate::core::TodoStatus::Completed => "completed",
                crate::core::TodoStatus::Cancelled => "cancelled",
            };
            lines.push(format!("- [{status}] {}", item.content));
        }

        Some(Message {
            role: Role::System,
            content: lines.join("\n"),
            attachments: Vec::new(),
            tool_call_id: None,
        })
    }
}

#[derive(Debug, Deserialize)]
struct TodoWriteOutput {
    todos: Vec<TodoItem>,
}

fn parse_todos(result: &ToolResult) -> Option<Vec<TodoItem>> {
    if let Ok(parsed) = serde_json::from_value::<TodoWriteOutput>(result.payload.clone()) {
        return Some(parsed.todos);
    }

    serde_json::from_str::<TodoWriteOutput>(&result.output)
        .ok()
        .map(|parsed| parsed.todos)
}
