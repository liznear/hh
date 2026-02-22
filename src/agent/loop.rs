use crate::agent::state::AgentState;
use crate::permission::{Decision, PermissionMatcher};
use crate::provider::{Message, Provider, ProviderRequest, Role};
use crate::safety::sanitize_tool_output;
use crate::session::{SessionEvent, SessionStore, event_id};
use crate::tool::registry::ToolRegistry;

pub struct AgentLoop<P>
where
    P: Provider,
{
    pub provider: P,
    pub tool_registry: ToolRegistry,
    pub permissions: PermissionMatcher,
    pub max_steps: usize,
    pub model: String,
    pub session: SessionStore,
}

impl<P> AgentLoop<P>
where
    P: Provider,
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
            let response = self.provider.complete(req).await?;

            let assistant = response.assistant_message;
            self.session.append(&SessionEvent::Message {
                id: event_id(),
                role: Role::Assistant,
                content: assistant.content.clone(),
                tool_call_id: None,
            })?;
            state.push(assistant.clone());

            if response.done {
                return Ok(assistant.content);
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
                        let output = sanitize_tool_output(&output);
                        state.push(Message {
                            role: Role::Tool,
                            content: output.clone(),
                            tool_call_id: Some(call.id.clone()),
                        });
                        self.session.append(&SessionEvent::ToolResult {
                            id: call.id,
                            is_error: true,
                            output,
                        })?;
                    }
                    Decision::Ask => {
                        let approved = approve(&call.name)?;
                        self.session.append(&SessionEvent::Approval {
                            id: event_id(),
                            tool_name: call.name.clone(),
                            approved,
                        })?;
                        if !approved {
                            let output = format!("tool approval denied: {}", call.name);
                            state.push(Message {
                                role: Role::Tool,
                                content: output.clone(),
                                tool_call_id: Some(call.id.clone()),
                            });
                            self.session.append(&SessionEvent::ToolResult {
                                id: call.id,
                                is_error: true,
                                output,
                            })?;
                            continue;
                        }

                        let result = self
                            .tool_registry
                            .execute(&call.name, call.arguments.clone())
                            .await;
                        let output = sanitize_tool_output(&result.output);
                        state.push(Message {
                            role: Role::Tool,
                            content: output.clone(),
                            tool_call_id: Some(call.id.clone()),
                        });
                        self.session.append(&SessionEvent::ToolResult {
                            id: call.id,
                            is_error: result.is_error,
                            output,
                        })?;
                    }
                    Decision::Allow => {
                        let result = self
                            .tool_registry
                            .execute(&call.name, call.arguments.clone())
                            .await;
                        let output = sanitize_tool_output(&result.output);
                        state.push(Message {
                            role: Role::Tool,
                            content: output.clone(),
                            tool_call_id: Some(call.id.clone()),
                        });
                        self.session.append(&SessionEvent::ToolResult {
                            id: call.id,
                            is_error: result.is_error,
                            output,
                        })?;
                    }
                }
            }

            state.step += 1;
        }

        Ok("Reached max steps without final answer".to_string())
    }
}
