use super::app::{ChatApp, ChatMessage};
use super::ui::build_message_lines;

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
