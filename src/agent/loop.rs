use crate::agent::events::AgentEvents;
use crate::agent::state::AgentState;
use crate::permission::{Decision, PermissionMatcher};
use crate::provider::{Message, Provider, ProviderRequest, ProviderStreamEvent, Role, ToolCall};
use crate::safety::sanitize_tool_output;
use crate::session::{SessionEvent, SessionStore, event_id};
use crate::tool::registry::ToolRegistry;

pub struct AgentLoop<P, E>
where
    P: Provider,
    E: AgentEvents,
{
    pub provider: P,
    pub tool_registry: ToolRegistry,
    pub permissions: PermissionMatcher,
    pub max_steps: usize,
    pub model: String,
    pub session: SessionStore,
    pub events: E,
}

impl<P, E> AgentLoop<P, E>
where
    P: Provider,
    E: AgentEvents,
{
    pub async fn run<F>(&self, prompt: String, mut approve: F) -> anyhow::Result<String>
    where
        F: FnMut(&str) -> anyhow::Result<bool>,
    {
        let mut state = AgentState {
            messages: self.session.replay_messages()?,
            step: 0,
        };

        state.push(Message {
            role: Role::User,
            content: prompt.clone(),
            tool_call_id: None,
        });
        self.session.append(&SessionEvent::Message {
            id: event_id(),
            role: Role::User,
            content: prompt,
            tool_call_id: None,
        })?;

        while state.step < self.max_steps {
            let req = ProviderRequest {
                model: self.model.clone(),
                messages: state.messages.clone(),
                tools: self.tool_registry.schemas(),
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

            let assistant = Message {
                role: Role::Assistant,
                content: assistant_content.clone(),
                tool_call_id: None,
            };

            self.session.append(&SessionEvent::Message {
                id: event_id(),
                role: Role::Assistant,
                content: assistant.content.clone(),
                tool_call_id: None,
            })?;
            if !thinking_content.is_empty() {
                self.session.append(&SessionEvent::Thinking {
                    id: event_id(),
                    content: thinking_content,
                })?;
            }
            state.push(assistant.clone());

            // If no tool calls, the agent is done - always call on_assistant_done
            if response.done {
                self.events.on_assistant_done();
                return Ok(assistant_content);
            }

            for call in response.tool_calls {
                self.session.append(&SessionEvent::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })?;

                match self.permissions.decision_for_tool(&call.name) {
                    Decision::Deny => {
                        let output = format!("tool denied: {}", call.name);
                        self.record_tool_error(&call, output, &mut state)?;
                    }
                    Decision::Ask => {
                        self.events.on_tool_start(&call.name, &call.arguments);
                        let approved = approve(&call.name)?;
                        self.session.append(&SessionEvent::Approval {
                            id: event_id(),
                            tool_name: call.name.clone(),
                            approved,
                        })?;
                        if !approved {
                            let output = format!("tool approval denied: {}", call.name);
                            let output = sanitize_tool_output(&output);
                            self.events.on_tool_end(&call.name, true, &preview(&output));
                            self.record_tool_result(call.id.clone(), true, output, &mut state)?;
                            continue;
                        }

                        self.execute_tool_call(&call, &mut state).await?;
                    }
                    Decision::Allow => {
                        self.execute_tool_call(&call, &mut state).await?;
                    }
                }
            }

            state.step += 1;
        }

        Ok("Reached max steps without final answer".to_string())
    }

    async fn execute_tool_call(
        &self,
        call: &ToolCall,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        self.events.on_tool_start(&call.name, &call.arguments);
        let result = self
            .tool_registry
            .execute(&call.name, call.arguments.clone())
            .await;
        let output = sanitize_tool_output(&result.output);
        self.events
            .on_tool_end(&call.name, result.is_error, &preview(&output));
        self.record_tool_result(call.id.clone(), result.is_error, output, state)
    }

    fn record_tool_error(
        &self,
        call: &ToolCall,
        output: String,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        self.events.on_tool_start(&call.name, &call.arguments);
        let output = sanitize_tool_output(&output);
        self.events.on_tool_end(&call.name, true, &preview(&output));
        self.record_tool_result(call.id.clone(), true, output, state)
    }

    fn record_tool_result(
        &self,
        call_id: String,
        is_error: bool,
        output: String,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        state.push(Message {
            role: Role::Tool,
            content: output.clone(),
            tool_call_id: Some(call_id.clone()),
        });
        self.session.append(&SessionEvent::ToolResult {
            id: call_id,
            is_error,
            output,
        })?;
        Ok(())
    }
}

fn preview(text: &str) -> String {
    let max_chars = 160;
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", preview)
    } else {
        preview
    }
}
