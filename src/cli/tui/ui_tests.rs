use super::app::{ChatApp, ChatMessage, TodoItemView, TodoPriority, TodoStatus};
use super::event::TuiEvent;
use super::ui::{build_message_lines, render_app};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

#[test]
fn test_tool_start_rendering() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "test_tool".to_string(),
        args: "\"arg1\"".to_string(),
        output: None,
        is_error: None,
    });

    let lines = build_message_lines(&app, 100);
    // Should be 1 line if fits
    assert_eq!(lines.len(), 1, "Expected single line for short args");

    // Verify content of first line
    let spans = &lines[0].spans;
    // Expected: -> (muted), Test Tool "arg1" (secondary)
    assert!(spans.iter().any(|s| s.content.contains("Test Tool")));
    assert!(spans.iter().any(|s| s.content.contains("arg1")));
}

#[test]
fn test_tool_end_rendering() {
    let mut app = ChatApp::default();
    // Compact rendering shows the original action label, not the output
    app.messages.push(ChatMessage::ToolCall {
        name: "test_tool".to_string(),
        args: "\"arg1\"".to_string(),
        output: Some("success".to_string()),
        is_error: Some(false),
    });

    let lines = build_message_lines(&app, 100);
    // Should be 1 line if fits
    assert_eq!(lines.len(), 1, "Expected single line for short output");

    let spans = &lines[0].spans;
    // Expected: ✓ (accent), Test Tool "arg1" (secondary)
    assert!(spans.iter().any(|s| s.content.contains("Test Tool")));
    assert!(spans.iter().any(|s| s.content.contains("arg1")));
    // We do NOT show output in compact mode
}

#[test]
fn test_tool_start_wrapping() {
    let mut app = ChatApp::default();
    // Create a long JSON string for args to force wrapping
    let long_args = format!("\"{}\"", "a".repeat(200));
    app.messages.push(ChatMessage::ToolCall {
        name: "test_tool".to_string(),
        args: long_args,
        output: None,
        is_error: None,
    });

    let lines = build_message_lines(&app, 50);
    // Should wrap
    assert!(lines.len() > 1, "Expected multiple lines for long args");
}

#[test]
fn test_duplicate_pending_tool_start_is_deduped() {
    let mut app = ChatApp::default();
    let args = json!({"command": "rm plan.md"});

    app.handle_event(&TuiEvent::ToolStart {
        name: "bash".to_string(),
        args: args.clone(),
    });
    app.handle_event(&TuiEvent::ToolStart {
        name: "bash".to_string(),
        args,
    });
    app.handle_event(&TuiEvent::ToolEnd {
        name: "bash".to_string(),
        result: crate::tool::ToolResult::ok_text("ok", ""),
    });

    let lines = build_message_lines(&app, 100);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert_eq!(
        rendered
            .iter()
            .filter(|line| line.contains("Run `rm plan.md`"))
            .count(),
        1,
        "Expected exactly one tool call row after duplicate starts"
    );
    assert!(
        rendered.iter().any(|line| line.contains('✓')),
        "Expected completed status marker"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("->")),
        "Expected no pending marker to remain"
    );
}

#[test]
fn test_fenced_code_block_hides_fences() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::Assistant(
        "```rust\nlet x = 1;\n```".to_string(),
    ));

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(
        rendered.iter().any(|line| line.contains("let x = 1;")),
        "Expected code content to render"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("```")),
        "Expected fence markers to be hidden"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("`rust")),
        "Expected no broken fence rendering"
    );
}

#[test]
fn test_fenced_code_block_preserves_indentation() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::Assistant(
        "```rust\nif state {\n    .iter()\n}\n```".to_string(),
    ));

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(
        rendered.iter().any(|line| line == "      .iter()"),
        "Expected leading spaces in code line to be preserved"
    );
}

#[test]
fn test_fenced_code_block_applies_keyword_highlighting() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::Assistant(
        "```rust\nif state {\n    let x = 1;\n}\n```".to_string(),
    ));

    let lines = build_message_lines(&app, 120);
    let if_line = lines
        .iter()
        .find(|line| line_text(line).contains("if state"))
        .expect("Expected a rendered line containing rust code");

    let if_span = if_line
        .spans
        .iter()
        .find(|span| span.content.as_ref().contains("if"))
        .expect("Expected 'if' token span");
    let state_span = if_line
        .spans
        .iter()
        .find(|span| span.content.as_ref().contains("state"))
        .expect("Expected 'state' token span");

    assert_ne!(if_span.style.fg, state_span.style.fg);
}

#[test]
fn sidebar_todo_rendering_shows_progress_and_status_markers() {
    let mut app = ChatApp::default();
    app.todo_items = vec![
        TodoItemView {
            content: "Ship feature".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::High,
        },
        TodoItemView {
            content: "Write tests".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Medium,
        },
    ];

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw app");

    let buffer = terminal.backend().buffer();
    let full_text = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(full_text.contains("1 / 2 done"));
    assert!(full_text.contains("[ ] Ship feature"));
    assert!(full_text.contains("[x] Write tests"));
}

#[test]
fn edit_tool_success_renders_diff_header_and_lines() {
    let mut app = ChatApp::default();
    let output = serde_json::json!({
        "path": "src/main.rs",
        "applied": true,
        "summary": {"added_lines": 1, "removed_lines": 1},
        "diff": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n"
    })
    .to_string();

    app.messages.push(ChatMessage::ToolCall {
        name: "edit".to_string(),
        args: "{}".to_string(),
        output: Some(output),
        is_error: Some(false),
    });

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("src/main.rs  +1 -1"))
    );
    assert!(rendered.iter().any(|line| line.contains("+new")));
    assert!(rendered.iter().any(|line| line.contains("-old")));

    let added_span = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref().contains("+new"))
        .expect("added diff line");
    let removed_span = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref().contains("-old"))
        .expect("removed diff line");

    assert_ne!(added_span.style.fg, removed_span.style.fg);
}

#[test]
fn write_tool_success_renders_diff_header_and_lines() {
    let mut app = ChatApp::default();
    let output = serde_json::json!({
        "path": "README.md",
        "applied": true,
        "summary": {"added_lines": 2, "removed_lines": 1},
        "diff": "--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n-old\n+new\n+line2\n"
    })
    .to_string();

    app.messages.push(ChatMessage::ToolCall {
        name: "write".to_string(),
        args: "{}".to_string(),
        output: Some(output),
        is_error: Some(false),
    });

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("README.md  +2 -1"))
    );
    assert!(rendered.iter().any(|line| line.contains("+new")));
    assert!(rendered.iter().any(|line| line.contains("-old")));
}

#[test]
fn todo_write_tool_end_updates_todo_state_from_full_output() {
    let mut app = ChatApp::default();
    app.handle_event(&TuiEvent::ToolStart {
        name: "todo_write".to_string(),
        args: json!({"todos": []}),
    });

    let output = json!({
        "todos": [
            {"content": "One", "status": "pending", "priority": "low"},
            {"content": "Two", "status": "completed", "priority": "high"}
        ],
        "counts": {"total": 2, "pending": 1, "in_progress": 0, "completed": 1, "cancelled": 0}
    })
    .to_string();

    app.handle_event(&TuiEvent::ToolEnd {
        name: "todo_write".to_string(),
        result: crate::tool::ToolResult::ok_json_typed(
            "todo list updated",
            "application/vnd.hh.todo+json",
            serde_json::from_str(&output).expect("todo output json"),
        ),
    });

    assert_eq!(app.todo_items.len(), 2);
    assert_eq!(app.todo_items[0].content, "One");
    assert_eq!(app.todo_items[0].status, TodoStatus::Pending);
    assert_eq!(app.todo_items[1].status, TodoStatus::Completed);
}
