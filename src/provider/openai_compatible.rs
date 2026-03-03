use crate::core::{
    Message, MessageAttachment, Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent,
    Role, ToolCall,
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

    fn request_body(
        &self,
        req: &ProviderRequest,
        stream: bool,
        image_url_as_object: bool,
        image_data_format: ImageDataFormat,
        include_tools: bool,
    ) -> Value {
        let requested_model = if req.model.is_empty() {
            self.model.as_str()
        } else {
            req.model.as_str()
        };

        let tools = if include_tools {
            req.tools
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
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let messages = messages_to_wire(&req.messages, image_url_as_object, image_data_format);

        let mut body = json!({
            "model": requested_model,
            "messages": messages,
            "stream": stream,
        });
        if include_tools && !tools.is_empty() {
            body["tools"] = json!(tools);
            body["tool_choice"] = json!("auto");
        }
        body
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
        let context_tokens = parse_context_tokens(value);

        Ok(ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content,
                attachments: Vec::new(),
                tool_call_id: None,
            },
            done: tool_calls.is_empty(),
            tool_calls,
            thinking,
            context_tokens,
        })
    }

    async fn send_request(
        &self,
        req: &ProviderRequest,
        stream: bool,
        error_context: &str,
    ) -> anyhow::Result<reqwest::Response> {
        struct RequestAttempt {
            label: &'static str,
            image_url_object: bool,
            image_data_format: ImageDataFormat,
            include_tools: bool,
            requires_retryable_previous_error: bool,
        }

        let mut attempts = vec![RequestAttempt {
            label: "primary",
            image_url_object: true,
            image_data_format: ImageDataFormat::DataUrl,
            include_tools: true,
            requires_retryable_previous_error: false,
        }];

        if has_image_attachments(req) {
            attempts.extend([
                RequestAttempt {
                    label: "fallback_no_tools",
                    image_url_object: true,
                    image_data_format: ImageDataFormat::DataUrl,
                    include_tools: false,
                    requires_retryable_previous_error: true,
                },
                RequestAttempt {
                    label: "fallback_raw_base64",
                    image_url_object: true,
                    image_data_format: ImageDataFormat::RawBase64,
                    include_tools: false,
                    requires_retryable_previous_error: true,
                },
                RequestAttempt {
                    label: "fallback_string_image_url",
                    image_url_object: false,
                    image_data_format: ImageDataFormat::DataUrl,
                    include_tools: false,
                    requires_retryable_previous_error: false,
                },
            ]);
        }

        let mut failures: Vec<(String, reqwest::StatusCode, String)> = Vec::new();
        for attempt in attempts {
            if attempt.requires_retryable_previous_error
                && !failures
                    .last()
                    .is_some_and(|(_, status, body)| should_retry_for_image_payload(*status, body))
            {
                break;
            }

            let body = self.request_body(
                req,
                stream,
                attempt.image_url_object,
                attempt.image_data_format,
                attempt.include_tools,
            );
            let context = if attempt.label == "primary" {
                error_context.to_string()
            } else {
                format!("{} ({})", error_context, attempt.label)
            };

            let response = self
                .client
                .post(self.endpoint())
                .headers(self.auth_headers()?)
                .json(&body)
                .send()
                .await
                .with_context(|| context)?;

            if response.status().is_success() {
                return Ok(response);
            }

            let status = response.status();
            let error = response.text().await.unwrap_or_default();
            failures.push((attempt.label.to_string(), status, error));
        }

        if failures.is_empty() {
            bail!("provider request failed without attempts")
        }

        let mut details = String::new();
        for (idx, (label, status, body)) in failures.iter().enumerate() {
            if idx > 0 {
                details.push(' ');
            }
            details.push_str(&format!("({label} {status}: {body})"));
        }

        bail!("provider request failed {details}")
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
        let mut context_tokens = None;

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
                    Some(StreamLine::Payload(value)) => {
                        if let Some(tokens) = parse_context_tokens(&value) {
                            context_tokens = Some(tokens);
                        }
                        apply_stream_chunk(
                            &value,
                            &mut assistant,
                            &mut thinking,
                            &mut partial_calls,
                            &mut on_event,
                        )
                    }
                    None => continue,
                }
            }
        }

        if !stream_done {
            match parse_stream_line(buffer.trim()) {
                Some(StreamLine::Payload(value)) => {
                    if let Some(tokens) = parse_context_tokens(&value) {
                        context_tokens = Some(tokens);
                    }
                    apply_stream_chunk(
                        &value,
                        &mut assistant,
                        &mut thinking,
                        &mut partial_calls,
                        &mut on_event,
                    )
                }
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
                attachments: Vec::new(),
                tool_call_id: None,
            },
            done: tool_calls.is_empty(),
            tool_calls,
            thinking: if thinking.is_empty() {
                None
            } else {
                Some(thinking)
            },
            context_tokens,
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

#[derive(Clone, Copy)]
enum ImageDataFormat {
    DataUrl,
    RawBase64,
}

fn message_to_wire(
    message: &Message,
    image_url_as_object: bool,
    image_data_format: ImageDataFormat,
) -> Value {
    let content = if message.attachments.is_empty() {
        json!(message.content)
    } else {
        let mut parts = Vec::new();
        if !message.content.is_empty() {
            parts.push(json!({
                "type": "text",
                "text": message.content,
            }));
        }

        for attachment in &message.attachments {
            match attachment {
                MessageAttachment::Image {
                    media_type,
                    data_base64,
                } => {
                    let image_payload = match image_data_format {
                        ImageDataFormat::DataUrl => {
                            format!("data:{};base64,{}", media_type, data_base64)
                        }
                        ImageDataFormat::RawBase64 => data_base64.clone(),
                    };
                    if image_url_as_object {
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": image_payload,
                            }
                        }));
                    } else {
                        parts.push(json!({
                            "type": "image_url",
                            "image_url": image_payload,
                        }));
                    }
                }
            }
        }

        json!(parts)
    };

    let mut wire = json!({
        "role": role_to_wire(&message.role),
        "content": content,
    });
    if let Some(id) = &message.tool_call_id {
        wire["tool_call_id"] = json!(id);
    }
    wire
}

fn messages_to_wire(
    messages: &[Message],
    image_url_as_object: bool,
    image_data_format: ImageDataFormat,
) -> Vec<Value> {
    let mut wire_messages = Vec::with_capacity(messages.len());
    let mut known_tool_call_ids = std::collections::HashSet::<String>::new();

    for message in messages {
        if matches!(message.role, Role::Tool)
            && let Some(call_id) = message
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
            && !known_tool_call_ids.contains(call_id)
        {
            wire_messages.push(synthetic_assistant_tool_call(call_id));
            known_tool_call_ids.insert(call_id.to_string());
        }

        wire_messages.push(message_to_wire(
            message,
            image_url_as_object,
            image_data_format,
        ));
    }

    wire_messages
}

fn synthetic_assistant_tool_call(call_id: &str) -> Value {
    json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [
            {
                "id": call_id,
                "type": "function",
                "function": {
                    "name": "tool_result",
                    "arguments": "{}"
                }
            }
        ]
    })
}

fn has_image_attachments(req: &ProviderRequest) -> bool {
    req.messages
        .iter()
        .any(|message| !message.attachments.is_empty())
}

fn should_retry_for_image_payload(status: reqwest::StatusCode, body: &str) -> bool {
    if !status.is_client_error() {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("invalid api parameter")
        || lower.contains("invalid parameter")
        || lower.contains("image_url")
        || lower.contains("invalid type")
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

fn parse_context_tokens(payload: &Value) -> Option<usize> {
    let usage = payload.get("usage")?.as_object()?;
    usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .or_else(|| usage.get("total_tokens"))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_to_wire_inserts_synthetic_assistant_for_tool_message() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "hello".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            Message {
                role: Role::Tool,
                content: "ok".to_string(),
                attachments: Vec::new(),
                tool_call_id: Some("call_123".to_string()),
            },
        ];

        let wire = messages_to_wire(&messages, true, ImageDataFormat::DataUrl);
        assert_eq!(wire.len(), 3);
        assert_eq!(wire[1]["role"], "assistant");
        assert_eq!(wire[1]["tool_calls"][0]["id"], "call_123");
        assert_eq!(wire[2]["role"], "tool");
        assert_eq!(wire[2]["tool_call_id"], "call_123");
    }

    #[test]
    fn messages_to_wire_skips_synthetic_when_tool_call_id_missing() {
        let messages = vec![Message {
            role: Role::Tool,
            content: "ok".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
        }];

        let wire = messages_to_wire(&messages, true, ImageDataFormat::DataUrl);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0]["role"], "tool");
        assert!(wire[0].get("tool_call_id").is_none());
    }
}
