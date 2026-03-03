use super::app::{ChatApp, ChatMessage, ModelOptionView, TodoItemView, TodoPriority, TodoStatus};
use super::event::TuiEvent;
use super::ui::{UiLayout, build_message_lines, render_app};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Color};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn leading_spaces(text: &str) -> usize {
    text.chars().take_while(|c| *c == ' ').count()
}

fn test_layout() -> UiLayout {
    UiLayout::default()
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
    // Expected: → (muted), Test Tool "arg1" (secondary)
    assert!(spans.iter().any(|s| s.content.contains("Test Tool")));
    assert!(spans.iter().any(|s| s.content.contains("arg1")));
}

#[test]
fn pending_task_row_shows_click_to_open_hint() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "task".to_string(),
        args: json!({
            "name": "Check",
            "description": "desc",
            "prompt": "prompt",
            "subagent_type": "explore"
        })
        .to_string(),
        output: None,
        is_error: None,
    });

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();
    assert!(rendered.iter().any(|line| line.contains("(click to open)")));
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
fn failed_tool_call_renders_error_detail_line() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "test_tool".to_string(),
        args: "\"arg1\"".to_string(),
        output: Some("permission denied: missing credentials".to_string()),
        is_error: Some(true),
    });

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("✗") && line.contains("Test Tool"))
    );

    let detail_line = lines
        .iter()
        .find(|line| line_text(line).contains("└ permission denied: missing credentials"))
        .expect("tool error detail line");
    let detail_span = detail_line
        .spans
        .iter()
        .find(|span| span.content.contains("permission denied"))
        .expect("error detail text span");
    assert_eq!(detail_span.style.fg, Some(Color::Red));
}

#[test]
fn failed_tool_call_long_error_wraps_with_child_indent() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "test_tool".to_string(),
        args: "\"arg1\"".to_string(),
        output: Some(
            "this is a very long tool error message that should wrap cleanly across multiple lines without losing indentation"
                .to_string(),
        ),
        is_error: Some(true),
    });

    let rendered: Vec<String> = build_message_lines(&app, 45)
        .iter()
        .map(line_text)
        .collect();

    let first_error_idx = rendered
        .iter()
        .position(|line| line.contains("└ this is a very long"))
        .expect("first wrapped error line");
    let wrapped_line = rendered
        .get(first_error_idx + 1)
        .expect("wrapped continuation line");

    let layout = test_layout();
    assert!(wrapped_line.starts_with(&" ".repeat(layout.message_indent_width() + 4)));
}

#[test]
fn compaction_renders_separator_and_preserves_previous_messages() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Assistant("Before compaction".to_string()));
    app.messages.push(ChatMessage::Compaction(
        "- Keep key requirements\n- Keep pending tasks".to_string(),
    ));

    let lines = build_message_lines(&app, 100);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("Before compaction"))
    );
    assert!(rendered.iter().any(|line| line.contains("Compaction")));
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("Keep key requirements"))
    );
}

#[test]
fn compaction_start_renders_separator_immediately() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Assistant("Before compaction".to_string()));
    app.handle_event(&TuiEvent::CompactionStart);

    let lines = build_message_lines(&app, 100);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(rendered.iter().any(|line| line.contains("Compaction")));
}

#[test]
fn context_usage_ignores_messages_before_compaction_boundary() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Assistant("1234567890".to_string()));
    app.messages
        .push(ChatMessage::Assistant("abcdefghij".to_string()));
    app.messages
        .push(ChatMessage::Compaction("summary".to_string()));
    app.messages
        .push(ChatMessage::Assistant("after".to_string()));

    let (used, _) = app.context_usage();
    let expected_chars = "summary".len() + "after".len();
    let expected_tokens = expected_chars / 4;
    assert_eq!(used, expected_tokens);
}

#[test]
fn context_usage_uses_provider_reported_tokens_when_available() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Assistant("local estimate".to_string()));

    app.handle_event(&TuiEvent::ContextUsage(4321));

    let (used, _) = app.context_usage();
    assert_eq!(used, 4321);
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
        rendered.iter().all(|line| !line.contains('→')),
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
    let layout = test_layout();

    assert!(
        rendered.iter().any(|line| {
            line.trim_end().ends_with(".iter()")
                && leading_spaces(line) >= layout.message_indent_width() + 4
        }),
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
fn assistant_messages_do_not_render_assistant_label() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Assistant("Hello world".to_string()));

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    assert!(rendered.iter().any(|line| line.contains("Hello world")));
    assert!(rendered.iter().all(|line| !line.contains("Assistant")));
}

#[test]
fn assistant_list_items_render_during_streaming() {
    let mut app = ChatApp::default();
    app.handle_event(&TuiEvent::AssistantDelta("- first item\n".to_string()));
    app.handle_event(&TuiEvent::AssistantDelta("- second item".to_string()));

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();
    let combined = rendered.join("\n");

    assert!(combined.contains("- first item"));
    assert!(combined.contains("- second item"));
}

#[test]
fn thinking_message_is_not_truncated() {
    let mut app = ChatApp::default();
    let tail = "TAIL_SEGMENT";
    app.messages.push(ChatMessage::Thinking(format!(
        "{}{}",
        "a".repeat(260),
        tail
    )));

    let lines = build_message_lines(&app, 120);
    let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");

    assert!(rendered.contains(tail));
}

#[test]
fn thinking_has_one_blank_line_before_it() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Assistant("previous".to_string()));
    app.messages
        .push(ChatMessage::Thinking("thinking".to_string()));
    app.messages
        .push(ChatMessage::Assistant("answer".to_string()));

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let think_idx = rendered
        .iter()
        .position(|line| line.contains("Thinking:"))
        .expect("thinking line");
    assert_eq!(rendered[think_idx - 1], "");
    assert_ne!(rendered[think_idx - 2], "");
}

#[test]
fn thinking_has_one_blank_line_after_it() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Thinking("thinking".to_string()));
    app.messages
        .push(ChatMessage::Assistant("answer".to_string()));

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let think_idx = rendered
        .iter()
        .position(|line| line.contains("Thinking:"))
        .expect("thinking line");

    assert_eq!(rendered[think_idx + 1], "");
    assert_ne!(rendered[think_idx + 2], "");
}

#[test]
fn thinking_with_trailing_newlines_has_one_blank_line_after_it() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Thinking("thinking\n\n".to_string()));
    app.messages
        .push(ChatMessage::Assistant("answer".to_string()));

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let think_idx = rendered
        .iter()
        .position(|line| line.contains("Thinking:"))
        .expect("thinking line");

    assert_eq!(rendered[think_idx + 1], "");
    assert_ne!(rendered[think_idx + 2], "");
}

#[test]
fn thinking_continuation_lines_use_message_indent() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::Thinking(
        "this is a long thinking message that should wrap to multiple lines".to_string(),
    ));

    let rendered: Vec<String> = build_message_lines(&app, 35)
        .iter()
        .map(line_text)
        .collect();
    let layout = test_layout();

    let think_idx = rendered
        .iter()
        .position(|line| line.contains("Thinking:"))
        .expect("thinking line");

    assert!(
        !rendered[think_idx + 1].is_empty(),
        "expected wrapped continuation line"
    );
    assert_eq!(
        leading_spaces(&rendered[think_idx + 1]),
        layout.message_indent_width()
    );
}

#[test]
fn assistant_has_one_blank_line_before_it_after_tool_output() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "bash".to_string(),
        args: json!({"command": "ls"}).to_string(),
        output: Some("ok".to_string()),
        is_error: Some(false),
    });
    app.messages
        .push(ChatMessage::Assistant("answer".to_string()));

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let answer_idx = rendered
        .iter()
        .position(|line| line.contains("answer"))
        .expect("assistant line");

    assert_eq!(rendered[answer_idx - 1], "");
    assert_ne!(rendered[answer_idx - 2], "");
}

#[test]
fn tool_call_has_one_blank_line_before_it_after_assistant() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::Assistant(
        "Let me look at the TUI rendering code to understand how messages are displayed:"
            .to_string(),
    ));
    app.messages.push(ChatMessage::ToolCall {
        name: "read".to_string(),
        args: json!({"filePath": "./src/cli/tui/ui.rs"}).to_string(),
        output: Some("ok".to_string()),
        is_error: Some(false),
    });

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let tool_idx = rendered
        .iter()
        .position(|line| line.contains("Read ./src/cli/tui/ui.rs"))
        .expect("tool line");

    assert_eq!(rendered[tool_idx - 1], "");
    assert_ne!(rendered[tool_idx - 2], "");
}

#[test]
fn thinking_uses_markdown_renderer() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Thinking("**bold** `code`".to_string()));

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();

    let think_line = rendered
        .iter()
        .find(|line| line.contains("Thinking:"))
        .expect("thinking line");
    assert!(think_line.contains("bold code"));
    assert!(!think_line.contains("**"));
    assert!(!think_line.contains('`'));
}

#[test]
fn thinking_list_items_render_during_streaming() {
    let mut app = ChatApp::default();
    app.handle_event(&TuiEvent::Thinking(
        "- first item\n- second item".to_string(),
    ));

    let lines = build_message_lines(&app, 120);
    let rendered: Vec<String> = lines.iter().map(line_text).collect();
    let combined = rendered.join("\n");

    assert!(combined.contains("Thinking: - first item"));
    assert!(combined.contains("- second item"));
}

#[test]
fn thinking_prefix_is_yellow_and_body_is_grey() {
    let mut app = ChatApp::default();
    app.messages
        .push(ChatMessage::Thinking("hello".to_string()));

    let lines = build_message_lines(&app, 120);
    let think_line = lines
        .iter()
        .find(|line| line_text(line).contains("Thinking:"))
        .expect("thinking line");

    let prefix_span = think_line
        .spans
        .iter()
        .find(|span| span.content.contains("Thinking:"))
        .expect("thinking prefix span");
    assert_eq!(prefix_span.style.fg, Some(Color::Rgb(227, 152, 67)));

    let body_span = think_line
        .spans
        .iter()
        .find(|span| span.content.contains("hello"))
        .expect("thinking body span");
    assert_eq!(body_span.style.fg, Some(Color::Rgb(98, 108, 124)));
}

#[test]
fn user_prompt_box_has_inner_top_bottom_padding_and_left_indent() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::User("hello".to_string()));

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let bubble_lines: Vec<&String> = rendered.iter().filter(|line| line.contains('▌')).collect();
    assert!(bubble_lines.len() >= 3);
    let layout = test_layout();
    let bubble_prefix = format!("{}▌", " ".repeat(layout.main_content_left_offset()));
    assert!(
        bubble_lines
            .iter()
            .all(|line| line.starts_with(&bubble_prefix))
    );
}

#[test]
fn error_message_uses_message_indent() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::Error(
        "Reached max steps without final answer".to_string(),
    ));

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();

    let error_line = rendered
        .iter()
        .find(|line| line.contains("Error:"))
        .expect("error line");
    let layout = test_layout();
    assert!(error_line.starts_with(&format!(
        "{}Error:",
        " ".repeat(layout.message_indent_width())
    )));
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
fn sidebar_omits_todo_section_when_empty() {
    let app = ChatApp::default();
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

    assert!(full_text.contains("Context"));
    assert!(!full_text.contains("TODO"));
}

#[test]
fn sidebar_modified_files_section_shows_aggregated_stats() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "edit".to_string(),
        args: "{}".to_string(),
        output: Some(
            serde_json::json!({
                "path": "src/cli/tui/ui.rs",
                "applied": true,
                "summary": {"added_lines": 2, "removed_lines": 1},
                "diff": ""
            })
            .to_string(),
        ),
        is_error: Some(false),
    });
    app.messages.push(ChatMessage::ToolCall {
        name: "write".to_string(),
        args: "{}".to_string(),
        output: Some(
            serde_json::json!({
                "path": "src/cli/tui/ui.rs",
                "applied": true,
                "summary": {"added_lines": 1, "removed_lines": 0},
                "diff": ""
            })
            .to_string(),
        ),
        is_error: Some(false),
    });
    app.messages.push(ChatMessage::ToolCall {
        name: "edit".to_string(),
        args: "{}".to_string(),
        output: Some(
            serde_json::json!({
                "path": "src/cli/tui/app.rs",
                "applied": true,
                "summary": {"added_lines": 4, "removed_lines": 0},
                "diff": ""
            })
            .to_string(),
        ),
        is_error: Some(false),
    });

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

    assert!(full_text.contains("Modified Files"));
    assert!(full_text.contains("src/cli/tui/ui.rs"));
    assert!(full_text.contains("+3 -1"));
    assert!(full_text.contains("src/cli/tui/app.rs"));
    assert!(full_text.contains("+4"));
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
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("+") && line.contains("new"))
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("-") && line.contains("old"))
    );

    let added_span = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref().contains("new"))
        .expect("added diff line");
    let removed_span = lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.as_ref().contains("old"))
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
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("+") && line.contains("new"))
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("-") && line.contains("old"))
    );
}

#[test]
fn edit_diff_header_includes_tool_name() {
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

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("Edit src/main.rs  +1 -1"))
    );
}

#[test]
fn side_by_side_diff_pairs_removed_and_added_lines() {
    let mut app = ChatApp::default();
    let output = serde_json::json!({
        "path": "src/main.rs",
        "applied": true,
        "summary": {"added_lines": 1, "removed_lines": 1},
        "diff": "@@ -1 +1 @@\n-old\n+new\n"
    })
    .to_string();

    app.messages.push(ChatMessage::ToolCall {
        name: "edit".to_string(),
        args: "{}".to_string(),
        output: Some(output),
        is_error: Some(false),
    });

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("old") && line.contains("|") && line.contains("new"))
    );
}

#[test]
fn side_by_side_diff_shows_line_numbers_for_changed_rows() {
    let mut app = ChatApp::default();
    let output = serde_json::json!({
        "path": "Cargo.toml",
        "applied": true,
        "summary": {"added_lines": 1, "removed_lines": 1},
        "diff": "@@ -27 +27 @@\n-arboard = \"3.6\"\n+arboard = { version = \"3.6\", features = [\"wayland-data-control\"] }\n"
    })
    .to_string();

    app.messages.push(ChatMessage::ToolCall {
        name: "edit".to_string(),
        args: "{}".to_string(),
        output: Some(output),
        is_error: Some(false),
    });

    let rendered: Vec<String> = build_message_lines(&app, 140)
        .iter()
        .map(line_text)
        .collect();

    let changed_row = rendered
        .iter()
        .find(|line| line.contains("arboard") && line.contains("|"))
        .expect("changed row");
    assert!(changed_row.contains("27 -"));
    assert!(changed_row.contains("27 +"));
}

#[test]
fn list_tool_completed_row_shows_entry_count() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "list".to_string(),
        args: json!({"path":"."}).to_string(),
        output: Some(json!({"path":".","count":3,"entries":["a","b","c"]}).to_string()),
        is_error: Some(false),
    });

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();
    assert!(rendered.iter().any(|line| line.contains("(3 entries)")));
}

#[test]
fn grep_tool_completed_row_shows_match_count() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolCall {
        name: "grep".to_string(),
        args: json!({"path":".","pattern":"foo"}).to_string(),
        output: Some(
            json!({"path":".","pattern":"foo","count":2,"matches":["a:1:foo","b:2:foo"]})
                .to_string(),
        ),
        is_error: Some(false),
    });

    let rendered: Vec<String> = build_message_lines(&app, 120)
        .iter()
        .map(line_text)
        .collect();
    assert!(rendered.iter().any(|line| line.contains("(2 matches)")));
}

#[test]
fn status_row_text_is_not_rendered() {
    let app = ChatApp::default();
    let backend = TestBackend::new(120, 25);
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

    assert!(!full_text.contains(":quit | Ctrl+C"));
}

#[test]
fn processing_indicator_uses_block_spinner_glyphs() {
    let mut app = ChatApp::default();
    app.set_processing(true);

    let backend = TestBackend::new(120, 25);
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

    assert!(full_text.contains("■"));
    assert!(full_text.contains("⬝"));
    assert!(full_text.contains("0s"));
    assert!(
        full_text.find("0s").expect("find processing duration")
            < full_text
                .find("esc interrupt")
                .expect("find interrupt hint")
    );
    assert!(!full_text.contains("00s"));
    assert!(!full_text.contains("00m 00s"));
    assert!(full_text.contains("esc interrupt"));
    assert!(!full_text.contains("esc again to interrupt"));
}

#[test]
fn processing_indicator_uses_confirm_interrupt_hint_after_first_esc() {
    let mut app = ChatApp::default();
    app.set_processing(true);
    app.arm_esc_interrupt();

    let backend = TestBackend::new(120, 25);
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

    assert!(full_text.contains("esc again to interrupt"));
    assert!(!full_text.contains("esc interrupt"));
}

#[test]
fn processing_indicator_is_hidden_when_idle() {
    let app = ChatApp::default();

    let backend = TestBackend::new(120, 25);
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

    assert!(!full_text.contains("esc interrupt"));
    assert!(!full_text.contains("■"));
}

#[test]
fn input_panel_keeps_top_padding_and_renders_second_line() {
    let mut app = ChatApp::default();
    app.set_input("a\nb".to_string());

    let backend = TestBackend::new(120, 25);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw app");

    let buffer = terminal.backend().buffer();
    let mut pos_a = None;
    let mut pos_b = None;
    for y in 0..25u16 {
        for x in 0..120u16 {
            let symbol = buffer[(x, y)].symbol();
            if symbol == "a" {
                pos_a = Some((x, y));
            }
            if symbol == "b" {
                pos_b = Some((x, y));
            }
        }
    }

    let (x_a, y_a) = pos_a.expect("find first input line");
    let (x_b, y_b) = pos_b.expect("find second input line");
    assert_eq!(x_a, x_b);
    assert_eq!(y_b, y_a + 1);
    assert_eq!(buffer[(x_a, y_a - 1)].symbol(), " ");
}

#[test]
fn input_panel_grows_up_to_five_lines() {
    let mut app = ChatApp::default();
    app.set_input("L1\nL2\nL3\nL4\nL5\nL6".to_string());

    let backend = TestBackend::new(120, 25);
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

    assert!(!full_text.contains("L1"));
    assert!(full_text.contains("L2"));
    assert!(full_text.contains("L3"));
    assert!(full_text.contains("L4"));
    assert!(full_text.contains("L5"));
    assert!(full_text.contains("L6"));
}

#[test]
fn input_panel_renders_model_line_with_blank_separator() {
    let mut app = ChatApp::default();
    app.set_input("hello".to_string());
    app.configure_models(
        "openai/gpt-5".to_string(),
        vec![ModelOptionView {
            full_id: "openai/gpt-5".to_string(),
            provider_name: "OpenAI".to_string(),
            model_name: "GPT-5".to_string(),
            modality: "text -> text".to_string(),
            max_context_size: 128_000,
        }],
    );

    let backend = TestBackend::new(120, 25);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw app");

    let buffer = terminal.backend().buffer();
    let mut input_pos = None;
    let mut status_y = None;

    for y in 0..25u16 {
        let row_text = (0..120u16)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if row_text.contains("OpenAI GPT-5") {
            status_y = Some(y);
        }

        for x in 0..120u16 {
            if buffer[(x, y)].symbol() == "h" {
                input_pos = Some((x, y));
            }
        }
    }

    let (input_x, input_y) = input_pos.expect("find input row");
    let status_y = status_y.expect("find model status row");
    assert_eq!(status_y, input_y + 2);
    assert_eq!(buffer[(input_x, input_y + 1)].symbol(), " ");
}

#[test]
fn submitting_bulleted_input_does_not_seed_todos_without_tool_output() {
    let mut app = ChatApp::default();
    app.set_input("- Add one blank line above the session title in sidebar.".to_string());

    let _ = app.submit_input();

    assert!(app.todo_items.is_empty());
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

#[test]
fn question_prompt_renders_overlay() {
    let mut app = ChatApp::default();
    let (tx, _rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "Is it something you can hold in one hand?".to_string(),
            header: "Size".to_string(),
            options: vec![
                crate::core::QuestionOption {
                    label: "Yes".to_string(),
                    description: "Small enough to hold easily".to_string(),
                },
                crate::core::QuestionOption {
                    label: "No".to_string(),
                    description: "Too large or unwieldy for one hand".to_string(),
                },
            ],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let backend = TestBackend::new(120, 30);
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

    assert!(full_text.contains("Is it something you can hold in one hand?"));
    assert!(full_text.contains("Type your own answer"));
}

#[test]
fn question_prompt_submits_selected_answer_on_enter() {
    let mut app = ChatApp::default();
    let (tx, rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "Pick one".to_string(),
            header: "Choice".to_string(),
            options: vec![
                crate::core::QuestionOption {
                    label: "Alpha".to_string(),
                    description: "First".to_string(),
                },
                crate::core::QuestionOption {
                    label: "Beta".to_string(),
                    description: "Second".to_string(),
                },
            ],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let result = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(result, crate::cli::tui::QuestionKeyResult::Submitted);

    let answers = rx
        .blocking_recv()
        .expect("question response channel should receive a value")
        .expect("question should resolve successfully");
    assert_eq!(answers, vec![vec!["Alpha".to_string()]]);
}

#[test]
fn custom_option_typing_requires_enter_first() {
    let mut app = ChatApp::default();
    let (tx, _rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "What name should I use?".to_string(),
            header: "Name".to_string(),
            options: vec![crate::core::QuestionOption {
                label: "Preferred name".to_string(),
                description: "Use a short name".to_string(),
            }],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

    let before_enter = app.pending_question_view().expect("pending question");
    assert!(!before_enter.custom_mode);
    assert!(before_enter.custom_value.is_empty());

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

    let after_enter = app.pending_question_view().expect("pending question");
    assert!(after_enter.custom_mode);
    assert_eq!(after_enter.custom_value, "a");
}

#[test]
fn esc_clears_custom_once_then_dismisses_on_second_press() {
    let mut app = ChatApp::default();
    let (tx, mut rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "What name should I use?".to_string(),
            header: "Name".to_string(),
            options: vec![crate::core::QuestionOption {
                label: "Preferred name".to_string(),
                description: "Use a short name".to_string(),
            }],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    let first = app.handle_question_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(first, crate::cli::tui::QuestionKeyResult::Handled);
    assert!(app.has_pending_question());
    let after_first = app.pending_question_view().expect("pending question");
    assert!(!after_first.custom_mode);
    assert!(after_first.custom_value.is_empty());
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    let second = app.handle_question_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(second, crate::cli::tui::QuestionKeyResult::Dismissed);
    assert!(!app.has_pending_question());
    assert!(
        rx.blocking_recv()
            .expect("question response should be sent")
            .is_err()
    );
}

#[test]
fn entering_custom_option_adds_newline_placeholder() {
    let mut app = ChatApp::default();
    let (tx, _rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "What name should I use?".to_string(),
            header: "Name".to_string(),
            options: vec![crate::core::QuestionOption {
                label: "Preferred name".to_string(),
                description: "Use a short name".to_string(),
            }],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let pending = app.pending_question_view().expect("pending question");
    assert!(pending.custom_mode);
    assert_eq!(pending.custom_value, "");
}

#[test]
fn custom_mode_enter_submits_answer() {
    let mut app = ChatApp::default();
    let (tx, rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "What name should I use?".to_string(),
            header: "Name".to_string(),
            options: vec![crate::core::QuestionOption {
                label: "Preferred name".to_string(),
                description: "Use a short name".to_string(),
            }],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::NONE));
    let result = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(result, crate::cli::tui::QuestionKeyResult::Submitted);
    let answers = rx
        .blocking_recv()
        .expect("question response should be sent")
        .expect("question should resolve successfully");
    assert_eq!(answers, vec![vec!["N".to_string()]]);
}

#[test]
fn custom_mode_shift_enter_inserts_newline() {
    let mut app = ChatApp::default();
    let (tx, _rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "What name should I use?".to_string(),
            header: "Name".to_string(),
            options: vec![crate::core::QuestionOption {
                label: "Preferred name".to_string(),
                description: "Use a short name".to_string(),
            }],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    let pending = app.pending_question_view().expect("pending question");
    assert!(pending.custom_mode);
    assert_eq!(pending.custom_value, "\n");
}

#[test]
fn custom_typed_value_renders_once() {
    let mut app = ChatApp::default();
    let (tx, _rx) = oneshot::channel();
    app.handle_event(&TuiEvent::QuestionPrompt {
        questions: vec![crate::core::QuestionPrompt {
            question: "What name should I use?".to_string(),
            header: "Name".to_string(),
            options: vec![crate::core::QuestionOption {
                label: "Preferred name".to_string(),
                description: "Use a short name".to_string(),
            }],
            multiple: false,
            custom: true,
        }],
        responder: Arc::new(Mutex::new(Some(tx))),
    });

    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let _ = app.handle_question_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    for c in "asdf".chars() {
        let _ = app.handle_question_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    let backend = TestBackend::new(120, 30);
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

    assert_eq!(full_text.match_indices("asdf").count(), 1);
}

#[test]
fn subagent_session_view_sidebar_and_back_hint_render() {
    let mut app = ChatApp::default();
    app.session_name = "Parent Session".to_string();
    app.subagent_items.push(crate::cli::tui::SubagentItemView {
        task_id: "task-1".to_string(),
        session_id: "session-1".to_string(),
        name: "Inspect code".to_string(),
        parent_task_id: None,
        agent_name: "general".to_string(),
        prompt: "scan repo".to_string(),
        summary: None,
        depth: 1,
        started_at: 1,
        finished_at: None,
        status: crate::cli::tui::SubagentStatusView::Running,
    });
    app.open_subagent_session(
        "task-1".to_string(),
        "session-1".to_string(),
        "Inspect code".to_string(),
        vec![ChatMessage::Assistant("child message".to_string())],
    );

    let backend = TestBackend::new(120, 30);
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

    assert!(full_text.contains("Parent Session"));
    assert!(full_text.contains("-> Subagent Session: Inspect code"));
    assert!(full_text.contains("esc back to main agent"));
    assert!(!full_text.contains("<- Back"));
}

#[test]
fn completed_subagent_session_shows_footer_instead_of_back_hint() {
    let mut app = ChatApp::default();
    app.subagent_items.push(crate::cli::tui::SubagentItemView {
        task_id: "task-1".to_string(),
        session_id: "session-1".to_string(),
        name: "Inspect code".to_string(),
        parent_task_id: None,
        agent_name: "general".to_string(),
        prompt: "scan repo".to_string(),
        summary: Some("done".to_string()),
        depth: 1,
        started_at: 1,
        finished_at: Some(9),
        status: crate::cli::tui::SubagentStatusView::Completed,
    });
    app.open_subagent_session(
        "task-1".to_string(),
        "session-1".to_string(),
        "Inspect code".to_string(),
        vec![ChatMessage::Assistant("child message".to_string())],
    );

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_app(frame, &app))
        .expect("draw app");

    let full_text = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(full_text.contains("✓"));
    assert!(!full_text.contains("esc back to main agent"));
}
