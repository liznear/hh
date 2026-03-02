pub mod state;
pub mod subagent_manager;

pub use super::{AgentEvents, NoopEvents};

use crate::core::{
    ApprovalChoice, ApprovalDecision, ApprovalPolicy, ApprovalRequest, Message, Provider,
    ProviderRequest, ProviderStreamEvent, QuestionAnswers, QuestionPrompt, Role, SessionReader,
    SessionSink, ToolCall, ToolExecutor,
};
use crate::safety::sanitize_tool_output;
use crate::session::{SessionEvent, event_id};
use crate::tool::ToolResult;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Serialize;
use state::AgentState;
use std::future::Future;

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
    pub async fn run<AP, APFut>(&self, prompt: Message, mut approve: AP) -> anyhow::Result<String>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
    {
        self.run_with_question_tool(prompt, &mut approve, |_questions| async {
            anyhow::bail!("question tool is unavailable in this mode; provide a question handler")
        })
        .await
    }

    pub async fn run_with_question_tool<AP, APFut, Q, QFut>(
        &self,
        prompt: Message,
        mut approve: AP,
        mut ask_question: Q,
    ) -> anyhow::Result<String>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
    {
        let replayed_events = self.session.replay_events()?;
        let mut state = AgentState {
            messages: self.session.replay_messages()?,
            todo_items: Vec::new(),
            step: 0,
        };
        let mut session_allowed_tools = std::collections::HashSet::<String>::new();

        restore_session_approvals(&replayed_events, &self.tools, &mut session_allowed_tools)?;

        let mut tool_name_by_call_id = std::collections::HashMap::new();
        for event in &replayed_events {
            match event {
                SessionEvent::ToolCall { call } => {
                    tool_name_by_call_id.insert(call.id.clone(), call.name.clone());
                }
                SessionEvent::ToolResult { id, result, .. } => {
                    if let (Some(name), Some(tool_result)) =
                        (tool_name_by_call_id.get(id), result.as_ref())
                    {
                        state.apply_tool_result(name, tool_result);
                    }
                }
                _ => {}
            }
        }

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
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
            )?;
        }

        self.append_message(&mut state, prompt)?;

        loop {
            if self.max_steps > 0 && state.step >= self.max_steps {
                anyhow::bail!("Reached max steps without final answer")
            }

            let mut request_messages = state.messages.clone();
            if let Some(state_message) = state.state_for_llm() {
                request_messages.push(state_message);
            }

            let req = ProviderRequest {
                model: self.model.clone(),
                messages: request_messages,
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

            if let Some(tokens) = response.context_tokens {
                self.events.on_context_usage(tokens);
            }

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
                attachments: Vec::new(),
                tool_call_id: None,
            };

            self.append_message(&mut state, assistant.clone())?;

            if response.done {
                self.events.on_assistant_done();
                return Ok(assistant_content);
            }

            let mut pending_non_blocking = FuturesUnordered::new();

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
                        if !session_allowed_tools.contains(&call.name) {
                            self.events.on_tool_start(&call.name, &call.arguments);
                            let choice = approve(ApprovalRequest {
                                title: "Tool Execution Approval".to_string(),
                                body: format!(
                                    "Allow tool `{}` to execute with the current arguments?",
                                    call.name
                                ),
                                action: serde_json::json!({
                                    "operation": "tool_execution",
                                    "tool_name": call.name,
                                }),
                            })
                            .await?;

                            if choice == ApprovalChoice::AllowSession {
                                session_allowed_tools.insert(call.name.clone());
                            }

                            let approved = choice != ApprovalChoice::Deny;
                            self.session.append(&SessionEvent::Approval {
                                id: event_id(),
                                tool_name: call.name.clone(),
                                approved,
                                action: Some(serde_json::json!({
                                    "operation": "tool_execution",
                                    "tool_name": call.name,
                                })),
                                choice: Some(choice),
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
                    }
                    ApprovalDecision::Allow => {}
                }

                if call.name == "question" {
                    self.events.on_tool_start(&call.name, &call.arguments);
                    let result = self
                        .execute_question_tool_call(&call, &mut ask_question)
                        .await;
                    self.events.on_tool_end(&call.name, &result);
                    self.record_tool_result(&call, result, &mut state)?;
                    continue;
                }

                if self.tools.is_non_blocking(&call.name) {
                    let event_args = decorate_tool_start_args(&call.name, &call.arguments);
                    self.events.on_tool_start(&call.name, &event_args);
                    pending_non_blocking.push(async {
                        let mut result =
                            self.tools.execute(&call.name, call.arguments.clone()).await;
                        result.output = sanitize_tool_output(&result.output);
                        (call, result)
                    });
                    continue;
                }

                self.execute_tool_call(&call, &mut state, &mut approve)
                    .await?;
            }

            while let Some((call, result)) = pending_non_blocking.next().await {
                self.events.on_tool_end(&call.name, &result);
                self.record_tool_result(&call, result, &mut state)?;
            }

            state.step += 1;
        }
    }

    async fn execute_question_tool_call<Q, QFut>(
        &self,
        call: &ToolCall,
        ask_question: &mut Q,
    ) -> ToolResult
    where
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
    {
        let parsed = match crate::tool::question::parse_question_args(call.arguments.clone()) {
            Ok(parsed) => parsed,
            Err(err) => return ToolResult::err_text("invalid_question_args", err.to_string()),
        };

        match ask_question(parsed.questions.clone()).await {
            Ok(answers) => crate::tool::question::question_result(&parsed.questions, answers),
            Err(err) => ToolResult::err_text("question_dismissed", err.to_string()),
        }
    }

    async fn execute_tool_call<AP, APFut>(
        &self,
        call: &ToolCall,
        state: &mut AgentState,
        approve: &mut AP,
    ) -> anyhow::Result<()>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
    {
        loop {
            let event_args = decorate_tool_start_args(&call.name, &call.arguments);
            self.events.on_tool_start(&call.name, &event_args);
            let mut result = if call.name == "todo_read" {
                todo_snapshot_result(&state.todo_items)
            } else {
                self.tools.execute(&call.name, call.arguments.clone()).await
            };

            if let Some(request) = parse_approval_request(&result) {
                let choice = approve(request.clone()).await?;
                let approved = choice != ApprovalChoice::Deny;
                self.session.append(&SessionEvent::Approval {
                    id: event_id(),
                    tool_name: call.name.clone(),
                    approved,
                    action: Some(request.action.clone()),
                    choice: Some(choice),
                })?;

                if !approved {
                    let denied = ToolResult::err_text("denied", "approval denied by user");
                    self.events.on_tool_end(&call.name, &denied);
                    return self.record_tool_result(call, denied, state);
                }

                let applied = self
                    .tools
                    .apply_approval_decision(&request.action, choice)?;
                if !applied {
                    result = ToolResult::err_text(
                        "approval_error",
                        "approval decision could not be applied",
                    );
                    result.output = sanitize_tool_output(&result.output);
                    self.events.on_tool_end(&call.name, &result);
                    return self.record_tool_result(call, result, state);
                }

                continue;
            }

            result.output = sanitize_tool_output(&result.output);
            self.events.on_tool_end(&call.name, &result);
            return self.record_tool_result(call, result, state);
        }
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
        self.record_tool_result(call, result, state)
    }

    fn record_tool_result(
        &self,
        call: &ToolCall,
        result: ToolResult,
        state: &mut AgentState,
    ) -> anyhow::Result<()> {
        let call_id = call.id.clone();
        state.push(Message {
            role: Role::Tool,
            content: result.output.clone(),
            attachments: Vec::new(),
            tool_call_id: Some(call_id.clone()),
        });
        if state.apply_tool_result(&call.name, &result) {
            self.events.on_todo_items_changed(&state.todo_items);
        }
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

fn decorate_tool_start_args(name: &str, args: &serde_json::Value) -> serde_json::Value {
    if name != "task" {
        return args.clone();
    }
    let mut obj = args.as_object().cloned().unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    obj.insert("__started_at".to_string(), serde_json::Value::from(now));
    serde_json::Value::Object(obj)
}

fn restore_session_approvals<T: ToolExecutor>(
    replayed_events: &[SessionEvent],
    tools: &T,
    session_allowed_tools: &mut std::collections::HashSet<String>,
) -> anyhow::Result<()> {
    for event in replayed_events {
        let SessionEvent::Approval {
            approved: true,
            action: Some(action),
            choice: Some(ApprovalChoice::AllowSession),
            ..
        } = event
        else {
            continue;
        };

        if let Some(tool_name) = action.get("tool_name").and_then(|value| value.as_str())
            && action
                .get("operation")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == "tool_execution")
        {
            session_allowed_tools.insert(tool_name.to_string());
            continue;
        }

        let _ = tools.apply_approval_decision(action, ApprovalChoice::AllowSession)?;
    }

    Ok(())
}

fn parse_approval_request(result: &ToolResult) -> Option<ApprovalRequest> {
    if result.summary != "approval_required" {
        return None;
    }

    let payload = result.payload.as_object()?;
    let title = payload.get("title")?.as_str()?.to_string();
    let body = payload.get("body")?.as_str()?.to_string();
    let action = payload.get("action")?.clone();

    Some(ApprovalRequest {
        title,
        body,
        action,
    })
}

#[derive(Debug, Serialize)]
struct TodoSnapshotCounts {
    total: usize,
    pending: usize,
    in_progress: usize,
    completed: usize,
    cancelled: usize,
}

#[derive(Debug, Serialize)]
struct TodoSnapshotOutput {
    todos: Vec<crate::core::TodoItem>,
    counts: TodoSnapshotCounts,
}

fn todo_snapshot_result(items: &[crate::core::TodoItem]) -> ToolResult {
    let mut counts = TodoSnapshotCounts {
        total: items.len(),
        pending: 0,
        in_progress: 0,
        completed: 0,
        cancelled: 0,
    };

    for item in items {
        match item.status {
            crate::core::TodoStatus::Pending => counts.pending += 1,
            crate::core::TodoStatus::InProgress => counts.in_progress += 1,
            crate::core::TodoStatus::Completed => counts.completed += 1,
            crate::core::TodoStatus::Cancelled => counts.cancelled += 1,
        }
    }

    let output = TodoSnapshotOutput {
        todos: items.to_vec(),
        counts,
    };

    ToolResult::ok_json_typed_serializable(
        "todo list snapshot",
        "application/vnd.hh.todo+json",
        &output,
    )
}
