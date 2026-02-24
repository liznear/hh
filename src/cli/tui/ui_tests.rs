use super::app::{ChatApp, ChatMessage};
use super::ui::build_message_lines;

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
