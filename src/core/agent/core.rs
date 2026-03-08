use crate::core::{
    Message, Provider, ProviderRequest, ProviderStreamEvent, Role, ToolCall, ToolSchema,
};
use std::collections::HashSet;

use super::types::{CoreInput, CoreOutput, ErrorPayload};

#[derive(Debug, Clone)]
pub struct CoreTurnResult {
    pub assistant_content: String,
    pub assistant_message: Message,
    pub tool_calls: Vec<ToolCall>,
    pub done: bool,
    pub context_tokens: Option<usize>,
}

pub struct AgentCore<'a, P>
where
    P: Provider,
{
    provider: &'a P,
    model: String,
    system_prompt: String,
    tool_schemas: Vec<ToolSchema>,
    max_steps: usize,
    pending_tool_call_ids: HashSet<String>,
    ephemeral_state: Option<Message>,
}

impl<'a, P> AgentCore<'a, P>
where
    P: Provider,
{
    pub fn new(
        provider: &'a P,
        model: String,
        system_prompt: String,
        tool_schemas: Vec<ToolSchema>,
        max_steps: usize,
    ) -> Self {
        Self {
            provider,
            model,
            system_prompt,
            tool_schemas,
            max_steps,
            pending_tool_call_ids: HashSet::new(),
            ephemeral_state: None,
        }
    }

    pub fn max_steps(&self) -> usize {
        self.max_steps
    }

    pub fn should_inject_system_prompt(&self, messages: &[Message]) -> bool {
        messages.iter().all(|message| message.role != Role::System)
            && !self.system_prompt.trim().is_empty()
    }

    pub fn system_prompt_message(&self) -> Message {
        Message {
            role: Role::System,
            content: self.system_prompt.clone(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn request_messages(&self, messages: &[Message]) -> Vec<Message> {
        let mut request_messages = messages.to_vec();
        if let Some(state_message) = self.ephemeral_state.clone() {
            request_messages.push(state_message);
        }
        request_messages
    }

    pub fn register_tool_result(&mut self, call_id: &str) -> anyhow::Result<()> {
        if self.pending_tool_call_ids.remove(call_id) {
            return Ok(());
        }

        anyhow::bail!("received tool result for unknown call_id: {call_id}")
    }

    pub fn has_pending_tool_calls(&self) -> bool {
        !self.pending_tool_call_ids.is_empty()
    }

    pub fn pending_tool_call_count(&self) -> usize {
        self.pending_tool_call_ids.len()
    }

    pub fn cancel_pending_tool_calls(&mut self) {
        self.pending_tool_call_ids.clear();
    }

    pub fn handle_input(&mut self, input: CoreInput) -> anyhow::Result<()> {
        match input {
            CoreInput::ToolResult { call_id, .. } => self.register_tool_result(&call_id),
            CoreInput::SetEphemeralState(state) => {
                self.ephemeral_state = state;
                Ok(())
            }
            CoreInput::Cancel => {
                self.cancel_pending_tool_calls();
                Ok(())
            }
        }
    }

    pub async fn complete_turn<F>(
        &mut self,
        messages: &[Message],
        mut emit: F,
    ) -> anyhow::Result<CoreTurnResult>
    where
        F: FnMut(CoreOutput) + Send,
    {
        if self.has_pending_tool_calls() {
            anyhow::bail!("cannot start next provider turn while tool results are still pending")
        }

        let req = ProviderRequest {
            model: self.model.clone(),
            messages: self.request_messages(messages),
            tools: self.tool_schemas.clone(),
        };

        let mut assistant_content = String::new();
        let mut thinking_content = String::new();
        let response = match self
            .provider
            .complete_stream(req, |event| match event {
                ProviderStreamEvent::AssistantDelta(delta) => {
                    assistant_content.push_str(&delta);
                    emit(CoreOutput::AssistantDelta(delta));
                }
                ProviderStreamEvent::ThinkingDelta(delta) => {
                    thinking_content.push_str(&delta);
                    emit(CoreOutput::ThinkingDelta(delta));
                }
            })
            .await
        {
            Ok(response) => response,
            Err(err) => {
                emit(CoreOutput::Error(ErrorPayload {
                    message: err.to_string(),
                }));
                return Err(err);
            }
        };

        if let Some(tokens) = response.context_tokens {
            emit(CoreOutput::ContextUsage(tokens));
        }

        if assistant_content.is_empty() {
            assistant_content = response.assistant_message.content.clone();
            if !assistant_content.is_empty() {
                emit(CoreOutput::AssistantDelta(assistant_content.clone()));
            }
        }

        if thinking_content.is_empty()
            && let Some(thinking) = &response.thinking
            && !thinking.is_empty()
        {
            emit(CoreOutput::ThinkingDelta(thinking.clone()));
        }

        let assistant = Message {
            role: Role::Assistant,
            content: assistant_content.clone(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: response.tool_calls.clone(),
        };

        emit(CoreOutput::MessageAdded(assistant.clone()));

        if response.done {
            emit(CoreOutput::TurnComplete);
            self.pending_tool_call_ids.clear();
        } else {
            self.pending_tool_call_ids = response
                .tool_calls
                .iter()
                .map(|call| call.id.clone())
                .collect();
            for call in &response.tool_calls {
                emit(CoreOutput::ToolCallRequested(call.clone()));
            }
        }

        Ok(CoreTurnResult {
            assistant_content,
            assistant_message: assistant,
            tool_calls: response.tool_calls,
            done: response.done,
            context_tokens: response.context_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Message, ProviderRequest, ProviderResponse, Role, ToolCall};
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestProvider {
        responses: Arc<Mutex<VecDeque<ProviderResponse>>>,
    }

    #[async_trait]
    impl Provider for TestProvider {
        async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            self.responses
                .lock()
                .expect("provider lock")
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted provider response remaining"))
        }
    }

    fn user_message() -> Message {
        Message {
            role: Role::User,
            content: "hello".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    #[tokio::test]
    async fn complete_turn_blocks_next_turn_until_all_tool_results_arrive() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: vec![
                            ToolCall {
                                id: "call-1".to_string(),
                                name: "todo_write".to_string(),
                                arguments: json!({}),
                            },
                            ToolCall {
                                id: "call-2".to_string(),
                                name: "todo_read".to_string(),
                                arguments: json!({}),
                            },
                        ],
                    },
                    tool_calls: vec![
                        ToolCall {
                            id: "call-1".to_string(),
                            name: "todo_write".to_string(),
                            arguments: json!({}),
                        },
                        ToolCall {
                            id: "call-2".to_string(),
                            name: "todo_read".to_string(),
                            arguments: json!({}),
                        },
                    ],
                    done: false,
                    thinking: None,
                    context_tokens: None,
                },
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: "done".to_string(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    },
                    tool_calls: Vec::new(),
                    done: true,
                    thinking: None,
                    context_tokens: None,
                },
            ]))),
        };

        let mut core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let messages = vec![user_message()];

        let first_turn = core.complete_turn(&messages, |_output| {}).await;
        assert!(first_turn.is_ok());
        assert!(core.has_pending_tool_calls());
        assert_eq!(core.pending_tool_call_count(), 2);

        let blocked = core.complete_turn(&messages, |_output| {}).await;
        assert!(blocked.is_err());

        core.register_tool_result("call-1")
            .expect("register first tool result");
        assert!(core.has_pending_tool_calls());

        let still_blocked = core.complete_turn(&messages, |_output| {}).await;
        assert!(still_blocked.is_err());

        core.register_tool_result("call-2")
            .expect("register second tool result");
        assert!(!core.has_pending_tool_calls());

        let second_turn = core.complete_turn(&messages, |_output| {}).await;
        assert!(second_turn.is_ok());
    }

    #[test]
    fn register_tool_result_rejects_unknown_call_id() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::new())),
        };
        let mut core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);

        let result = core.register_tool_result("missing-call");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn handle_input_tool_result_acknowledges_pending_call() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "todo_read".to_string(),
                        arguments: json!({}),
                    }],
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "todo_read".to_string(),
                    arguments: json!({}),
                }],
                done: false,
                thinking: None,
                context_tokens: None,
            }]))),
        };
        let mut core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);

        core.complete_turn(&[user_message()], |_output| {})
            .await
            .expect("complete turn");
        assert!(core.has_pending_tool_calls());

        core.handle_input(CoreInput::ToolResult {
            call_id: "call-1".to_string(),
            name: "todo_read".to_string(),
            result: crate::tool::ToolResult::ok_text("ok", "ok"),
        })
        .expect("ack call id");

        assert!(!core.has_pending_tool_calls());
    }

    #[tokio::test]
    async fn handle_input_cancel_clears_pending_calls() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "todo_read".to_string(),
                        arguments: json!({}),
                    }],
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "todo_read".to_string(),
                    arguments: json!({}),
                }],
                done: false,
                thinking: None,
                context_tokens: None,
            }]))),
        };
        let mut core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);

        core.complete_turn(&[user_message()], |_output| {})
            .await
            .expect("complete turn");
        assert!(core.has_pending_tool_calls());

        core.handle_input(CoreInput::Cancel)
            .expect("cancel pending calls");

        assert!(!core.has_pending_tool_calls());
    }

    #[tokio::test]
    async fn complete_turn_emits_error_output_on_provider_failure() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::new())),
        };
        let mut core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);

        let mut outputs = Vec::new();
        let result = core
            .complete_turn(&[user_message()], |output| outputs.push(output))
            .await;

        assert!(result.is_err());
        assert!(
            outputs
                .iter()
                .any(|output| matches!(output, CoreOutput::Error(_)))
        );
    }
}
