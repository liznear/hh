use hh::agent::AgentEvents;
use hh::cli::render::{LiveRender, ThinkingMode, format_args_preview, truncate_text};
use serde_json::json;

#[test]
fn thinking_toggle_switches_modes() {
    let render = LiveRender::new();
    assert_eq!(render.thinking_mode(), ThinkingMode::Collapsed);
    assert_eq!(render.toggle_thinking_mode(), ThinkingMode::Expanded);
    assert_eq!(render.toggle_thinking_mode(), ThinkingMode::Collapsed);
}

#[test]
fn args_preview_is_compact_json_and_truncated() {
    let args = json!({"path":"/tmp/file.txt","recursive":true});
    let preview = format_args_preview(&args, 10);
    let char_len = preview.chars().count();
    assert!(char_len <= 11);
    assert!(preview.ends_with('…') || char_len <= 10);
}

#[test]
fn truncate_text_truncates_on_char_boundary() {
    let input = "αβγδε";
    let output = truncate_text(input, 3);
    assert_eq!(output, "αβγ…");
}

#[test]
fn live_render_accepts_event_callbacks() {
    let render = LiveRender::new();
    render.begin_turn();
    render.on_thinking("planning");
    render.on_tool_start("read", &json!({"path":"Cargo.toml"}));
    render.on_tool_end("read", false, "ok");
    render.on_assistant_delta("hello");
    render.on_assistant_done();
}
