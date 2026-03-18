use crate::traits::{Provider, ToolRegistry};
use crate::types::{AgentInput, AgentOutput, Message, ProviderRequest, Role, ToolCall, ToolResult};
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc;

const CANCELLATION_ERROR: &str = "agent run cancelled";

pub fn is_cancellation_error(err: &anyhow::Error) -> bool {
    err.to_string().contains(CANCELLATION_ERROR)
}

pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_steps: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            system_prompt: String::new(),
            max_steps: 100,
        }
    }
}

pub struct AgentLoop<P, R>
where
    P: Provider,
    R: ToolRegistry,
{
    provider: P,
    tool_registry: R,
    config: AgentConfig,
    pending_tool_call_ids: HashSet<String>,
    tool_results: HashMap<String, ToolResult>,
    blocking_tools: HashSet<String>,
    ephemeral_state: Option<Message>,
}

impl<P, R> AgentLoop<P, R>
where
    P: Provider,
    R: ToolRegistry,
{
    pub fn new(provider: P, tool_registry: R, config: AgentConfig) -> Self {
        let blocking_tools = tool_registry
            .schemas()
            .iter()
            .filter(|s| s.blocking)
            .map(|s| s.name.clone())
            .collect();
        Self {
            provider,
            tool_registry,
            config,
            pending_tool_call_ids: HashSet::new(),
            tool_results: HashMap::new(),
            blocking_tools,
            ephemeral_state: None,
        }
    }

    pub fn tool_schemas(&self) -> Vec<crate::types::ToolSchema> {
        self.tool_registry.schemas()
    }

    fn is_blocking(&self, tool_name: &str) -> bool {
        self.blocking_tools.contains(tool_name)
    }

    pub async fn run<D>(
        &mut self,
        messages: &mut Vec<Message>,
        mut input_rx: mpsc::Receiver<AgentInput>,
        emit_output: &mut (impl FnMut(AgentOutput) + Send),
        mut drain_pending_messages: D,
    ) -> anyhow::Result<Option<String>>
    where
        D: FnMut() -> Vec<Message>,
    {
        let mut pending_messages: VecDeque<Message> = VecDeque::new();
        let mut cancelled = false;

        let mut first_message = None;
        while first_message.is_none() {
            match input_rx.recv().await {
                Some(AgentInput::Message(message)) => first_message = Some(message),
                Some(AgentInput::ToolResult { call_id, result }) => {
                    self.tool_results.insert(call_id, result);
                }
                Some(AgentInput::SetEphemeralState(state)) => {
                    self.ephemeral_state = state;
                }
                Some(AgentInput::Cancel) => {
                    cancelled = true;
                    break;
                }
                None => break,
            }
        }

        let Some(first_message) = first_message else {
            if cancelled {
                emit_output(AgentOutput::Cancelled);
                anyhow::bail!(CANCELLATION_ERROR)
            }
            return Ok(None);
        };

        messages.push(first_message.clone());
        emit_output(AgentOutput::MessageAdded(first_message));

        if self.should_inject_system_prompt(messages) {
            let system_message = self.system_prompt_message();
            messages.insert(0, system_message.clone());
            emit_output(AgentOutput::MessageAdded(system_message));
        }

        let mut step = 0usize;

        loop {
            if cancelled {
                emit_output(AgentOutput::Cancelled);
                anyhow::bail!(CANCELLATION_ERROR)
            }

            while let Some(message) = pending_messages.pop_front() {
                messages.push(message.clone());
                emit_output(AgentOutput::MessageAdded(message));
            }

            for message in drain_pending_messages() {
                messages.push(message.clone());
                emit_output(AgentOutput::MessageAdded(message));
            }

            if self.config.max_steps > 0 && step >= self.config.max_steps {
                anyhow::bail!("Reached max steps without final answer")
            }

            let tool_schemas = self.tool_schemas();
            let turn_result = self
                .execute_turn(
                    messages,
                    tool_schemas,
                    &mut input_rx,
                    emit_output,
                    &mut cancelled,
                )
                .await?;

            if let Some(final_content) = turn_result {
                return Ok(Some(final_content));
            }

            step += 1;

            loop {
                tokio::select! {
                    maybe_input = input_rx.recv() => {
                        match maybe_input {
                            Some(AgentInput::Message(message)) => {
                                pending_messages.push_back(message);
                            }
                            Some(AgentInput::ToolResult { call_id, result }) => {
                                self.tool_results.insert(call_id, result);
                            }
                            Some(AgentInput::SetEphemeralState(state)) => {
                                self.ephemeral_state = state;
                            }
                            Some(AgentInput::Cancel) => {
                                cancelled = true;
                                break;
                            }
                            None => {
                                cancelled = true;
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {
                        break;
                    }
                }
            }
        }
    }

    async fn execute_turn(
        &mut self,
        messages: &mut Vec<Message>,
        tool_schemas: Vec<crate::types::ToolSchema>,
        input_rx: &mut mpsc::Receiver<AgentInput>,
        emit_output: &mut (impl FnMut(AgentOutput) + Send),
        cancelled: &mut bool,
    ) -> anyhow::Result<Option<String>> {
        let mut request_messages = messages.to_vec();
        if let Some(state_message) = self.ephemeral_state.clone() {
            request_messages.push(state_message);
        }
        let req = ProviderRequest {
            model: self.config.model.clone(),
            messages: request_messages,
            tools: tool_schemas,
        };

        let response = self.execute_provider_turn(req, emit_output).await?;

        // Emit any thinking content that came in the response (not streamed as deltas)
        if let Some(ref thinking) = response.thinking {
            emit_output(AgentOutput::ThinkingDelta(thinking.clone()));
        }

        if let Some(tokens) = response.context_tokens {
            emit_output(AgentOutput::ContextUsage(tokens));
        }

        let assistant_content = response.assistant_message.content.clone();
        let assistant_message = Message {
            role: Role::Assistant,
            content: assistant_content.clone(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: response.tool_calls.clone(),
        };

        messages.push(assistant_message.clone());
        emit_output(AgentOutput::MessageAdded(assistant_message));

        if response.done {
            emit_output(AgentOutput::TurnComplete);
            return Ok(Some(assistant_content));
        }

        self.pending_tool_call_ids = response
            .tool_calls
            .iter()
            .map(|call| call.id.clone())
            .collect();

        self.process_tool_calls(response.tool_calls, input_rx, emit_output, cancelled)
            .await?;

        if self.pending_tool_call_ids.is_empty() {
            Ok(None)
        } else {
            anyhow::bail!("provider turn ended with unresolved tool call results")
        }
    }

    async fn execute_provider_turn(
        &self,
        req: ProviderRequest,
        emit_output: &mut (impl FnMut(AgentOutput) + Send),
    ) -> anyhow::Result<crate::types::ProviderResponse> {
        let (tx, mut rx) = mpsc::channel::<crate::types::ProviderStreamEvent>(1024);
        let stream_future = self.provider.complete_stream(req, move |event| {
            let _ = tx.try_send(event);
        });

        let mut assistant_content = String::new();
        let mut thinking_content = String::new();

        let mut handle_event = |event: crate::types::ProviderStreamEvent| match event {
            crate::types::ProviderStreamEvent::AssistantDelta(delta) => {
                assistant_content.push_str(&delta);
                emit_output(AgentOutput::AssistantDelta(delta));
            }
            crate::types::ProviderStreamEvent::ThinkingDelta(delta) => {
                thinking_content.push_str(&delta);
                emit_output(AgentOutput::ThinkingDelta(delta));
            }
        };

        let response = stream_future.await?;

        while let Ok(event) = rx.try_recv() {
            handle_event(event);
        }

        // Use accumulated stream content if non-empty, otherwise fall back to response content
        let final_content = if assistant_content.is_empty() {
            response.assistant_message.content.clone()
        } else {
            assistant_content
        };

        Ok(crate::types::ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: final_content,
                ..response.assistant_message
            },
            tool_calls: response.tool_calls,
            done: response.done,
            thinking: response.thinking,
            context_tokens: response.context_tokens,
        })
    }

    async fn process_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        input_rx: &mut mpsc::Receiver<AgentInput>,
        emit_output: &mut (impl FnMut(AgentOutput) + Send),
        cancelled: &mut bool,
    ) -> anyhow::Result<()> {
        for call in tool_calls {
            if *cancelled {
                emit_output(AgentOutput::Cancelled);
                anyhow::bail!(CANCELLATION_ERROR)
            }

            let is_blocking = self.is_blocking(&call.name);

            emit_output(AgentOutput::ToolCallRequested {
                call: call.clone(),
                blocking: is_blocking,
            });

            if is_blocking {
                self.wait_for_tool_result(&call.id, input_rx, emit_output, cancelled)
                    .await?;
            }
        }

        while !self.pending_tool_call_ids.is_empty() && !*cancelled {
            tokio::select! {
                maybe_input = input_rx.recv() => {
                    match maybe_input {
                        Some(AgentInput::Message(_)) => {}
                        Some(AgentInput::ToolResult { call_id, result }) => {
                            self.register_tool_result(&call_id, result)?;
                        }
                        Some(AgentInput::SetEphemeralState(state)) => {
                            self.ephemeral_state = state;
                        }
                        Some(AgentInput::Cancel) => {
                            *cancelled = true;
                            self.pending_tool_call_ids.clear();
                            emit_output(AgentOutput::Cancelled);
                            anyhow::bail!(CANCELLATION_ERROR)
                        }
                        None => {
                            *cancelled = true;
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {
                    let received_ids: Vec<_> = self.tool_results.keys().cloned().collect();
                    for call_id in received_ids {
                        if self.pending_tool_call_ids.contains(&call_id) {
                            let _ = self.register_tool_result_from_cache(&call_id);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn wait_for_tool_result(
        &mut self,
        call_id: &str,
        input_rx: &mut mpsc::Receiver<AgentInput>,
        emit_output: &mut (impl FnMut(AgentOutput) + Send),
        cancelled: &mut bool,
    ) -> anyhow::Result<()> {
        loop {
            if *cancelled {
                emit_output(AgentOutput::Cancelled);
                anyhow::bail!(CANCELLATION_ERROR)
            }

            if let Some(result) = self.tool_results.remove(call_id) {
                self.register_tool_result(call_id, result)?;
                return Ok(());
            }

            match input_rx.recv().await {
                Some(AgentInput::Message(_)) => {}
                Some(AgentInput::ToolResult {
                    call_id: result_call_id,
                    result,
                }) => {
                    if result_call_id == *call_id {
                        self.register_tool_result(call_id, result)?;
                        return Ok(());
                    } else {
                        self.tool_results.insert(result_call_id, result);
                    }
                }
                Some(AgentInput::SetEphemeralState(state)) => {
                    self.ephemeral_state = state;
                }
                Some(AgentInput::Cancel) => {
                    *cancelled = true;
                    self.pending_tool_call_ids.clear();
                    emit_output(AgentOutput::Cancelled);
                    anyhow::bail!(CANCELLATION_ERROR)
                }
                None => {
                    *cancelled = true;
                    break;
                }
            }
        }

        Ok(())
    }

    fn register_tool_result(&mut self, call_id: &str, _result: ToolResult) -> anyhow::Result<()> {
        if self.pending_tool_call_ids.remove(call_id) {
            Ok(())
        } else {
            anyhow::bail!("received tool result for unknown call_id: {call_id}")
        }
    }

    fn register_tool_result_from_cache(&mut self, call_id: &str) -> anyhow::Result<()> {
        if let Some(result) = self.tool_results.remove(call_id) {
            self.register_tool_result(call_id, result)
        } else {
            anyhow::bail!("no cached result for call_id: {call_id}")
        }
    }

    fn should_inject_system_prompt(&self, messages: &[Message]) -> bool {
        messages.iter().all(|message| message.role != Role::System)
            && !self.config.system_prompt.trim().is_empty()
    }

    fn system_prompt_message(&self) -> Message {
        Message {
            role: Role::System,
            content: self.config.system_prompt.clone(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolSchema;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct TestProvider {
        responses: Arc<Mutex<VecDeque<crate::types::ProviderResponse>>>,
    }

    #[async_trait]
    impl Provider for TestProvider {
        async fn complete(
            &self,
            _req: ProviderRequest,
        ) -> anyhow::Result<crate::types::ProviderResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted response"))
        }
    }

    struct TestToolRegistry {
        blocking_tools: HashSet<String>,
    }

    #[async_trait]
    impl ToolRegistry for TestToolRegistry {
        fn schemas(&self) -> Vec<ToolSchema> {
            self.blocking_tools
                .iter()
                .map(|name| ToolSchema {
                    name: name.clone(),
                    description: format!("{name} tool"),
                    capability: None,
                    mutating: None,
                    blocking: true,
                    parameters: serde_json::json!({}),
                })
                .collect()
        }

        fn is_blocking(&self, tool_name: &str) -> bool {
            self.blocking_tools.contains(tool_name)
        }
    }

    fn user_message(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    fn tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: json!({}),
        }
    }

    fn response_with_tool_calls(
        calls: Vec<ToolCall>,
        done: bool,
    ) -> crate::types::ProviderResponse {
        crate::types::ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: String::new(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: calls.clone(),
            },
            tool_calls: calls,
            done,
            thinking: None,
            context_tokens: None,
        }
    }

    fn final_response(content: &str) -> crate::types::ProviderResponse {
        crate::types::ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: content.to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
            tool_calls: Vec::new(),
            done: true,
            thinking: None,
            context_tokens: None,
        }
    }

    #[tokio::test]
    async fn returns_final_answer_when_no_tool_calls() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![final_response("done")]))),
        };
        let registry = TestToolRegistry {
            blocking_tools: HashSet::new(),
        };

        let mut agent = AgentLoop::new(
            provider,
            registry,
            AgentConfig {
                model: "test".to_string(),
                system_prompt: String::new(),
                max_steps: 10,
            },
        );

        let (tx, rx) = mpsc::channel(10);
        tx.send(AgentInput::Message(user_message("hello")))
            .await
            .unwrap();

        let mut messages = Vec::new();
        let mut outputs = Vec::new();
        let result = agent
            .run(&mut messages, rx, &mut |o| outputs.push(o), &mut Vec::new)
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("done".to_string()));
    }

    #[tokio::test]
    async fn processes_blocking_tools_sequentially() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![
                response_with_tool_calls(
                    vec![tool_call("1", "bash"), tool_call("2", "bash")],
                    false,
                ),
                final_response("done"),
            ]))),
        };
        let registry = TestToolRegistry {
            blocking_tools: vec!["bash".to_string()].into_iter().collect(),
        };

        let mut agent = AgentLoop::new(
            provider,
            registry,
            AgentConfig {
                model: "test".to_string(),
                system_prompt: String::new(),
                max_steps: 10,
            },
        );

        let (tx, rx) = mpsc::channel(10);
        tx.send(AgentInput::Message(user_message("run")))
            .await
            .unwrap();

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx_clone
                .send(AgentInput::ToolResult {
                    call_id: "1".to_string(),
                    result: ToolResult::ok_text("ok", "result 1"),
                })
                .await
                .unwrap();
            tx_clone
                .send(AgentInput::ToolResult {
                    call_id: "2".to_string(),
                    result: ToolResult::ok_text("ok", "result 2"),
                })
                .await
                .unwrap();
        });

        let mut messages = Vec::new();
        let mut outputs = Vec::new();
        let result = agent
            .run(&mut messages, rx, &mut |o| outputs.push(o), &mut Vec::new)
            .await;

        assert!(result.is_ok());
        let tool_requests: Vec<_> = outputs
            .iter()
            .filter_map(|o| match o {
                AgentOutput::ToolCallRequested { call, blocking } => {
                    Some((call.id.clone(), *blocking))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            tool_requests,
            vec![("1".to_string(), true), ("2".to_string(), true)]
        );
    }

    #[tokio::test]
    async fn cancellation_emits_cancelled_output() {
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::new())),
        };
        let registry = TestToolRegistry {
            blocking_tools: HashSet::new(),
        };

        let mut agent = AgentLoop::new(provider, registry, AgentConfig::default());

        let (tx, rx) = mpsc::channel(10);
        tx.send(AgentInput::Cancel).await.unwrap();

        let mut messages = Vec::new();
        let mut outputs = Vec::new();
        let result = agent
            .run(&mut messages, rx, &mut |o| outputs.push(o), &mut Vec::new)
            .await;

        assert!(result.is_err());
        assert!(is_cancellation_error(&result.unwrap_err()));
        assert!(outputs.iter().any(|o| matches!(o, AgentOutput::Cancelled)));
    }
}
