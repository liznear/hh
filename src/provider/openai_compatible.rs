use crate::core::{
    Message, MessageAttachment, Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent,
    Role, ToolCall,
};
use anyhow::Context;
use async_trait::async_trait;
use futures::StreamExt;
use rig::OneOrMany;
use rig::client::CompletionClient;
use rig::completion::{CompletionModel, CompletionRequest as RigCompletionRequest, GetTokenUsage};
use rig::message as rig_message;
use rig::providers::openai;
use rig::streaming::{StreamedAssistantContent, ToolCallDeltaContent};
use std::collections::BTreeMap;
use std::env;

#[derive(Default)]
struct StreamedToolCall {
    id: String,
    name: String,
    arguments_json: String,
}

impl StreamedToolCall {
    fn into_tool_call(self) -> ToolCall {
        let arguments = serde_json::from_str(&self.arguments_json)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        ToolCall {
            id: self.id,
            name: self.name,
            arguments,
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn context_tokens(input_tokens: u64) -> Option<usize> {
    if input_tokens == 0 {
        None
    } else {
        Some(input_tokens as usize)
    }
}

fn build_provider_response(
    assistant: String,
    thinking: String,
    tool_calls: Vec<ToolCall>,
    context_tokens: Option<usize>,
) -> ProviderResponse {
    ProviderResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: assistant,
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        },
        done: tool_calls.is_empty(),
        tool_calls,
        thinking: non_empty(thinking),
        context_tokens,
    }
}

pub struct OpenAiCompatibleProvider {
    base_url: String,
    model: String,
    api_key_env: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, model: String, api_key_env: String) -> Self {
        Self {
            base_url,
            model,
            api_key_env,
        }
    }

    fn resolve_model<'a>(&'a self, req: &'a ProviderRequest) -> &'a str {
        if req.model.trim().is_empty() {
            self.model.as_str()
        } else {
            req.model.as_str()
        }
    }

    fn build_completion_model(
        &self,
        model: String,
    ) -> anyhow::Result<openai::completion::CompletionModel> {
        let api_key = env::var(&self.api_key_env)
            .with_context(|| format!("missing API key env var {}", self.api_key_env))?;
        let client = openai::CompletionsClient::builder()
            .api_key(api_key.as_str())
            .base_url(self.base_url.as_str())
            .build()
            .context("failed to build rig OpenAI-compatible client")?;

        Ok(client.completion_model(model))
    }

    fn to_rig_request(&self, req: ProviderRequest) -> anyhow::Result<RigCompletionRequest> {
        let mut preamble_parts = Vec::new();
        let mut chat_history = Vec::new();

        for message in req.messages {
            match message.role {
                Role::System => {
                    if !message.content.trim().is_empty() {
                        preamble_parts.push(message.content);
                    }
                }
                _ => {
                    if let Some(chat_message) = message_to_rig(message)? {
                        chat_history.push(chat_message);
                    }
                }
            }
        }

        let chat_history = OneOrMany::many(chat_history).map_err(|_| {
            anyhow::anyhow!("provider request requires at least one non-system message")
        })?;

        let tools = req
            .tools
            .into_iter()
            .map(|tool| rig::completion::ToolDefinition {
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect();

        Ok(RigCompletionRequest {
            model: non_empty(req.model),
            preamble: non_empty(preamble_parts.join("\n\n")),
            chat_history,
            documents: Vec::new(),
            tools,
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
        })
    }

    fn parse_completion_response(
        response: rig::completion::CompletionResponse<openai::completion::CompletionResponse>,
    ) -> ProviderResponse {
        let mut assistant = String::new();
        let mut thinking = String::new();
        let mut tool_calls = Vec::new();

        for item in response.choice {
            match item {
                rig::message::AssistantContent::Text(text) => {
                    assistant.push_str(text.text.as_str())
                }
                rig::message::AssistantContent::Reasoning(reasoning) => {
                    let text = reasoning.display_text();
                    if !text.is_empty() {
                        thinking.push_str(text.as_str());
                    }
                }
                rig::message::AssistantContent::ToolCall(tool_call) => {
                    tool_calls.push(ToolCall {
                        id: tool_call.id,
                        name: tool_call.function.name,
                        arguments: tool_call.function.arguments,
                    });
                }
                rig::message::AssistantContent::Image(_) => {}
            }
        }

        build_provider_response(
            assistant,
            thinking,
            tool_calls,
            context_tokens(response.usage.input_tokens),
        )
    }
}

fn message_to_rig(message: Message) -> anyhow::Result<Option<rig_message::Message>> {
    match message.role {
        Role::User => {
            let mut content = Vec::new();
            if !message.content.is_empty() {
                content.push(rig_message::UserContent::Text(rig_message::Text {
                    text: message.content,
                }));
            }
            for attachment in message.attachments {
                content.push(attachment.into());
            }
            let content = OneOrMany::many(content)
                .map_err(|_| anyhow::anyhow!("user message cannot be empty"))?;
            Ok(Some(rig_message::Message::User { content }))
        }
        Role::Assistant => {
            let mut content = Vec::new();
            if !message.content.is_empty() {
                content.push(rig_message::AssistantContent::Text(rig_message::Text {
                    text: message.content,
                }));
            }
            for call in message.tool_calls {
                content.push(call.into());
            }
            let content = OneOrMany::many(content)
                .map_err(|_| anyhow::anyhow!("assistant message cannot be empty"))?;
            Ok(Some(rig_message::Message::Assistant { id: None, content }))
        }
        Role::Tool => {
            let content = OneOrMany::one(rig_message::ToolResultContent::Text(rig_message::Text {
                text: message.content,
            }));
            let tool_result = rig_message::ToolResult {
                id: message.tool_call_id.unwrap_or_default(),
                call_id: None,
                content,
            };

            let content = OneOrMany::one(rig_message::UserContent::ToolResult(tool_result));
            Ok(Some(rig_message::Message::User { content }))
        }
        Role::System => Ok(None),
    }
}

impl From<MessageAttachment> for rig_message::UserContent {
    fn from(value: MessageAttachment) -> Self {
        match value {
            MessageAttachment::Image {
                media_type,
                data_base64,
            } => {
                let url = format!("data:{media_type};base64,{data_base64}");
                rig_message::UserContent::Image(rig_message::Image {
                    data: rig_message::DocumentSourceKind::Url(url),
                    media_type: None,
                    detail: None,
                    additional_params: None,
                })
            }
        }
    }
}

impl From<ToolCall> for rig_message::AssistantContent {
    fn from(value: ToolCall) -> Self {
        rig_message::AssistantContent::ToolCall(rig_message::ToolCall {
            id: value.id,
            call_id: None,
            function: rig_message::ToolFunction {
                name: value.name,
                arguments: value.arguments,
            },
            signature: None,
            additional_params: None,
        })
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
        let model_name = self.resolve_model(&req).to_string();
        let model = self.build_completion_model(model_name)?;
        let request = self.to_rig_request(req)?;
        let response = model
            .completion(request)
            .await
            .context("provider request failed")?;
        Ok(Self::parse_completion_response(response))
    }

    async fn complete_stream<F>(
        &self,
        req: ProviderRequest,
        mut on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send,
    {
        let model_name = self.resolve_model(&req).to_string();
        let model = self.build_completion_model(model_name)?;
        let request = self.to_rig_request(req)?;
        let mut stream = model
            .stream(request)
            .await
            .context("provider stream request failed")?;

        let mut assistant = String::new();
        let mut thinking = String::new();
        let mut partial_calls: BTreeMap<String, StreamedToolCall> = BTreeMap::new();
        let mut call_order = Vec::new();
        let mut context_tokens = None;

        while let Some(item) = stream.next().await {
            let item = item.context("provider stream parse failed")?;
            match item {
                StreamedAssistantContent::Text(text) => {
                    assistant.push_str(text.text.as_str());
                    on_event(ProviderStreamEvent::AssistantDelta(text.text));
                }
                StreamedAssistantContent::Reasoning(reasoning) => {
                    let delta = reasoning.display_text();
                    if !delta.is_empty() {
                        thinking.push_str(delta.as_str());
                        on_event(ProviderStreamEvent::ThinkingDelta(delta));
                    }
                }
                StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                    thinking.push_str(reasoning.as_str());
                    on_event(ProviderStreamEvent::ThinkingDelta(reasoning));
                }
                StreamedAssistantContent::ToolCall {
                    tool_call,
                    internal_call_id,
                } => {
                    if !call_order.iter().any(|id| id == internal_call_id.as_str()) {
                        call_order.push(internal_call_id.clone());
                    }

                    let entry = partial_calls.entry(internal_call_id).or_default();
                    entry.id = tool_call.id;
                    entry.name = tool_call.function.name;
                    entry.arguments_json = serde_json::to_string(&tool_call.function.arguments)
                        .unwrap_or_else(|_| "{}".to_string());
                }
                StreamedAssistantContent::ToolCallDelta {
                    id,
                    internal_call_id,
                    content,
                } => {
                    if !call_order
                        .iter()
                        .any(|existing| existing == internal_call_id.as_str())
                    {
                        call_order.push(internal_call_id.clone());
                    }

                    let entry = partial_calls.entry(internal_call_id).or_default();
                    if entry.id.is_empty() {
                        entry.id = id;
                    }

                    match content {
                        ToolCallDeltaContent::Name(name) => {
                            entry.name = name;
                        }
                        ToolCallDeltaContent::Delta(delta) => {
                            entry.arguments_json.push_str(delta.as_str());
                        }
                    }
                }
                StreamedAssistantContent::Final(final_response) => {
                    if let Some(tokens) = final_response.token_usage() {
                        context_tokens = Some(tokens.input_tokens as usize);
                    }
                }
            }
        }

        let tool_calls = call_order
            .into_iter()
            .filter_map(|key| partial_calls.remove(&key))
            .filter(|call| !call.name.is_empty())
            .map(StreamedToolCall::into_tool_call)
            .collect::<Vec<_>>();

        Ok(build_provider_response(
            assistant,
            thinking,
            tool_calls,
            context_tokens,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
