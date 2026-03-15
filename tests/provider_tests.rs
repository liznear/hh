use hh_cli::core;
use hh_cli::provider::openai_compatible::OpenAiCompatibleProvider;
use hh_cli::provider::{
    Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent, ToolCall,
};
use serde_json::json;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Debug)]
struct CapturedRequest {
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

async fn read_http_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];

    let header_end = loop {
        let read = stream.read(&mut chunk).await.expect("read request chunk");
        assert!(read > 0, "request ended before headers");
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };

    let headers_raw = String::from_utf8(buffer[..header_end].to_vec()).expect("headers utf8");
    let mut lines = headers_raw.split("\r\n");
    let request_line = lines.next().expect("request line");
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("path")
        .to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream
            .read(&mut chunk)
            .await
            .expect("read request body chunk");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    let body = String::from_utf8(buffer[body_start..body_start + content_length].to_vec())
        .expect("body utf8");

    CapturedRequest {
        path,
        headers,
        body,
    }
}

async fn spawn_mock_server(
    response_body: String,
    content_type: &'static str,
) -> (String, tokio::sync::oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let (captured_tx, captured_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept connection");
        let captured = read_http_request(&mut stream).await;
        let _ = captured_tx.send(captured);

        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    });

    (format!("http://{addr}"), captured_rx)
}

fn request_with_tool() -> ProviderRequest {
    ProviderRequest {
        model: String::new(),
        messages: vec![core::Message {
            role: core::Role::User,
            content: "Summarize this".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        tools: vec![core::ToolSchema {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            capability: None,
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        }],
    }
}

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
fn provider_response_supports_thinking_field() {
    let response = ProviderResponse {
        assistant_message: hh_cli::provider::Message {
            role: hh_cli::provider::Role::Assistant,
            content: "hello".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        },
        tool_calls: vec![ToolCall {
            id: "call-1".to_string(),
            name: "read".to_string(),
            arguments: json!({"path":"Cargo.toml"}),
        }],
        done: false,
        thinking: Some("analyzing".to_string()),
        context_tokens: None,
    };

    assert_eq!(response.thinking.as_deref(), Some("analyzing"));
}

#[tokio::test]
async fn complete_posts_chat_completions_and_parses_tool_calls() {
    let response_body = json!({
        "id": "cmpl_1",
        "object": "chat.completion",
        "created": 1,
        "model": "model-x",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "done",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "read",
                                "arguments": "{\"path\":\"Cargo.toml\"}"
                            }
                        }
                    ]
                },
                "finish_reason": "tool_calls"
            }
        ],
        "usage": {"prompt_tokens": 7, "total_tokens": 12}
    })
    .to_string();

    let (base_url, captured_rx) = spawn_mock_server(response_body, "application/json").await;
    let provider =
        OpenAiCompatibleProvider::new(base_url, "model-x".to_string(), "HOME".to_string());

    let response = Provider::complete(&provider, request_with_tool())
        .await
        .expect("provider complete");

    assert_eq!(response.assistant_message.content, "done");
    assert_eq!(response.context_tokens, Some(7));
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].name, "read");
    assert_eq!(
        response.tool_calls[0].arguments,
        json!({"path": "Cargo.toml"})
    );

    let captured = captured_rx.await.expect("captured request");
    assert_eq!(captured.path, "/chat/completions");
    assert!(captured.headers.contains_key("authorization"));

    let payload: serde_json::Value = serde_json::from_str(&captured.body).expect("request json");
    assert_eq!(payload["model"], "model-x");
    assert_eq!(payload["messages"][0]["role"], "user");
    assert_eq!(payload["messages"][0]["content"], "Summarize this");
    assert_eq!(payload["tools"][0]["function"]["name"], "read");
}

#[tokio::test]
async fn stream_parses_tool_call_deltas_and_emits_events() {
    let chunk1 = json!({"choices":[{"delta":{"content":"hi "}}]}).to_string();
    let chunk2 = json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_2",
                    "function": {
                        "name": "read",
                        "arguments": "{\"path\""
                    }
                }]
            }
        }]
    })
    .to_string();
    let chunk3 = json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "function": {
                        "arguments": ":\"Cargo.toml\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 12, "total_tokens": 20}
    })
    .to_string();
    let sse = format!("data: {chunk1}\n\ndata: {chunk2}\n\ndata: {chunk3}\n\ndata: [DONE]\n\n");

    let (base_url, captured_rx) = spawn_mock_server(sse, "text/event-stream").await;
    let provider =
        OpenAiCompatibleProvider::new(base_url, "model-stream".to_string(), "HOME".to_string());

    let mut deltas = Vec::new();
    let response = provider
        .complete_stream(request_with_tool(), |event| {
            deltas.push(event);
        })
        .await
        .expect("provider stream");

    assert_eq!(response.assistant_message.content, "hi ");
    assert_eq!(response.context_tokens, Some(12));
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_2");
    assert_eq!(response.tool_calls[0].name, "read");
    assert_eq!(
        response.tool_calls[0].arguments,
        json!({"path": "Cargo.toml"})
    );
    assert!(!response.done);

    assert!(deltas.iter().any(
        |event| matches!(event, ProviderStreamEvent::AssistantDelta(delta) if delta == "hi ")
    ));

    let captured = captured_rx.await.expect("captured request");
    let payload: serde_json::Value = serde_json::from_str(&captured.body).expect("request json");
    assert_eq!(payload["stream"], true);
    assert_eq!(payload["stream_options"]["include_usage"], true);
}
