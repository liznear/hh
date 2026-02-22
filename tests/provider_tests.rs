use hh::provider::openai_compatible::OpenAiCompatibleProvider;
use hh::provider::{ProviderResponse, StreamedToolCall, ToolCall};
use serde_json::json;

#[test]
fn provider_endpoint_normalizes_trailing_slash() {
    let provider = OpenAiCompatibleProvider::new(
        "https://example.com/v1/".to_string(),
        "model-x".to_string(),
        "OPENAI_API_KEY".to_string(),
    );

    let debug = format!("{:?}", std::any::type_name_of_val(&provider));
    assert!(debug.contains("OpenAiCompatibleProvider"));
}

#[test]
fn streamed_tool_call_builds_arguments_json() {
    let call = StreamedToolCall {
        id: "call-1".to_string(),
        name: "read".to_string(),
        arguments_json: "{\"path\":\"Cargo.toml\"}".to_string(),
    }
    .into_tool_call();

    assert_eq!(call.id, "call-1");
    assert_eq!(call.name, "read");
    assert_eq!(call.arguments, json!({"path":"Cargo.toml"}));
}

#[test]
fn streamed_tool_call_invalid_json_falls_back_to_object() {
    let call = StreamedToolCall {
        id: "call-2".to_string(),
        name: "bash".to_string(),
        arguments_json: "{".to_string(),
    }
    .into_tool_call();

    assert_eq!(call.arguments, json!({}));
}

#[test]
fn provider_response_supports_thinking_field() {
    let response = ProviderResponse {
        assistant_message: hh::provider::Message {
            role: hh::provider::Role::Assistant,
            content: "hello".to_string(),
            tool_call_id: None,
        },
        tool_calls: vec![ToolCall {
            id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: json!({"path":"Cargo.toml"}),
        }],
        done: false,
        thinking: Some("analyzing".to_string()),
    };

    assert_eq!(response.thinking.as_deref(), Some("analyzing"));
}
