use crate::core::{
    Message, Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent, Role, ToolCall,
};
use crate::provider::StreamedToolCall;
use anyhow::{Context, bail};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use std::env;

const THINKING_FIELDS: [&str; 3] = ["reasoning", "thinking", "reasoning_content"];

pub struct OpenAiCompatibleProvider {
    base_url: String,
    model: String,
    api_key_env: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, model: String, api_key_env: String) -> Self {
        Self {
            base_url,
            model,
            api_key_env,
            client: reqwest::Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn auth_headers(&self) -> anyhow::Result<HeaderMap> {
        let api_key = env::var(&self.api_key_env)
            .with_context(|| format!("missing API key env var {}", self.api_key_env))?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))?,
        );
        Ok(headers)
    }

    fn request_body(&self, req: &ProviderRequest, stream: bool) -> Value {
        let model = if req.model.is_empty() {
            self.model.as_str()
        } else {
            req.model.as_str()
        };

        let tools = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect::<Vec<_>>();

        let messages = req.messages.iter().map(message_to_wire).collect::<Vec<_>>();

        json!({
            "model": model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
            "stream": stream,
        })
    }

    fn parse_chat_response(value: &Value) -> anyhow::Result<ProviderResponse> {
        let choice = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .context("provider response missing choices[0]")?;

        let message = choice
            .get("message")
            .and_then(|m| m.as_object())
            .context("provider response missing message")?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();

        let thinking = extract_thinking(message);
        let tool_calls = parse_tool_calls(message)?;

        Ok(ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content,
                tool_call_id: None,
            },
            done: tool_calls.is_empty(),
            tool_calls,
            thinking,
        })
    }

    async fn send_request(
        &self,
        req: &ProviderRequest,
        stream: bool,
        error_context: &str,
    ) -> anyhow::Result<reqwest::Response> {
        let response = self
            .client
            .post(self.endpoint())
            .headers(self.auth_headers()?)
            .json(&self.request_body(req, stream))
            .send()
            .await
            .with_context(|| error_context.to_string())?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("provider error {}: {}", status, body);
        }

        Ok(response)
    }

    async fn complete_stream_inner<F>(
        &self,
        req: &ProviderRequest,
        mut on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send,
    {
        let response = self
            .send_request(req, true, "provider stream request failed")
            .await?;

        let mut assistant = String::new();
        let mut thinking = String::new();
        let mut partial_calls: Vec<StreamedToolCall> = Vec::new();
        let mut stream_done = false;

        let mut buffer = String::new();
        let mut resp = response;
        while !stream_done && let Some(chunk) = resp.chunk().await.context("stream read failed")? {
            let txt = String::from_utf8_lossy(&chunk);
            buffer.push_str(&txt);

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end_matches('\r').to_string();
                buffer.drain(..=pos);

                match parse_stream_line(&line) {
                    Some(StreamLine::Done) => {
                        stream_done = true;
                        break;
                    }
                    Some(StreamLine::Payload(value)) => apply_stream_chunk(
                        &value,
                        &mut assistant,
                        &mut thinking,
                        &mut partial_calls,
                        &mut on_event,
                    ),
                    None => continue,
                }
            }
        }

        if !stream_done {
            match parse_stream_line(buffer.trim()) {
                Some(StreamLine::Payload(value)) => apply_stream_chunk(
                    &value,
                    &mut assistant,
                    &mut thinking,
                    &mut partial_calls,
                    &mut on_event,
                ),
                Some(StreamLine::Done) | None => {}
            }
        }

        let tool_calls = partial_calls
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .map(StreamedToolCall::into_tool_call)
            .collect::<Vec<_>>();

        Ok(ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: assistant,
                tool_call_id: None,
            },
            done: tool_calls.is_empty(),
            tool_calls,
            thinking: if thinking.is_empty() {
                None
            } else {
                Some(thinking)
            },
        })
    }
}

fn emit_response_stream_events<F>(response: &ProviderResponse, on_event: &mut F)
where
    F: FnMut(ProviderStreamEvent) + Send,
{
    if let Some(thinking) = &response.thinking {
        on_event(ProviderStreamEvent::ThinkingDelta(thinking.clone()));
    }
    if !response.assistant_message.content.is_empty() {
        on_event(ProviderStreamEvent::AssistantDelta(
            response.assistant_message.content.clone(),
        ));
    }
}

enum StreamLine {
    Done,
    Payload(Value),
}

fn message_to_wire(message: &Message) -> Value {
    let mut wire = json!({
        "role": role_to_wire(&message.role),
        "content": message.content,
    });
    if let Some(id) = &message.tool_call_id {
        wire["tool_call_id"] = json!(id);
    }
    wire
}

fn role_to_wire(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn parse_stream_line(line: &str) -> Option<StreamLine> {
    let line = line.trim();
    if line.is_empty() || !line.starts_with("data:") {
        return None;
    }

    let payload = line.trim_start_matches("data:").trim();
    if payload == "[DONE]" {
        return Some(StreamLine::Done);
    }

    serde_json::from_str(payload).ok().map(StreamLine::Payload)
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
        let response = self
            .send_request(&req, false, "provider request failed")
            .await?;

        let value: Value = response.json().await.context("invalid provider JSON")?;
        Self::parse_chat_response(&value)
    }

    async fn complete_stream<F>(
        &self,
        req: ProviderRequest,
        mut on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send,
    {
        match self.complete_stream_inner(&req, &mut on_event).await {
            Ok(response) => Ok(response),
            Err(_) => {
                let response = self.complete(req).await?;
                emit_response_stream_events(&response, &mut on_event);
                Ok(response)
            }
        }
    }
}

fn parse_tool_calls(message: &serde_json::Map<String, Value>) -> anyhow::Result<Vec<ToolCall>> {
    let mut tool_calls = Vec::new();
    if let Some(calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for call in calls {
            let id = call
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let function = call
                .get("function")
                .and_then(|v| v.as_object())
                .context("tool call missing function")?;
            let name = function
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let args_raw = function
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let arguments: Value = serde_json::from_str(args_raw).unwrap_or_else(|_| json!({}));
            tool_calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }
    }
    Ok(tool_calls)
}

fn extract_thinking(message: &serde_json::Map<String, Value>) -> Option<String> {
    THINKING_FIELDS.iter().find_map(|k| {
        message
            .get(*k)
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
    })
}

fn apply_stream_chunk<F>(
    value: &Value,
    assistant: &mut String,
    thinking: &mut String,
    partial_calls: &mut Vec<StreamedToolCall>,
    on_event: &mut F,
) where
    F: FnMut(ProviderStreamEvent) + Send,
{
    let Some(choice) = value
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    else {
        return;
    };

    let Some(delta) = choice.get("delta").and_then(|v| v.as_object()) else {
        return;
    };

    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
        assistant.push_str(content);
        on_event(ProviderStreamEvent::AssistantDelta(content.to_string()));
    }

    for key in THINKING_FIELDS {
        if let Some(text) = delta.get(key).and_then(|v| v.as_str()) {
            thinking.push_str(text);
            on_event(ProviderStreamEvent::ThinkingDelta(text.to_string()));
        }
    }

    if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
        for call in tool_calls {
            let index = call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            while partial_calls.len() <= index {
                partial_calls.push(StreamedToolCall::default());
            }

            let entry = &mut partial_calls[index];
            if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                entry.id = id.to_string();
            }
            if let Some(function) = call.get("function").and_then(|v| v.as_object()) {
                if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                    entry.name = name.to_string();
                }
                if let Some(args_piece) = function.get("arguments").and_then(|v| v.as_str()) {
                    entry.arguments_json.push_str(args_piece);
                }
            }
        }
    }
}
