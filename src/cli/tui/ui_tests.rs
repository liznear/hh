use super::app::{ChatApp, ChatMessage};
use super::ui::build_message_lines;

#[test]
fn test_tool_start_rendering() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolStart {
        name: "test_tool".to_string(),
        args: "arg1".to_string(),
    });

    let lines = build_message_lines(&app, 100);
    // Should be 1 line if fits
    assert_eq!(lines.len(), 1, "Expected single line for short args");

    // Verify content of first line
    let spans = &lines[0].spans;
    // Expected: tool: (magenta), test_tool (magenta bold), > start (raw), arg1 (dark gray)
    assert!(spans.iter().any(|s| s.content.contains("test_tool")));
    assert!(spans.iter().any(|s| s.content.contains("arg1")));
}

#[test]
fn test_tool_end_rendering() {
    let mut app = ChatApp::default();
    app.messages.push(ChatMessage::ToolEnd {
        name: "test_tool".to_string(),
        is_error: false,
        output: "success".to_string(),
    });

    let lines = build_message_lines(&app, 100);
    // Should be 1 line if fits
    assert_eq!(lines.len(), 1, "Expected single line for short output");

    let spans = &lines[0].spans;
    assert!(spans.iter().any(|s| s.content.contains("test_tool")));
    assert!(spans.iter().any(|s| s.content.contains("success")));
}

#[test]
fn test_tool_start_wrapping() {
    let mut app = ChatApp::default();
    let long_args = "a".repeat(200);
    app.messages.push(ChatMessage::ToolStart {
        name: "test_tool".to_string(),
        args: long_args.clone(),
    });

    let lines = build_message_lines(&app, 50);
    // Should wrap
    assert!(lines.len() > 1, "Expected multiple lines for long args");
}

#[test]
fn test_tool_end_wrapping() {
    let mut app = ChatApp::default();
    let long_output = "a".repeat(200);
    app.messages.push(ChatMessage::ToolEnd {
        name: "test_tool".to_string(),
        is_error: false,
        output: long_output.clone(),
    });

    let lines = build_message_lines(&app, 50);
    // Should wrap
    assert!(lines.len() > 1, "Expected multiple lines for long output");
}
