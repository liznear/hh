pub mod state;
pub mod subagent_manager;

pub use super::{AgentEvents, NoopEvents};

use crate::core::{
    ApprovalChoice, ApprovalDecision, ApprovalPolicy, ApprovalRequest, Message, Provider,
    ProviderRequest, ProviderStreamEvent, QuestionAnswers, QuestionPrompt, Role, SessionReader,
    SessionSink, ToolCall, ToolExecutor,
};
use crate::permission::rules::{PermissionRule, RuleContext};
use crate::safety::sanitize_tool_output;
use crate::session::{SessionEvent, event_id};
use crate::tool::ToolResult;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Serialize;
use state::AgentState;
use std::future::Future;
use std::path::Path;

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
        let mut session_allowed_actions = std::collections::HashSet::<String>::new();
        let mut session_allowed_bash_rules = std::collections::HashSet::<String>::new();

        restore_session_approvals(
            &replayed_events,
            &self.tools,
            &mut session_allowed_actions,
            &mut session_allowed_bash_rules,
        )?;

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
                    tool_calls: Vec::new(),
                },
            )?;
        }

        self.append_message(&mut state, prompt)?;

        loop {
            if self.max_steps > 0 && state.step >= self.max_steps {
                anyhow::bail!("Reached max steps without final answer")
            }

            let queued_user_messages = self.events.drain_queued_user_messages();
            for queued in &queued_user_messages {
                self.append_message(&mut state, queued.message.clone())?;
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

            if !queued_user_messages.is_empty() {
                self.events
                    .on_queued_user_messages_consumed(&queued_user_messages);
            }

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
                tool_calls: response.tool_calls.clone(),
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

                match self
                    .approvals
                    .decision_for_tool_call(&call.name, &call.arguments)
                {
                    ApprovalDecision::Deny => {
                        let output = format!("tool denied: {}", call.name);
                        self.record_tool_error(&call, output, &mut state)?;
                        continue;
                    }
                    ApprovalDecision::Ask => {
                        let request = build_tool_execution_approval_request(&call);
                        let session_key = session_approval_key(&call.name, &request.action);
                        let matched_bash_session_rule = call.name == "bash"
                            && session_allowed_bash_rules
                                .iter()
                                .any(|rule| bash_rule_matches_call(rule, &call.arguments));

                        if !session_allowed_actions.contains(&session_key)
                            && !matched_bash_session_rule
                        {
                            self.events.on_tool_start(&call.name, &call.arguments);
                            let choice = approve(request.clone()).await?;

                            if matches!(
                                choice,
                                ApprovalChoice::AllowSession | ApprovalChoice::AllowAlways
                            ) {
                                session_allowed_actions.insert(session_key);
                            }
                            if choice == ApprovalChoice::AllowAlways
                                && let Some(rule) =
                                    bash_permission_rule_from_action(&request.action)
                            {
                                session_allowed_bash_rules.insert(rule.to_string());
                            }

                            let approved = choice != ApprovalChoice::Deny;
                            self.session.append(&SessionEvent::Approval {
                                id: event_id(),
                                tool_name: call.name.clone(),
                                approved,
                                action: Some(request.action.clone()),
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
                    let event_args =
                        decorate_tool_start_args(&call.id, &call.name, &call.arguments);
                    let execution_args = if call.name == "task" {
                        event_args.clone()
                    } else {
                        call.arguments.clone()
                    };
                    self.events.on_tool_start(&call.name, &event_args);
                    pending_non_blocking.push(async {
                        let mut result = self.tools.execute(&call.name, execution_args).await;
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
            let event_args = decorate_tool_start_args(&call.id, &call.name, &call.arguments);
            let execution_args = if call.name == "task" {
                event_args.clone()
            } else {
                call.arguments.clone()
            };
            self.events.on_tool_start(&call.name, &event_args);
            let mut result = if call.name == "todo_read" {
                todo_snapshot_result(&state.todo_items)
            } else {
                self.tools.execute(&call.name, execution_args).await
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
            tool_calls: Vec::new(),
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

fn decorate_tool_start_args(
    call_id: &str,
    name: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    if name != "task" {
        return args.clone();
    }
    let mut obj = args.as_object().cloned().unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    obj.insert("__started_at".to_string(), serde_json::Value::from(now));
    obj.insert(
        "__call_id".to_string(),
        serde_json::Value::from(call_id.to_string()),
    );
    serde_json::Value::Object(obj)
}

fn build_tool_execution_approval_request(call: &ToolCall) -> ApprovalRequest {
    let permission_rule = suggested_permission_rule(call);
    let approval_kind = if call.name == "bash" {
        "bash"
    } else if is_file_write_tool(&call.name) {
        "file_write"
    } else {
        "generic"
    };

    let stated_purpose = call
        .arguments
        .get("description")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('.'));

    let body = match call.name.as_str() {
        "bash" => {
            let command = call
                .arguments
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or("<unknown command>");
            if let Some(purpose) = stated_purpose {
                format!("Allow `{command}` to {purpose}")
            } else {
                format!("Allow `{command}` for the requested task")
            }
        }
        "write" | "edit" => {
            let path = call
                .arguments
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("<unknown path>");
            if let Some(purpose) = stated_purpose {
                format!("Allow writing `{path}` to {purpose}")
            } else {
                format!("Allow writing `{path}`")
            }
        }
        _ => {
            if let Some(purpose) = stated_purpose {
                format!("Allow tool `{}` to {purpose}", call.name)
            } else {
                format!("Allow tool `{}` with current arguments", call.name)
            }
        }
    };

    ApprovalRequest {
        title: "Tool Execution Approval".to_string(),
        body,
        action: serde_json::json!({
            "operation": "tool_execution",
            "tool_name": call.name,
            "approval_kind": approval_kind,
            "permission_rule": permission_rule,
        }),
    }
}

fn is_file_write_tool(tool_name: &str) -> bool {
    matches!(tool_name, "write" | "edit")
}

fn suggested_permission_rule(call: &ToolCall) -> Option<String> {
    match call.name.as_str() {
        "bash" => {
            let command = call.arguments.get("command")?.as_str()?.trim();
            if command.is_empty() {
                return None;
            }
            Some(format!("Bash({command}*)"))
        }
        "write" | "edit" => {
            let path = call.arguments.get("path")?.as_str()?.trim();
            if path.is_empty() {
                return None;
            }
            Some(format!("Edit({})", normalize_path_pattern(path)))
        }
        _ => None,
    }
}

fn normalize_path_pattern(path: &str) -> String {
    let path = path.replace('\\', "/");
    if std::path::Path::new(&path).is_absolute() {
        return format!("//{}", path.trim_start_matches('/'));
    }
    if path.starts_with("./") {
        return path;
    }
    if path.starts_with('/') {
        return path;
    }
    format!("./{path}")
}

fn restore_session_approvals<T: ToolExecutor>(
    replayed_events: &[SessionEvent],
    tools: &T,
    session_allowed_actions: &mut std::collections::HashSet<String>,
    session_allowed_bash_rules: &mut std::collections::HashSet<String>,
) -> anyhow::Result<()> {
    for event in replayed_events {
        let SessionEvent::Approval {
            approved: true,
            action: Some(action),
            choice: Some(choice),
            ..
        } = event
        else {
            continue;
        };

        if !matches!(
            choice,
            ApprovalChoice::AllowSession | ApprovalChoice::AllowAlways
        ) {
            continue;
        }

        if action
            .get("operation")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "tool_execution")
        {
            if let Some(tool_name) = action.get("tool_name").and_then(|value| value.as_str()) {
                session_allowed_actions.insert(session_approval_key(tool_name, action));
                if *choice == ApprovalChoice::AllowAlways
                    && let Some(rule) = bash_permission_rule_from_action(action)
                {
                    session_allowed_bash_rules.insert(rule.to_string());
                }
            }
            continue;
        }

        let _ = tools.apply_approval_decision(action, ApprovalChoice::AllowSession)?;
    }

    Ok(())
}

fn session_approval_key(tool_name: &str, action: &serde_json::Value) -> String {
    let approval_kind = action
        .get("approval_kind")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    if approval_kind == "bash"
        && let Some(rule) = action
            .get("permission_rule")
            .and_then(|value| value.as_str())
            .filter(|rule| !rule.trim().is_empty())
    {
        return format!("rule:{rule}");
    }

    format!("tool:{tool_name}")
}

fn bash_permission_rule_from_action(action: &serde_json::Value) -> Option<&str> {
    let approval_kind = action
        .get("approval_kind")
        .and_then(|value| value.as_str())?;
    if approval_kind != "bash" {
        return None;
    }

    action
        .get("permission_rule")
        .and_then(|value| value.as_str())
        .filter(|rule| !rule.trim().is_empty())
}

fn bash_rule_matches_call(rule: &str, args: &serde_json::Value) -> bool {
    let Some(parsed_rule) = PermissionRule::parse(rule) else {
        return false;
    };

    parsed_rule.matches(&RuleContext {
        tool_name: "bash",
        capability: "bash",
        args,
        workspace_root: Path::new("."),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::permission::PermissionMatcher;
    use crate::provider::ProviderResponse;
    use crate::session::SessionStore;
    use crate::tool::registry::ToolRegistry;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    #[derive(Clone)]
    struct TestProvider {
        responses: Arc<Mutex<VecDeque<ProviderResponse>>>,
        captured_requests: Arc<Mutex<Vec<ProviderRequest>>>,
    }

    #[async_trait]
    impl Provider for TestProvider {
        async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            if let Ok(mut captured) = self.captured_requests.lock() {
                captured.push(req);
            }
            self.responses
                .lock()
                .expect("provider lock")
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted provider response remaining"))
        }
    }

    #[derive(Clone)]
    struct TestQueuedEvents {
        queued: Arc<Mutex<VecDeque<crate::core::QueuedUserMessage>>>,
        consumed: Arc<Mutex<Vec<Vec<crate::core::QueuedUserMessage>>>>,
        enqueue_after_tool_end: Option<crate::core::QueuedUserMessage>,
    }

    impl AgentEvents for TestQueuedEvents {
        fn on_tool_end(&self, _name: &str, _result: &crate::tool::ToolResult) {
            if let Some(message) = self.enqueue_after_tool_end.clone()
                && let Ok(mut queued) = self.queued.lock()
                && queued.is_empty()
            {
                queued.push_back(message);
            }
        }

        fn drain_queued_user_messages(&self) -> Vec<crate::core::QueuedUserMessage> {
            let Ok(mut queued) = self.queued.lock() else {
                return Vec::new();
            };
            queued.drain(..).collect()
        }

        fn on_queued_user_messages_consumed(&self, messages: &[crate::core::QueuedUserMessage]) {
            if let Ok(mut consumed) = self.consumed.lock() {
                consumed.push(messages.to_vec());
            }
        }
    }

    #[tokio::test]
    async fn allowing_one_bash_command_for_session_does_not_skip_approval_for_other_bash_commands()
    {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");

        let mut settings = Settings::default();
        settings.session.root = temp.path().join("sessions");

        let tools = ToolRegistry::new(&settings, &workspace);
        let approvals = PermissionMatcher::new(settings.clone(), &tools.schemas(), &workspace);
        let session = SessionStore::new(
            &settings.session.root,
            &workspace,
            None,
            Some("test session".to_string()),
        )
        .expect("session store");

        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: vec![ToolCall {
                            id: "call-1".to_string(),
                            name: "bash".to_string(),
                            arguments: json!({ "command": "printf first", "timeout_ms": 1000 }),
                        }],
                    },
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "bash".to_string(),
                        arguments: json!({ "command": "printf first", "timeout_ms": 1000 }),
                    }],
                    done: false,
                    thinking: None,
                    context_tokens: None,
                },
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: vec![ToolCall {
                            id: "call-2".to_string(),
                            name: "bash".to_string(),
                            arguments: json!({ "command": "printf second", "timeout_ms": 1000 }),
                        }],
                    },
                    tool_calls: vec![ToolCall {
                        id: "call-2".to_string(),
                        name: "bash".to_string(),
                        arguments: json!({ "command": "printf second", "timeout_ms": 1000 }),
                    }],
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
            captured_requests: Arc::new(Mutex::new(Vec::new())),
        };

        let agent = AgentLoop {
            provider,
            tools,
            approvals,
            max_steps: 10,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
            events: NoopEvents,
        };

        let approval_count = Arc::new(Mutex::new(0usize));
        let approval_count_for_closure = approval_count.clone();

        let result = agent
            .run(
                Message {
                    role: Role::User,
                    content: "run checks".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                move |_request| {
                    let approval_count = approval_count_for_closure.clone();
                    async move {
                        let mut count = approval_count.lock().expect("approval count lock");
                        *count += 1;
                        Ok(ApprovalChoice::AllowSession)
                    }
                },
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(*approval_count.lock().expect("approval count lock"), 2);

        let events = session.replay_events().expect("replay events");
        let approvals_recorded = events
            .iter()
            .filter(|event| matches!(event, SessionEvent::Approval { .. }))
            .count();
        assert_eq!(approvals_recorded, 2);
    }

    #[tokio::test]
    async fn allow_always_bash_rule_applies_to_matching_command_in_same_session() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");

        let mut settings = Settings::default();
        settings.session.root = temp.path().join("sessions");

        let tools = ToolRegistry::new(&settings, &workspace);
        let approvals = PermissionMatcher::new(settings.clone(), &tools.schemas(), &workspace);
        let session = SessionStore::new(
            &settings.session.root,
            &workspace,
            None,
            Some("test session".to_string()),
        )
        .expect("session store");

        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: vec![ToolCall {
                            id: "call-1".to_string(),
                            name: "bash".to_string(),
                            arguments: json!({ "command": "echo hello", "timeout_ms": 1000 }),
                        }],
                    },
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "bash".to_string(),
                        arguments: json!({ "command": "echo hello", "timeout_ms": 1000 }),
                    }],
                    done: false,
                    thinking: None,
                    context_tokens: None,
                },
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: vec![ToolCall {
                            id: "call-2".to_string(),
                            name: "bash".to_string(),
                            arguments: json!({ "command": "echo hello world", "timeout_ms": 1000 }),
                        }],
                    },
                    tool_calls: vec![ToolCall {
                        id: "call-2".to_string(),
                        name: "bash".to_string(),
                        arguments: json!({ "command": "echo hello world", "timeout_ms": 1000 }),
                    }],
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
            captured_requests: Arc::new(Mutex::new(Vec::new())),
        };

        let agent = AgentLoop {
            provider,
            tools,
            approvals,
            max_steps: 10,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
            events: NoopEvents,
        };

        let approval_count = Arc::new(Mutex::new(0usize));
        let approval_count_for_closure = approval_count.clone();

        let result = agent
            .run(
                Message {
                    role: Role::User,
                    content: "run bash commands".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                move |_request| {
                    let approval_count = approval_count_for_closure.clone();
                    async move {
                        let mut count = approval_count.lock().expect("approval count lock");
                        *count += 1;
                        Ok(ApprovalChoice::AllowAlways)
                    }
                },
            )
            .await;

        assert!(result.is_ok());
        assert_eq!(*approval_count.lock().expect("approval count lock"), 1);

        let events = session.replay_events().expect("replay events");
        let approvals_recorded = events
            .iter()
            .filter(|event| matches!(event, SessionEvent::Approval { .. }))
            .count();
        assert_eq!(approvals_recorded, 1);
    }

    #[tokio::test]
    async fn queued_user_message_is_appended_before_next_provider_call() {
        let temp = tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("create workspace");

        let mut settings = Settings::default();
        settings.session.root = temp.path().join("sessions");

        let tools = ToolRegistry::new(&settings, &workspace);
        let approvals = PermissionMatcher::new(settings.clone(), &tools.schemas(), &workspace);
        let session = SessionStore::new(
            &settings.session.root,
            &workspace,
            None,
            Some("queued messages".to_string()),
        )
        .expect("session store");

        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![
                ProviderResponse {
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
            captured_requests: Arc::clone(&captured_requests),
        };

        let consumed = Arc::new(Mutex::new(Vec::new()));
        let queued_events = TestQueuedEvents {
            queued: Arc::new(Mutex::new(VecDeque::new())),
            consumed: Arc::clone(&consumed),
            enqueue_after_tool_end: Some(crate::core::QueuedUserMessage {
                message: Message {
                    role: Role::User,
                    content: "queued follow-up".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                message_index: Some(7),
            }),
        };

        let agent = AgentLoop {
            provider,
            tools,
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session,
            events: queued_events,
        };

        let result = agent
            .run(
                Message {
                    role: Role::User,
                    content: "initial prompt".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                |_request| async { Ok(ApprovalChoice::AllowOnce) },
            )
            .await;

        assert!(result.is_ok());

        let requests = captured_requests.lock().expect("captured requests");
        assert_eq!(requests.len(), 2);
        assert!(requests[1].messages.iter().any(|message| {
            message.role == Role::User && message.content == "queued follow-up"
        }));

        let consumed = consumed.lock().expect("consumed queue");
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].len(), 1);
        assert_eq!(consumed[0][0].message.content, "queued follow-up");
        assert_eq!(consumed[0][0].message_index, Some(7));
    }
}
