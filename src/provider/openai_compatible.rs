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

        let messages = req
            .messages
            .iter()
            .map(|message| message_to_wire(message, image_url_as_object, image_data_format))
            .collect::<Vec<_>>();

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
        })
    }

    async fn send_request(
        &self,
        req: &ProviderRequest,
        stream: bool,
        error_context: &str,
    ) -> anyhow::Result<reqwest::Response> {
        let primary_body = self.request_body(req, stream, true, ImageDataFormat::DataUrl, true);

        let primary = self
            .client
            .post(self.endpoint())
            .headers(self.auth_headers()?)
            .json(&primary_body)
            .send()
            .await
            .with_context(|| error_context.to_string())?;

        if primary.status().is_success() {
            return Ok(primary);
        }

        let primary_status = primary.status();
        let primary_error = primary.text().await.unwrap_or_default();
        if has_image_attachments(req)
            && should_retry_for_image_payload(primary_status, &primary_error)
        {
            let no_tools_body =
                self.request_body(req, stream, true, ImageDataFormat::DataUrl, false);
            let fallback_no_tools = self
                .client
                .post(self.endpoint())
                .headers(self.auth_headers()?)
                .json(&no_tools_body)
                .send()
                .await
                .with_context(|| format!("{} (fallback: no tools)", error_context))?;

            if fallback_no_tools.status().is_success() {
                return Ok(fallback_no_tools);
            }

            let no_tools_status = fallback_no_tools.status();
            let no_tools_error = fallback_no_tools.text().await.unwrap_or_default();

            if should_retry_for_image_payload(no_tools_status, &no_tools_error) {
                let raw_base64_body =
                    self.request_body(req, stream, true, ImageDataFormat::RawBase64, false);
                let fallback_raw_base64 = self
                    .client
                    .post(self.endpoint())
                    .headers(self.auth_headers()?)
                    .json(&raw_base64_body)
                    .send()
                    .await
                    .with_context(|| format!("{} (fallback: raw base64)", error_context))?;

                if fallback_raw_base64.status().is_success() {
                    return Ok(fallback_raw_base64);
                }

                let raw_base64_status = fallback_raw_base64.status();
                let raw_base64_error = fallback_raw_base64.text().await.unwrap_or_default();

                let string_image_body =
                    self.request_body(req, stream, false, ImageDataFormat::DataUrl, false);
                let fallback_string_image = self
                    .client
                    .post(self.endpoint())
                    .headers(self.auth_headers()?)
                    .json(&string_image_body)
                    .send()
                    .await
                    .with_context(|| format!("{} (fallback: string image_url)", error_context))?;

                if fallback_string_image.status().is_success() {
                    return Ok(fallback_string_image);
                }

                let string_status = fallback_string_image.status();
                let string_error = fallback_string_image.text().await.unwrap_or_default();
                bail!(
                    "provider error {}: {} (fallback_no_tools {}: {}) (fallback_raw_base64 {}: {}) (fallback_string_image_url {}: {})",
                    primary_status,
                    primary_error,
                    no_tools_status,
                    no_tools_error,
                    raw_base64_status,
                    raw_base64_error,
                    string_status,
                    string_error
                );
            }

            bail!(
                "provider error {}: {} (fallback_no_tools {}: {})",
                primary_status,
                primary_error,
                no_tools_status,
                no_tools_error
            );
        }

        bail!("provider error {}: {}", primary_status, primary_error)
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
