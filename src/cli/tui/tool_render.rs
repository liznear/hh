use crate::tool::ToolResult;
use serde::Deserialize;

use super::app::{TodoItemView, TodoPriority, TodoStatus};

#[derive(Debug, Clone)]
pub struct RenderedToolOutput {
    pub text: String,
    pub todos: Option<Vec<TodoItemView>>,
}

pub fn render_tool_result(tool_name: &str, result: &ToolResult) -> RenderedToolOutput {
    let renderer = select_renderer(&result.content_type);
    let mut rendered = renderer(result);

    if rendered.text.is_empty() {
        rendered.text = result.output.clone();
    }

    if rendered.todos.is_none() && tool_name == "todo_write" && !result.is_error {
        rendered.todos = parse_todos(&result.output);
    }

    rendered
}

type Renderer = fn(&ToolResult) -> RenderedToolOutput;

fn select_renderer(content_type: &str) -> Renderer {
    match content_type {
        "application/vnd.hh.todo+json" => render_todo,
        "application/json" => render_json,
        _ => render_text,
    }
}

fn render_text(result: &ToolResult) -> RenderedToolOutput {
    RenderedToolOutput {
        text: result.output.clone(),
        todos: None,
    }
}

fn render_json(result: &ToolResult) -> RenderedToolOutput {
    let text =
        serde_json::to_string_pretty(&result.payload).unwrap_or_else(|_| result.output.clone());
    RenderedToolOutput { text, todos: None }
}

fn render_todo(result: &ToolResult) -> RenderedToolOutput {
    let todos = parse_todos_from_value(&result.payload).or_else(|| parse_todos(&result.output));
    let text =
        serde_json::to_string_pretty(&result.payload).unwrap_or_else(|_| result.output.clone());
    RenderedToolOutput { text, todos }
}

#[derive(Debug, Deserialize)]
struct TodoWriteOutput {
    todos: Vec<TodoWireItem>,
}

#[derive(Debug, Deserialize)]
struct TodoWireItem {
    content: String,
    status: String,
    priority: String,
}

fn parse_todos(raw: &str) -> Option<Vec<TodoItemView>> {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    parse_todos_from_value(&value)
}

fn parse_todos_from_value(value: &serde_json::Value) -> Option<Vec<TodoItemView>> {
    let parsed: TodoWriteOutput = serde_json::from_value(value.clone()).ok()?;
    let mut todos = Vec::with_capacity(parsed.todos.len());
    for item in parsed.todos {
        let status = TodoStatus::from_wire(&item.status)?;
        let priority = TodoPriority::from_wire(&item.priority)?;
        todos.push(TodoItemView {
            content: item.content,
            status,
            priority,
        });
    }
    Some(todos)
}
