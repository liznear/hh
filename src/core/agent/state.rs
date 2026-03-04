//! Runtime agent state tracked across turns.
//!
//! Invariants:
//! - Only successful `todo_write` tool results can mutate the in-memory TODO snapshot.
//! - The TODO snapshot is treated as canonical runtime planning state for subsequent turns.

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
            tool_calls: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{TodoPriority, TodoStatus};

    fn sample_todos() -> Vec<TodoItem> {
        vec![
            TodoItem {
                content: "task one".to_string(),
                status: TodoStatus::Pending,
                priority: TodoPriority::Medium,
            },
            TodoItem {
                content: "task two".to_string(),
                status: TodoStatus::Completed,
                priority: TodoPriority::High,
            },
        ]
    }

    #[test]
    fn apply_tool_result_updates_state_for_successful_todo_write_payload() {
        let mut state = AgentState::default();
        let todos = sample_todos();
        let result = ToolResult::ok_json(
            "todo list updated",
            serde_json::json!({
                "todos": todos,
            }),
        );

        assert!(state.apply_tool_result("todo_write", &result));
        assert_eq!(state.todo_items.len(), 2);
        assert_eq!(state.todo_items[0].content, "task one");
        assert_eq!(state.todo_items[1].status, TodoStatus::Completed);
    }

    #[test]
    fn apply_tool_result_parses_todo_write_from_text_output_fallback() {
        let mut state = AgentState::default();
        let result = ToolResult::ok_text(
            "todo list updated",
            r#"{"todos":[{"content":"fallback","status":"in_progress","priority":"low"}]}"#,
        );

        assert!(state.apply_tool_result("todo_write", &result));
        assert_eq!(state.todo_items.len(), 1);
        assert_eq!(state.todo_items[0].status, TodoStatus::InProgress);
        assert_eq!(state.todo_items[0].priority, TodoPriority::Low);
    }

    #[test]
    fn apply_tool_result_ignores_non_todo_write_and_errors() {
        let mut state = AgentState::default();
        let ok = ToolResult::ok_json(
            "todo list updated",
            serde_json::json!({
                "todos": sample_todos(),
            }),
        );
        let err = ToolResult::err_json(
            "todo failed",
            serde_json::json!({
                "todos": sample_todos(),
            }),
        );

        assert!(!state.apply_tool_result("question", &ok));
        assert!(state.todo_items.is_empty());

        assert!(!state.apply_tool_result("todo_write", &err));
        assert!(state.todo_items.is_empty());
    }

    #[test]
    fn state_for_llm_reports_pending_count_and_items() {
        let mut state = AgentState::default();
        state.todo_items = sample_todos();

        let message = state.state_for_llm().expect("todo state message");
        assert!(matches!(message.role, Role::System));
        assert!(
            message
                .content
                .contains("Runtime TODO state: use this as the canonical plan snapshot.")
        );
        assert!(message.content.contains("1 pending out of 2 total tasks."));
        assert!(message.content.contains("- [pending] task one"));
        assert!(message.content.contains("- [completed] task two"));
    }
}
