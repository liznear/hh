pub mod state;

pub use super::{AgentEvents, NoopEvents};

use crate::core::{
    ApprovalDecision, ApprovalPolicy, Message, Provider, ProviderRequest, ProviderStreamEvent,
    Role, SessionReader, SessionSink, ToolCall, ToolExecutor,
};
use crate::safety::sanitize_tool_output;
use crate::session::{SessionEvent, event_id};
use crate::tool::ToolResult;
use state::AgentState;

pub struct AgentLoop<P, E, T, A, S>
where
    P: Provider,
    E: AgentEvents,
    T: ToolExecutor,
    A: ApprovalPolicy,
    S: SessionSink + SessionReader,
{
    pub provider: P,
    pub tools: T,
    pub approvals: A,
    pub max_steps: usize,
    pub model: String,
    pub system_prompt: String,
    pub session: S,
    pub events: E,
}

impl<P, E, T, A, S> AgentLoop<P, E, T, A, S>
where
    P: Provider,
    E: AgentEvents,
    T: ToolExecutor,
    A: ApprovalPolicy,
    S: SessionSink + SessionReader,
{
    pub async fn run<F>(&self, prompt: String, mut approve: F) -> anyhow::Result<String>
    where
        F: FnMut(&str) -> anyhow::Result<bool>,
    {
        let mut state = AgentState {
            messages: self.session.replay_messages()?,
            step: 0,
        };

        if state
            .messages
            .iter()
            .all(|message| message.role != Role::System)
            && !self.system_prompt.trim().is_empty()
        {
            self.append_message(
                &mut state,
                Message {
                    role: Role::System,
                    content: self.system_prompt.clone(),
                    tool_call_id: None,
                },
            )?;
        }

        self.append_message(
            &mut state,
            Message {
                role: Role::User,
                content: prompt,
                tool_call_id: None,
            },
        )?;

        while state.step < self.max_steps {
            let req = ProviderRequest {
                model: self.model.clone(),
                messages: state.messages.clone(),
                tools: self.tools.schemas(),
            };

            let mut assistant_content = String::new();
            let mut thinking_content = String::new();
            let response = self
                .provider
                .complete_stream(req, |event| match event {
                    ProviderStreamEvent::AssistantDelta(delta) => {
                        assistant_content.push_str(&delta);
                        self.events.on_assistant_delta(&delta);
                    }
                    ProviderStreamEvent::ThinkingDelta(delta) => {
                        thinking_content.push_str(&delta);
                        self.events.on_thinking(&delta);
                    }
                })
                .await?;

            if assistant_content.is_empty() {
                assistant_content = response.assistant_message.content.clone();
                if !assistant_content.is_empty() {
                    self.events.on_assistant_delta(&assistant_content);
                }
            }

            if thinking_content.is_empty()
                && let Some(t) = &response.thinking
            {
                thinking_content = t.clone();
            }

            if !thinking_content.is_empty() {
                self.session.append(&SessionEvent::Thinking {
                    id: event_id(),
                    content: thinking_content,
                })?;
            }

            let assistant = Message {
                role: Role::Assistant,
                content: assistant_content.clone(),
                tool_call_id: None,
            };

            self.append_message(&mut state, assistant.clone())?;

            if response.done {
                self.events.on_assistant_done();
                return Ok(assistant_content);
            }

            for call in response.tool_calls {
                self.session
                    .append(&SessionEvent::ToolCall { call: call.clone() })?;

                match self.approvals.decision_for_tool(&call.name) {
                    ApprovalDecision::Deny => {
                        let output = format!("tool denied: {}", call.name);
                        self.record_tool_error(&call, output, &mut state)?;
                        continue;
                    }
                    ApprovalDecision::Ask => {
                        self.events.on_tool_start(&call.name, &call.arguments);
                        let approved = approve(&call.name)?;
                        self.session.append(&SessionEvent::Approval {
                            id: event_id(),
                            tool_name: call.name.clone(),
                            approved,
                        })?;
                        if !approved {
                            self.record_tool_error(
                                &call,
                                format!("tool approval denied: {}", call.name),
                                &mut state,
                            )?;
                            continue;
                        }
                    }
                    ApprovalDecision::Allow => {}
                }

                self.execute_tool_call(&call, &mut state).await?;
            }

            state.step += 1;
        }

        anyhow::bail!("Reached max steps without final answer")
    }

    async fn execute_tool_call(
        &self,
        call: &ToolCall,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        self.events.on_tool_start(&call.name, &call.arguments);
        let mut result = self.tools.execute(&call.name, call.arguments.clone()).await;
        result.output = sanitize_tool_output(&result.output);
        self.events.on_tool_end(&call.name, &result);
        self.record_tool_result(call.id.clone(), result, state)
    }

    fn record_tool_error(
        &self,
        call: &ToolCall,
        output: String,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        self.events.on_tool_start(&call.name, &call.arguments);
        let result = ToolResult::err_text("denied", sanitize_tool_output(&output));
        self.events.on_tool_end(&call.name, &result);
        self.record_tool_result(call.id.clone(), result, state)
    }

    fn record_tool_result(
        &self,
        call_id: String,
        result: ToolResult,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        state.push(Message {
            role: Role::Tool,
            content: result.output.clone(),
            tool_call_id: Some(call_id.clone()),
        });
        self.session.append(&SessionEvent::ToolResult {
            id: call_id,
            is_error: result.is_error,
            output: result.output.clone(),
            result: Some(result),
        })?;
        Ok(())
    }

    fn append_message(&self, state: &mut AgentState, message: Message) -> anyhow::Result<()> {
        state.push(message.clone());
        self.session.append(&SessionEvent::Message {
            id: event_id(),
            message,
        })
    }
}
