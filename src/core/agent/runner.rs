use crate::core::{
    ApprovalChoice, ApprovalDecision, ApprovalPolicy, ApprovalRequest, Message, Provider,
    QuestionAnswers, QuestionPrompt, Role, ToolCall, ToolExecutor,
};
use crate::permission::rules::{PermissionRule, RuleContext};
use crate::safety::sanitize_tool_output;
use crate::session::SessionEvent;
use crate::tool::ToolExecution;
use crate::tool::ToolResult;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::path::Path;
use tokio::sync::{mpsc, watch};

use super::{
    CoreInput, CoreOutput, RunnerInput, RunnerOutput, RunnerState, StateOp, StatePatch,
    core::{AgentCore, CoreTurnResult},
};

#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct TurnState {
    pub messages: Vec<Message>,
    pub step: usize,
}

const CANCELLATION_ERROR_MESSAGE: &str = "agent run cancelled";

pub fn is_cancellation_error(err: &anyhow::Error) -> bool {
    err.to_string().contains(CANCELLATION_ERROR_MESSAGE)
}

pub struct AgentRunner<'a, P, T, A>
where
    P: Provider,
    T: ToolExecutor,
{
    pub core: AgentCore<'a, P>,
    pub tools: &'a T,
    pub approvals: &'a A,
    pub state: RunnerState,
    pub session_allowed_actions: HashSet<String>,
    pub session_allowed_bash_rules: HashSet<String>,
}

pub enum CallHandlingOutcome {
    Handled {
        message: Message,
        changed: bool,
    },
    NonBlocking {
        call: ToolCall,
        execution_args: Value,
    },
}

impl<'a, P, T, A> AgentRunner<'a, P, T, A>
where
    P: Provider,
    T: ToolExecutor,
    A: ApprovalPolicy,
{
    pub fn new(core: AgentCore<'a, P>, tools: &'a T, approvals: &'a A, state: RunnerState) -> Self {
        Self {
            core,
            tools,
            approvals,
            state,
            session_allowed_actions: HashSet::new(),
            session_allowed_bash_rules: HashSet::new(),
        }
    }

    pub fn hydrate_state_from_replayed_tool_results(
        &mut self,
        replayed_events: &[SessionEvent],
        has_snapshot: bool,
    ) {
        if has_snapshot {
            return;
        }

        let mut tool_name_by_call_id = std::collections::HashMap::new();
        for event in replayed_events {
            match event {
                SessionEvent::ToolCall { call } => {
                    tool_name_by_call_id.insert(call.id.clone(), call.name.clone());
                }
                SessionEvent::ToolResult { id, result, .. } => {
                    if let (Some(name), Some(tool_result)) =
                        (tool_name_by_call_id.get(id), result.as_ref())
                    {
                        self.apply_tool_result(name, tool_result, StatePatch::none());
                    }
                }
                _ => {}
            }
        }
    }

    pub fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> ApprovalDecision {
        self.approvals.decision_for_tool_call(tool_name, args)
    }

    pub fn is_non_blocking_tool(&self, name: &str) -> bool {
        self.tools.is_non_blocking(name)
    }

    pub async fn execute_tool(&self, name: &str, args: Value) -> ToolExecution {
        self.tools.execute(name, args).await
    }

    pub fn apply_approval_decision(
        &self,
        action: &Value,
        choice: ApprovalChoice,
    ) -> anyhow::Result<bool> {
        self.tools.apply_approval_decision(action, choice)
    }

    pub fn send_core_input(&mut self, input: CoreInput) -> anyhow::Result<()> {
        self.core.handle_input(input)
    }

    pub fn has_pending_tool_calls(&self) -> bool {
        self.core.has_pending_tool_calls()
    }

    pub fn apply_tool_result(
        &mut self,
        tool_name: &str,
        result: &ToolResult,
        patch: StatePatch,
    ) -> bool {
        apply_tool_outcome(&mut self.state, tool_name, result, patch)
    }

    pub fn record_tool_result(
        &mut self,
        call: &ToolCall,
        result: ToolResult,
        patch: StatePatch,
    ) -> anyhow::Result<(Message, bool)> {
        let tool_message = Message {
            role: Role::Tool,
            content: result.output.clone(),
            attachments: Vec::new(),
            tool_call_id: Some(call.id.clone()),
            tool_calls: Vec::new(),
        };

        let changed = self.apply_tool_result(&call.name, &result, patch);

        self.send_core_input(CoreInput::ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            result: result.clone(),
        })?;

        if changed {
            self.send_core_input(CoreInput::SetEphemeralState(self.state_for_llm()))?;
        }

        Ok((tool_message, changed))
    }

    pub fn record_denied_tool_error(
        &mut self,
        call: &ToolCall,
        output: String,
    ) -> anyhow::Result<(ToolResult, Message, bool)> {
        let result = ToolResult::err_text("denied", sanitize_tool_output(&output));
        let event_result = result.clone();
        let (message, changed) = self.record_tool_result(call, result, StatePatch::none())?;
        Ok((event_result, message, changed))
    }

    pub fn tool_event_and_execution_args(&self, call: &ToolCall) -> (Value, Value) {
        let event_args = decorate_tool_start_args(&call.id, &call.name, &call.arguments);
        let execution_args = if call.name == "task" {
            event_args.clone()
        } else {
            call.arguments.clone()
        };
        (event_args, execution_args)
    }

    pub fn should_prompt_for_approval(&self, call: &ToolCall, request: &ApprovalRequest) -> bool {
        let session_key = session_approval_key(&call.name, &request.action);
        let matched_bash_session_rule = call.name == "bash"
            && self
                .session_allowed_bash_rules
                .iter()
                .any(|rule| bash_rule_matches_call(rule, &call.arguments));

        !self.session_allowed_actions.contains(&session_key) && !matched_bash_session_rule
    }

    pub fn record_user_approval_decision(
        &mut self,
        call: &ToolCall,
        request: &ApprovalRequest,
        choice: ApprovalChoice,
    ) -> bool {
        if matches!(
            choice,
            ApprovalChoice::AllowSession | ApprovalChoice::AllowAlways
        ) {
            self.session_allowed_actions
                .insert(session_approval_key(&call.name, &request.action));
        }
        if choice == ApprovalChoice::AllowAlways
            && let Some(rule) = bash_permission_rule_from_action(&request.action)
        {
            self.session_allowed_bash_rules.insert(rule.to_string());
        }

        choice != ApprovalChoice::Deny
    }

    pub fn build_tool_execution_approval_request(&self, call: &ToolCall) -> ApprovalRequest {
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

    pub async fn complete_turn<F>(
        &mut self,
        messages: &[Message],
        mut emit: F,
    ) -> anyhow::Result<CoreTurnResult>
    where
        F: FnMut(CoreOutput) + Send,
    {
        let mut context_tokens = None;
        self.send_core_input(CoreInput::SetEphemeralState(self.state_for_llm()))?;
        let turn = self
            .core
            .complete_turn(messages, |output| match output {
                CoreOutput::ContextUsage(tokens) => {
                    context_tokens = Some(tokens);
                    emit(CoreOutput::ContextUsage(tokens));
                }
                other => emit(other),
            })
            .await?;

        if let Some(tokens) = context_tokens {
            let _ = self
                .state
                .apply_patch(StatePatch::with_op(StateOp::SetContextTokens { tokens }));
        }

        Ok(turn)
    }

    pub async fn execute_tool_call<AP, APFut, TS, TE>(
        &mut self,
        call: &ToolCall,
        approve: &mut AP,
        mut on_tool_start: TS,
        mut on_tool_end: TE,
        emit_output: &mut (impl FnMut(RunnerOutput) + Send),
    ) -> anyhow::Result<(Message, bool)>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        TS: FnMut(&str, &Value),
        TE: FnMut(&str, &ToolResult),
    {
        loop {
            let (event_args, execution_args) = self.tool_event_and_execution_args(call);

            on_tool_start(&call.name, &event_args);

            let execution = if call.name == "todo_read" {
                ToolExecution::from_result(todo_snapshot_result(&self.state.todo_items))
            } else {
                self.execute_tool(&call.name, execution_args).await
            };

            let ToolExecution { mut result, patch } = execution;

            if let Some(request) = parse_approval_request(&result) {
                emit_output(RunnerOutput::ApprovalRequired {
                    call_id: call.id.clone(),
                    request: request.clone(),
                });
                let choice = approve(request.clone()).await?;
                let approved = choice != ApprovalChoice::Deny;
                emit_output(RunnerOutput::ApprovalRecorded {
                    tool_name: call.name.clone(),
                    approved,
                    action: Some(request.action.clone()),
                    choice: Some(choice),
                });

                if !approved {
                    let denied = ToolResult::err_text("denied", "approval denied by user");
                    on_tool_end(&call.name, &denied);
                    return self.record_tool_result(call, denied, StatePatch::none());
                }

                let applied = self.apply_approval_decision(&request.action, choice)?;
                if !applied {
                    result = ToolResult::err_text(
                        "approval_error",
                        "approval decision could not be applied",
                    );
                    result.output = sanitize_tool_output(&result.output);
                    on_tool_end(&call.name, &result);
                    return self.record_tool_result(call, result, StatePatch::none());
                }

                continue;
            }

            result.output = sanitize_tool_output(&result.output);
            on_tool_end(&call.name, &result);
            return self.record_tool_result(call, result, patch);
        }
    }

    pub async fn execute_question_tool_call<Q, QFut, TS, TE>(
        &mut self,
        call: &ToolCall,
        ask_question: &mut Q,
        mut on_tool_start: TS,
        mut on_tool_end: TE,
    ) -> anyhow::Result<(Message, bool)>
    where
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
        TS: FnMut(&str, &Value),
        TE: FnMut(&str, &ToolResult),
    {
        on_tool_start(&call.name, &call.arguments);

        let parsed = match crate::tool::question::parse_question_args(call.arguments.clone()) {
            Ok(parsed) => parsed,
            Err(err) => {
                let result = ToolResult::err_text("invalid_question_args", err.to_string());
                on_tool_end(&call.name, &result);
                return self.record_tool_result(call, result, StatePatch::none());
            }
        };

        let result = match ask_question(parsed.questions.clone()).await {
            Ok(answers) => crate::tool::question::question_result(&parsed.questions, answers),
            Err(err) => ToolResult::err_text("question_dismissed", err.to_string()),
        };

        on_tool_end(&call.name, &result);

        self.record_tool_result(call, result, StatePatch::none())
    }

    pub async fn handle_tool_call<AP, APFut, Q, QFut, TS, TE>(
        &mut self,
        call: ToolCall,
        approve: &mut AP,
        ask_question: &mut Q,
        mut on_tool_start: TS,
        mut on_tool_end: TE,
        emit_output: &mut (impl FnMut(RunnerOutput) + Send),
    ) -> anyhow::Result<CallHandlingOutcome>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
        TS: FnMut(&str, &Value),
        TE: FnMut(&str, &ToolResult),
    {
        emit_output(RunnerOutput::ToolCallRecorded(call.clone()));

        match self.decision_for_tool_call(&call.name, &call.arguments) {
            ApprovalDecision::Deny => {
                on_tool_start(&call.name, &call.arguments);
                let (denied, message, changed) =
                    self.record_denied_tool_error(&call, format!("tool denied: {}", call.name))?;
                on_tool_end(&call.name, &denied);
                return Ok(CallHandlingOutcome::Handled { message, changed });
            }
            ApprovalDecision::Ask => {
                let request = self.build_tool_execution_approval_request(&call);
                if self.should_prompt_for_approval(&call, &request) {
                    emit_output(RunnerOutput::ApprovalRequired {
                        call_id: call.id.clone(),
                        request: request.clone(),
                    });
                    on_tool_start(&call.name, &call.arguments);
                    let choice = approve(request.clone()).await?;
                    let approved = self.record_user_approval_decision(&call, &request, choice);
                    emit_output(RunnerOutput::ApprovalRecorded {
                        tool_name: call.name.clone(),
                        approved,
                        action: Some(request.action.clone()),
                        choice: Some(choice),
                    });
                    if !approved {
                        let denied = ToolResult::err_text(
                            "denied",
                            sanitize_tool_output(&format!("tool approval denied: {}", call.name)),
                        );
                        on_tool_end(&call.name, &denied);
                        let (message, changed) =
                            self.record_tool_result(&call, denied, StatePatch::none())?;
                        return Ok(CallHandlingOutcome::Handled { message, changed });
                    }
                }
            }
            ApprovalDecision::Allow => {}
        }

        if call.name == "question" {
            if let Ok(parsed) = crate::tool::question::parse_question_args(call.arguments.clone()) {
                emit_output(RunnerOutput::QuestionRequired {
                    call_id: call.id.clone(),
                    prompts: parsed.questions,
                });
            }
            let (message, changed) = self
                .execute_question_tool_call(
                    &call,
                    ask_question,
                    &mut on_tool_start,
                    &mut on_tool_end,
                )
                .await?;
            return Ok(CallHandlingOutcome::Handled { message, changed });
        }

        if self.is_non_blocking_tool(&call.name) {
            let (event_args, execution_args) = self.tool_event_and_execution_args(&call);
            on_tool_start(&call.name, &event_args);
            return Ok(CallHandlingOutcome::NonBlocking {
                call,
                execution_args,
            });
        }

        let (message, changed) = self
            .execute_tool_call(
                &call,
                approve,
                &mut on_tool_start,
                &mut on_tool_end,
                emit_output,
            )
            .await?;
        Ok(CallHandlingOutcome::Handled { message, changed })
    }

    pub async fn process_tool_calls<AP, APFut, Q, QFut>(
        &mut self,
        messages: &mut Vec<Message>,
        tool_calls: Vec<ToolCall>,
        approve: &mut AP,
        ask_question: &mut Q,
        emit_output: &mut (impl FnMut(RunnerOutput) + Send),
    ) -> anyhow::Result<()>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
    {
        self.process_tool_calls_cancellable(
            messages,
            tool_calls,
            approve,
            ask_question,
            emit_output,
            &mut || std::future::pending::<()>(),
        )
        .await
    }

    pub async fn process_tool_calls_cancellable<AP, APFut, Q, QFut, C, CFut>(
        &mut self,
        messages: &mut Vec<Message>,
        tool_calls: Vec<ToolCall>,
        approve: &mut AP,
        ask_question: &mut Q,
        emit_output: &mut (impl FnMut(RunnerOutput) + Send),
        cancel: &mut C,
    ) -> anyhow::Result<()>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
        C: FnMut() -> CFut,
        CFut: Future<Output = ()> + Send,
    {
        let mut pending_non_blocking = FuturesUnordered::new();

        for call in tool_calls {
            let call_id = call.id.clone();
            let lifecycle_outputs = std::sync::Mutex::new(Vec::new());
            let handle_call = self.handle_tool_call(
                call,
                approve,
                ask_question,
                |name, args| {
                    if let Ok(mut outputs) = lifecycle_outputs.lock() {
                        outputs.push(RunnerOutput::ToolStart {
                            call_id: call_id.clone(),
                            name: name.to_string(),
                            args: args.clone(),
                        });
                    }
                },
                |name, result| {
                    if let Ok(mut outputs) = lifecycle_outputs.lock() {
                        outputs.push(RunnerOutput::ToolEnd {
                            call_id: call_id.clone(),
                            name: name.to_string(),
                            result: result.clone(),
                        });
                    }
                },
                emit_output,
            );

            match tokio::select! {
                _ = cancel() => {
                    self.send_core_input(CoreInput::Cancel)?;
                    emit_output(RunnerOutput::Cancelled);
                    anyhow::bail!(CANCELLATION_ERROR_MESSAGE)
                }
                outcome = handle_call => outcome?
            } {
                CallHandlingOutcome::Handled { message, changed } => {
                    emit_lifecycle_outputs(emit_output, lifecycle_outputs);
                    emit_tool_message_outputs(messages, message, &self.state, changed, emit_output);
                }
                CallHandlingOutcome::NonBlocking {
                    call,
                    execution_args,
                } => {
                    emit_lifecycle_outputs(emit_output, lifecycle_outputs);
                    let tools = self.tools;
                    pending_non_blocking.push(async move {
                        execute_non_blocking_tool(tools, call, execution_args).await
                    });
                }
            }
        }

        while !pending_non_blocking.is_empty() {
            let completion = tokio::select! {
                _ = cancel() => {
                    self.send_core_input(CoreInput::Cancel)?;
                    emit_output(RunnerOutput::Cancelled);
                    anyhow::bail!(CANCELLATION_ERROR_MESSAGE)
                }
                completion = pending_non_blocking.next() => completion
            };

            let Some((call, result)) = completion else {
                break;
            };

            emit_output(RunnerOutput::ToolEnd {
                call_id: call.id.clone(),
                name: call.name.clone(),
                result: result.result.clone(),
            });
            let (message, changed) = self.record_tool_result(&call, result.result, result.patch)?;
            emit_tool_message_outputs(messages, message, &self.state, changed, emit_output);
        }

        if self.has_pending_tool_calls() {
            anyhow::bail!("provider turn ended with unresolved tool call results")
        }

        Ok(())
    }

    pub async fn execute_turn_with_outputs<AP, APFut, Q, QFut>(
        &mut self,
        messages: &mut Vec<Message>,
        step: &mut usize,
        approve: &mut AP,
        ask_question: &mut Q,
    ) -> anyhow::Result<(Option<String>, Vec<RunnerOutput>)>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
    {
        self.execute_turn_with_outputs_cancellable(
            messages,
            step,
            approve,
            ask_question,
            &mut || std::future::pending::<()>(),
        )
        .await
    }

    pub async fn execute_turn_with_outputs_cancellable<AP, APFut, Q, QFut, C, CFut>(
        &mut self,
        messages: &mut Vec<Message>,
        step: &mut usize,
        approve: &mut AP,
        ask_question: &mut Q,
        cancel: &mut C,
    ) -> anyhow::Result<(Option<String>, Vec<RunnerOutput>)>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
        C: FnMut() -> CFut,
        CFut: Future<Output = ()> + Send,
    {
        let mut outputs = Vec::new();
        let result = self
            .execute_turn_with_output_sink(
                messages,
                step,
                approve,
                ask_question,
                &mut |output| outputs.push(output),
                cancel,
            )
            .await?;
        Ok((result, outputs))
    }

    pub async fn run_input_loop<D>(
        &mut self,
        messages: &mut Vec<Message>,
        mut input_rx: mpsc::Receiver<RunnerInput>,
        emit_output: &mut (impl FnMut(RunnerOutput) + Send),
        mut drain_pending_messages: D,
    ) -> anyhow::Result<Option<String>>
    where
        D: FnMut() -> Vec<Message>,
    {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut pending_messages = VecDeque::<Message>::new();
        let pending_approvals = std::sync::Arc::new(std::sync::Mutex::new(VecDeque::<(
            String,
            ApprovalChoice,
        )>::new()));
        let pending_approvals_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let pending_answers = std::sync::Arc::new(std::sync::Mutex::new(VecDeque::<(
            String,
            QuestionAnswers,
        )>::new()));
        let pending_answers_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut step = 0usize;

        let mut approve_from_input = |_request: ApprovalRequest| {
            let pending_approvals = pending_approvals.clone();
            let pending_approvals_notify = pending_approvals_notify.clone();
            async move {
                loop {
                    if let Ok(mut queued) = pending_approvals.lock()
                        && let Some((_call_id, choice)) = queued.pop_front()
                    {
                        return Ok(choice);
                    }
                    pending_approvals_notify.notified().await;
                }
            }
        };

        let mut question_from_input = |questions: Vec<QuestionPrompt>| {
            let pending_answers = pending_answers.clone();
            let pending_answers_notify = pending_answers_notify.clone();
            async move {
                let _ = questions;
                loop {
                    if let Ok(mut queued) = pending_answers.lock()
                        && let Some((_call_id, answers)) = queued.pop_front()
                    {
                        return Ok(answers);
                    }
                    pending_answers_notify.notified().await;
                }
            }
        };

        let mut first_message = None;
        while first_message.is_none() {
            match input_rx.recv().await {
                Some(RunnerInput::Message(message)) => first_message = Some(message),
                Some(RunnerInput::ApprovalDecision { call_id, choice }) => {
                    enqueue_approval(
                        &pending_approvals,
                        &pending_approvals_notify,
                        call_id,
                        choice,
                    );
                }
                Some(RunnerInput::QuestionAnswered { call_id, answers }) => {
                    enqueue_answer(&pending_answers, &pending_answers_notify, call_id, answers);
                }
                Some(RunnerInput::Cancel) => {
                    let _ = cancel_tx.send(true);
                    break;
                }
                None => break,
            }
        }

        let Some(first_message) = first_message else {
            if *cancel_rx.borrow() {
                emit_output(RunnerOutput::Cancelled);
                anyhow::bail!(CANCELLATION_ERROR_MESSAGE)
            }
            return Ok(None);
        };
        messages.push(first_message.clone());
        emit_output(RunnerOutput::MessageAdded(first_message));

        loop {
            if *cancel_rx.borrow() {
                emit_output(RunnerOutput::Cancelled);
                anyhow::bail!(CANCELLATION_ERROR_MESSAGE)
            }

            while let Some(message) = pending_messages.pop_front() {
                messages.push(message.clone());
                emit_output(RunnerOutput::MessageAdded(message));
            }

            for message in drain_pending_messages() {
                messages.push(message.clone());
                emit_output(RunnerOutput::MessageAdded(message));
            }

            let mut cancel = {
                let cancel_rx = cancel_rx.clone();
                move || wait_for_cancel(cancel_rx.clone())
            };

            let turn_future = self.execute_turn_with_output_sink(
                messages,
                &mut step,
                &mut approve_from_input,
                &mut question_from_input,
                emit_output,
                &mut cancel,
            );
            tokio::pin!(turn_future);

            let maybe_answer = loop {
                for message in drain_pending_messages() {
                    pending_messages.push_back(message);
                }

                tokio::select! {
                    turn_result = &mut turn_future => {
                        break turn_result?;
                    }
                    maybe_input = input_rx.recv() => {
                        match maybe_input {
                            Some(RunnerInput::Message(message)) => {
                                pending_messages.push_back(message);
                            }
                            Some(RunnerInput::ApprovalDecision { call_id, choice }) => {
                                enqueue_approval(
                                    &pending_approvals,
                                    &pending_approvals_notify,
                                    call_id,
                                    choice,
                                );
                            }
                            Some(RunnerInput::QuestionAnswered { call_id, answers }) => {
                                enqueue_answer(
                                    &pending_answers,
                                    &pending_answers_notify,
                                    call_id,
                                    answers,
                                );
                            }
                            Some(RunnerInput::Cancel) => {
                                let _ = cancel_tx.send(true);
                            }
                            None => {}
                        }
                    }
                }
            };

            if maybe_answer.is_some() {
                return Ok(maybe_answer);
            }
        }
    }

    async fn execute_turn_with_output_sink<AP, APFut, Q, QFut, C, CFut>(
        &mut self,
        messages: &mut Vec<Message>,
        step: &mut usize,
        approve: &mut AP,
        ask_question: &mut Q,
        emit_output: &mut (impl FnMut(RunnerOutput) + Send),
        cancel: &mut C,
    ) -> anyhow::Result<Option<String>>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
        C: FnMut() -> CFut,
        CFut: Future<Output = ()> + Send,
    {
        if self.core.max_steps() > 0 && *step >= self.core.max_steps() {
            anyhow::bail!("Reached max steps without final answer")
        }

        let (turn_result, thinking_content, cancelled) = {
            let mut thinking_content = String::new();
            let (core_tx, mut core_rx) = mpsc::channel::<CoreOutput>(256);
            let overflow_outputs =
                std::sync::Arc::new(std::sync::Mutex::new(VecDeque::<CoreOutput>::new()));
            let request_messages = messages.clone();
            let overflow_for_emit = overflow_outputs.clone();
            let complete_turn =
                self.complete_turn(&request_messages, |output| match core_tx.try_send(output) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(output)) => {
                        if !is_coalescible_core_output(&output)
                            && let Ok(mut overflow) = overflow_for_emit.lock()
                        {
                            overflow.push_back(output);
                        }
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {}
                });
            tokio::pin!(complete_turn);

            let mut apply_core_output = |output: CoreOutput| match output {
                CoreOutput::ThinkingDelta(delta) => {
                    thinking_content.push_str(&delta);
                    emit_output(RunnerOutput::ThinkingDelta(delta));
                }
                CoreOutput::AssistantDelta(delta) => {
                    emit_output(RunnerOutput::AssistantDelta(delta));
                }
                CoreOutput::ToolCallRequested(_) => {}
                CoreOutput::MessageAdded(_) => {}
                CoreOutput::TurnComplete => {}
                CoreOutput::Error(payload) => {
                    emit_output(RunnerOutput::Error(payload));
                }
                CoreOutput::ContextUsage(_) => {}
            };

            let mut cancelled = false;
            let turn_result = loop {
                if let Ok(mut overflow) = overflow_outputs.lock()
                    && let Some(output) = overflow.pop_front()
                {
                    apply_core_output(output);
                    continue;
                }

                tokio::select! {
                    _ = cancel() => {
                        cancelled = true;
                        break None;
                    }
                    maybe_output = core_rx.recv() => {
                        let Some(output) = maybe_output else {
                            continue;
                        };
                        apply_core_output(output);
                    }
                    turn = &mut complete_turn => {
                        let turn = match turn {
                            Ok(turn) => turn,
                            Err(err) => {
                                while let Ok(output) = core_rx.try_recv() {
                                    apply_core_output(output);
                                }
                                if let Ok(mut overflow) = overflow_outputs.lock() {
                                    while let Some(output) = overflow.pop_front() {
                                        apply_core_output(output);
                                    }
                                }
                                return Err(err);
                            }
                        };
                        while let Ok(output) = core_rx.try_recv() {
                            apply_core_output(output);
                        }
                        if let Ok(mut overflow) = overflow_outputs.lock() {
                            while let Some(output) = overflow.pop_front() {
                                apply_core_output(output);
                            }
                        }
                        break Some(turn);
                    }
                }
            };

            (turn_result, thinking_content, cancelled)
        };

        if cancelled {
            self.send_core_input(CoreInput::Cancel)?;
            emit_output(RunnerOutput::Cancelled);
            anyhow::bail!(CANCELLATION_ERROR_MESSAGE)
        }
        let turn = turn_result.ok_or_else(|| anyhow::anyhow!("provider turn cancelled"))?;

        emit_output(RunnerOutput::StateUpdated(self.state.clone()));

        if !thinking_content.is_empty() {
            emit_output(RunnerOutput::ThinkingRecorded(thinking_content));
        }

        messages.push(turn.assistant_message);
        if let Some(message) = messages.last().cloned() {
            emit_output(RunnerOutput::MessageAdded(message));
        }

        if turn.done {
            emit_output(RunnerOutput::SnapshotUpdated(self.state.clone()));
            emit_output(RunnerOutput::TurnComplete);
            return Ok(Some(turn.assistant_content));
        }

        self.process_tool_calls_cancellable(
            messages,
            turn.tool_calls,
            approve,
            ask_question,
            emit_output,
            cancel,
        )
        .await?;

        *step += 1;
        Ok(None)
    }

    pub fn state_for_llm(&self) -> Option<Message> {
        if self.state.todo_items.is_empty() {
            return None;
        }

        let mut lines = Vec::new();
        lines.push("Runtime TODO state: use this as the canonical plan snapshot.".to_string());

        let total = self.state.todo_items.len();
        let pending = self
            .state
            .todo_items
            .iter()
            .filter(|item| {
                matches!(
                    item.status,
                    crate::core::TodoStatus::Pending | crate::core::TodoStatus::InProgress
                )
            })
            .count();
        lines.push(format!("{pending} pending out of {total} total tasks."));

        for item in &self.state.todo_items {
            let status = match item.status {
                crate::core::TodoStatus::Pending => "pending",
                crate::core::TodoStatus::InProgress => "in_progress",
                crate::core::TodoStatus::Completed => "completed",
                crate::core::TodoStatus::Cancelled => "cancelled",
            };
            lines.push(format!("- [{status}] {}", item.content));
        }

        Some(Message {
            role: Role::System,
            content: lines.join("\n"),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        })
    }
}

fn decorate_tool_start_args(call_id: &str, name: &str, args: &Value) -> Value {
    if name != "task" {
        return args.clone();
    }
    let mut obj = args.as_object().cloned().unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    obj.insert("__started_at".to_string(), Value::from(now));
    obj.insert("__call_id".to_string(), Value::from(call_id.to_string()));
    Value::Object(obj)
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

fn session_approval_key(tool_name: &str, action: &Value) -> String {
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

fn bash_permission_rule_from_action(action: &Value) -> Option<&str> {
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

fn bash_rule_matches_call(rule: &str, args: &Value) -> bool {
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

#[derive(Debug, serde::Serialize)]
struct TodoSnapshotCounts {
    total: usize,
    pending: usize,
    in_progress: usize,
    completed: usize,
    cancelled: usize,
}

#[derive(Debug, serde::Serialize)]
struct TodoSnapshotOutput {
    todos: Vec<crate::core::TodoItem>,
    counts: TodoSnapshotCounts,
}

#[derive(Debug, Deserialize)]
struct TodoWriteOutput {
    todos: Vec<crate::core::TodoItem>,
}

fn parse_todos_from_tool_result(result: &ToolResult) -> Option<Vec<crate::core::TodoItem>> {
    if let Ok(parsed) = serde_json::from_value::<TodoWriteOutput>(result.payload.clone()) {
        return Some(parsed.todos);
    }

    serde_json::from_str::<TodoWriteOutput>(&result.output)
        .ok()
        .map(|parsed| parsed.todos)
}

pub fn apply_tool_outcome(
    state: &mut RunnerState,
    tool_name: &str,
    result: &ToolResult,
    patch: StatePatch,
) -> bool {
    let mut changed = state.apply_patch(patch);

    if !result.is_error
        && tool_name == "todo_write"
        && let Some(items) = parse_todos_from_tool_result(result)
        && state.todo_items != items
    {
        state.todo_items = items;
        changed = true;
    }

    changed
}

pub async fn execute_non_blocking_tool<T: ToolExecutor>(
    tools: &T,
    call: ToolCall,
    execution_args: Value,
) -> (ToolCall, ToolExecution) {
    let mut result = tools.execute(&call.name, execution_args).await;
    result.result.output = sanitize_tool_output(&result.result.output);
    (call, result)
}

fn emit_lifecycle_outputs(
    emit_output: &mut (impl FnMut(RunnerOutput) + Send),
    lifecycle_outputs: std::sync::Mutex<Vec<RunnerOutput>>,
) {
    for output in lifecycle_outputs
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
    {
        emit_output(output);
    }
}

fn emit_tool_message_outputs(
    messages: &mut Vec<Message>,
    message: Message,
    state: &RunnerState,
    changed: bool,
    emit_output: &mut (impl FnMut(RunnerOutput) + Send),
) {
    let message_for_output = message.clone();
    messages.push(message);
    emit_output(RunnerOutput::MessageAdded(message_for_output));
    emit_output(RunnerOutput::SnapshotUpdated(state.clone()));
    if changed {
        emit_output(RunnerOutput::StateUpdated(state.clone()));
    }
}

fn enqueue_approval(
    pending_approvals: &std::sync::Arc<std::sync::Mutex<VecDeque<(String, ApprovalChoice)>>>,
    notify: &std::sync::Arc<tokio::sync::Notify>,
    call_id: String,
    choice: ApprovalChoice,
) {
    if let Ok(mut queued) = pending_approvals.lock() {
        queued.push_back((call_id, choice));
        notify.notify_one();
    }
}

fn enqueue_answer(
    pending_answers: &std::sync::Arc<std::sync::Mutex<VecDeque<(String, QuestionAnswers)>>>,
    notify: &std::sync::Arc<tokio::sync::Notify>,
    call_id: String,
    answers: QuestionAnswers,
) {
    if let Ok(mut queued) = pending_answers.lock() {
        queued.push_back((call_id, answers));
        notify.notify_one();
    }
}

async fn wait_for_cancel(mut cancel_rx: watch::Receiver<bool>) {
    if *cancel_rx.borrow() {
        return;
    }
    let _ = cancel_rx.changed().await;
}

fn is_coalescible_core_output(output: &CoreOutput) -> bool {
    matches!(
        output,
        CoreOutput::ThinkingDelta(_) | CoreOutput::AssistantDelta(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        ApprovalDecision, ApprovalPolicy, Message, Provider, ProviderRequest, ProviderResponse,
        Role, TodoItem, TodoPriority, TodoStatus, ToolCall, ToolExecutor,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::{mpsc, watch};
    use tokio::time::{Duration, sleep, timeout};

    struct TestProvider {
        responses: Mutex<VecDeque<ProviderResponse>>,
    }

    struct CapturingProvider {
        responses: Mutex<VecDeque<ProviderResponse>>,
        requests: Arc<Mutex<Vec<ProviderRequest>>>,
    }

    struct HangingProvider {
        dropped: Arc<AtomicBool>,
    }

    struct HangingStreamProvider {
        dropped: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Provider for TestProvider {
        async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            self.responses
                .lock()
                .expect("provider responses lock")
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted provider response remaining"))
        }
    }

    #[async_trait]
    impl Provider for CapturingProvider {
        async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            self.requests
                .lock()
                .expect("captured requests lock")
                .push(req);
            self.responses
                .lock()
                .expect("provider responses lock")
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted provider response remaining"))
        }
    }

    #[async_trait]
    impl Provider for HangingProvider {
        async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            let _drop_signal = DropSignal {
                dropped: Arc::clone(&self.dropped),
            };
            sleep(Duration::from_secs(30)).await;
            Ok(ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "late".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                tool_calls: Vec::new(),
                done: true,
                thinking: None,
                context_tokens: None,
            })
        }
    }

    #[async_trait]
    impl Provider for HangingStreamProvider {
        async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            anyhow::bail!("complete() should not be used for stream provider")
        }

        async fn complete_stream<F>(
            &self,
            _req: ProviderRequest,
            _on_event: F,
        ) -> anyhow::Result<ProviderResponse>
        where
            F: FnMut(crate::core::ProviderStreamEvent) + Send,
        {
            let _drop_signal = DropSignal {
                dropped: Arc::clone(&self.dropped),
            };
            sleep(Duration::from_secs(30)).await;
            Ok(ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "late".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                tool_calls: Vec::new(),
                done: true,
                thinking: None,
                context_tokens: None,
            })
        }
    }

    struct TestApprovals;

    impl ApprovalPolicy for TestApprovals {
        fn decision_for_tool_call(&self, _tool_name: &str, _args: &Value) -> ApprovalDecision {
            ApprovalDecision::Allow
        }
    }

    struct DelayedNonBlockingTools;

    #[async_trait]
    impl ToolExecutor for DelayedNonBlockingTools {
        fn schemas(&self) -> Vec<crate::tool::schema::ToolSchema> {
            Vec::new()
        }

        async fn execute(&self, name: &str, _args: Value) -> ToolExecution {
            match name {
                "slow_tool" => sleep(Duration::from_millis(80)).await,
                "fast_tool" => sleep(Duration::from_millis(1)).await,
                _ => {}
            }

            ToolExecution::from_result(ToolResult::ok_text(
                format!("{name} ok"),
                format!("{name} output"),
            ))
        }

        fn is_non_blocking(&self, name: &str) -> bool {
            matches!(name, "slow_tool" | "fast_tool")
        }
    }

    struct SlowTodoReadTools;

    #[async_trait]
    impl ToolExecutor for SlowTodoReadTools {
        fn schemas(&self) -> Vec<crate::tool::schema::ToolSchema> {
            Vec::new()
        }

        async fn execute(&self, name: &str, _args: Value) -> ToolExecution {
            if name == "todo_read" {
                sleep(Duration::from_millis(40)).await;
            }

            ToolExecution::from_result(ToolResult::ok_text(
                format!("{name} ok"),
                format!("{name} output"),
            ))
        }
    }

    struct DropSignal {
        dropped: Arc<AtomicBool>,
    }

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    struct HangingNonBlockingTools {
        dropped: Arc<AtomicBool>,
    }

    #[async_trait]
    impl ToolExecutor for HangingNonBlockingTools {
        fn schemas(&self) -> Vec<crate::tool::schema::ToolSchema> {
            Vec::new()
        }

        async fn execute(&self, _name: &str, _args: Value) -> ToolExecution {
            let _drop_signal = DropSignal {
                dropped: Arc::clone(&self.dropped),
            };
            sleep(Duration::from_secs(30)).await;
            ToolExecution::from_result(ToolResult::ok_text("unexpected", "should not complete"))
        }

        fn is_non_blocking(&self, name: &str) -> bool {
            name == "slow_tool"
        }
    }

    struct TestData;

    impl TestData {
        fn user_message(content: &str) -> Message {
            Message {
                role: Role::User,
                content: content.to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            }
        }

        fn assistant_message(content: &str, tool_calls: Vec<ToolCall>) -> Message {
            Message {
                role: Role::Assistant,
                content: content.to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls,
            }
        }

        fn tool_call(id: &str, name: &str, arguments: Value) -> ToolCall {
            ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments,
            }
        }

        fn provider_response(
            content: &str,
            tool_calls: Vec<ToolCall>,
            done: bool,
            context_tokens: Option<usize>,
        ) -> ProviderResponse {
            ProviderResponse {
                assistant_message: Self::assistant_message(content, tool_calls.clone()),
                tool_calls,
                done,
                thinking: None,
                context_tokens,
            }
        }
    }

    fn mock_provider_with_responses(responses: Vec<ProviderResponse>) -> TestProvider {
        TestProvider {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }

    fn test_turn_state_with_user(content: &str) -> TurnState {
        TurnState {
            messages: vec![TestData::user_message(content)],
            ..Default::default()
        }
    }

    async fn test_ask_question_not_expected(
        _questions: Vec<QuestionPrompt>,
    ) -> anyhow::Result<Vec<Vec<String>>> {
        anyhow::bail!("question tool should not be called in this test")
    }

    fn test_spawn_cancel_after(delay: Duration) -> watch::Receiver<bool> {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        tokio::spawn(async move {
            sleep(delay).await;
            let _ = cancel_tx.send(true);
        });
        cancel_rx
    }

    async fn test_wait_for_cancel_signal(mut cancel_rx: watch::Receiver<bool>) {
        if *cancel_rx.borrow() {
            return;
        }
        let _ = cancel_rx.changed().await;
    }

    #[test]
    fn apply_tool_outcome_updates_todos_from_patch() {
        let mut state = RunnerState::default();
        let items = vec![TodoItem {
            content: "from patch".to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::Medium,
        }];

        let changed = apply_tool_outcome(
            &mut state,
            "todo_write",
            &ToolResult::ok_text("ok", "ok"),
            StatePatch::with_op(StateOp::SetTodoItems {
                items: items.clone(),
            }),
        );

        assert!(changed);
        assert_eq!(state.todo_items, items);
    }

    #[test]
    fn apply_tool_outcome_fallback_parses_todo_write_payload() {
        let mut state = RunnerState::default();
        let result = ToolResult::ok_json(
            "todo list updated",
            serde_json::json!({
                "todos": [{
                    "content": "from payload",
                    "status": "in_progress",
                    "priority": "low"
                }]
            }),
        );

        let changed = apply_tool_outcome(&mut state, "todo_write", &result, StatePatch::none());

        assert!(changed);
        assert_eq!(state.todo_items.len(), 1);
        assert_eq!(state.todo_items[0].content, "from payload");
        assert_eq!(state.todo_items[0].status, TodoStatus::InProgress);
    }

    #[tokio::test]
    async fn process_tool_calls_correlates_out_of_order_non_blocking_results_by_call_id() {
        let tool_calls = vec![
            TestData::tool_call("call-slow", "slow_tool", json!({})),
            TestData::tool_call("call-fast", "fast_tool", json!({})),
        ];
        let provider = mock_provider_with_responses(vec![TestData::provider_response(
            "",
            tool_calls.clone(),
            false,
            None,
        )]);
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let mut state = test_turn_state_with_user("run tools");

        let turn = runner
            .complete_turn(&state.messages, |_output| {})
            .await
            .expect("seed pending tool calls");
        let tool_calls = turn.tool_calls;

        let mut outputs = Vec::new();
        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = test_ask_question_not_expected;

        runner
            .process_tool_calls(
                &mut state.messages,
                tool_calls,
                &mut approve,
                &mut ask_question,
                &mut |output| outputs.push(output),
            )
            .await
            .expect("process tool calls");

        let tool_ends = outputs
            .iter()
            .filter_map(|output| {
                if let RunnerOutput::ToolEnd {
                    call_id,
                    name,
                    result,
                } = output
                {
                    Some((call_id.clone(), name.clone(), result.output.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_ends.len(), 2);
        assert_eq!(tool_ends[0].0, "call-fast");
        assert_eq!(tool_ends[0].1, "fast_tool");
        assert_eq!(tool_ends[0].2, "fast_tool output");
        assert_eq!(tool_ends[1].0, "call-slow");
        assert_eq!(tool_ends[1].1, "slow_tool");
        assert_eq!(tool_ends[1].2, "slow_tool output");

        let tool_messages = state
            .messages
            .iter()
            .filter_map(|message| {
                (message.role == Role::Tool).then(|| {
                    (
                        message.tool_call_id.clone().unwrap_or_default(),
                        message.content.clone(),
                    )
                })
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_messages.len(), 2);
        assert_eq!(
            tool_messages[0],
            ("call-fast".to_string(), "fast_tool output".to_string())
        );
        assert_eq!(
            tool_messages[1],
            ("call-slow".to_string(), "slow_tool output".to_string())
        );
    }

    #[tokio::test]
    async fn execute_turn_cancellation_stops_inflight_non_blocking_tools_and_clears_pending_calls()
    {
        let provider = mock_provider_with_responses(vec![TestData::provider_response(
            "",
            vec![TestData::tool_call("call-slow", "slow_tool", json!({}))],
            false,
            None,
        )]);
        let dropped = Arc::new(AtomicBool::new(false));
        let tools = HangingNonBlockingTools {
            dropped: Arc::clone(&dropped),
        };
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let mut state = test_turn_state_with_user("run slow tool");

        let cancel_rx = test_spawn_cancel_after(Duration::from_millis(25));

        let mut cancel = {
            let cancel_rx = cancel_rx.clone();
            move || {
                let cancel_rx = cancel_rx.clone();
                async move { test_wait_for_cancel_signal(cancel_rx).await }
            }
        };

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = test_ask_question_not_expected;

        let result = timeout(
            Duration::from_millis(250),
            runner.execute_turn_with_outputs_cancellable(
                &mut state.messages,
                &mut state.step,
                &mut approve,
                &mut ask_question,
                &mut cancel,
            ),
        )
        .await
        .expect("runner cancellation should resolve quickly");

        let err = result.expect_err("turn should be cancelled");
        assert!(err.to_string().contains("cancelled"));
        assert!(!runner.has_pending_tool_calls());

        sleep(Duration::from_millis(10)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn execute_turn_cancellation_drops_inflight_provider_future() {
        let dropped = Arc::new(AtomicBool::new(false));
        let provider = HangingProvider {
            dropped: Arc::clone(&dropped),
        };
        let tools = SlowTodoReadTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let mut state = test_turn_state_with_user("hang provider");

        let cancel_rx = test_spawn_cancel_after(Duration::from_millis(25));

        let mut cancel = {
            let cancel_rx = cancel_rx.clone();
            move || {
                let cancel_rx = cancel_rx.clone();
                async move { test_wait_for_cancel_signal(cancel_rx).await }
            }
        };

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = test_ask_question_not_expected;

        let result = timeout(
            Duration::from_millis(250),
            runner.execute_turn_with_outputs_cancellable(
                &mut state.messages,
                &mut state.step,
                &mut approve,
                &mut ask_question,
                &mut cancel,
            ),
        )
        .await
        .expect("runner cancellation should resolve quickly");

        let err = result.expect_err("turn should be cancelled");
        assert!(is_cancellation_error(&err));

        sleep(Duration::from_millis(10)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn execute_turn_cancellation_drops_inflight_provider_stream_future() {
        let dropped = Arc::new(AtomicBool::new(false));
        let provider = HangingStreamProvider {
            dropped: Arc::clone(&dropped),
        };
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let mut state = test_turn_state_with_user("hang provider stream");

        let cancel_rx = test_spawn_cancel_after(Duration::from_millis(25));

        let mut cancel = {
            let cancel_rx = cancel_rx.clone();
            move || {
                let cancel_rx = cancel_rx.clone();
                async move { test_wait_for_cancel_signal(cancel_rx).await }
            }
        };

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = test_ask_question_not_expected;

        let result = timeout(
            Duration::from_millis(250),
            runner.execute_turn_with_outputs_cancellable(
                &mut state.messages,
                &mut state.step,
                &mut approve,
                &mut ask_question,
                &mut cancel,
            ),
        )
        .await
        .expect("runner cancellation should resolve quickly");

        let err = result.expect_err("turn should be cancelled");
        assert!(is_cancellation_error(&err));

        sleep(Duration::from_millis(10)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn run_input_loop_processes_message_and_returns_final_answer() {
        let provider = mock_provider_with_responses(vec![TestData::provider_response(
            "final answer",
            Vec::new(),
            true,
            None,
        )]);
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        tx.send(RunnerInput::Message(TestData::user_message("hello")))
            .await
            .expect("send message");

        let mut outputs = Vec::new();
        let mut state = TurnState::default();

        let answer = runner
            .run_input_loop(
                &mut state.messages,
                rx,
                &mut |output| outputs.push(output),
                Vec::new,
            )
            .await
            .expect("run input loop");

        assert_eq!(answer.as_deref(), Some("final answer"));
        assert!(
            outputs
                .iter()
                .any(|o| matches!(o, RunnerOutput::TurnComplete))
        );
    }

    #[tokio::test]
    async fn run_input_loop_cancel_interrupts_hanging_provider() {
        let dropped = Arc::new(AtomicBool::new(false));
        let provider = HangingProvider {
            dropped: Arc::clone(&dropped),
        };
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        tx.send(RunnerInput::Message(TestData::user_message("hello")))
            .await
            .expect("send message");
        tokio::spawn(async move {
            sleep(Duration::from_millis(25)).await;
            let _ = tx.send(RunnerInput::Cancel).await;
        });

        let mut state = TurnState::default();

        let result = timeout(
            Duration::from_millis(250),
            runner.run_input_loop(
                &mut state.messages,
                rx,
                &mut |_output| {},
                Vec::new,
            ),
        )
        .await
        .expect("run loop should resolve quickly");

        let err = result.expect_err("run should be cancelled");
        assert!(is_cancellation_error(&err));
        sleep(Duration::from_millis(10)).await;
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn run_input_loop_cancel_before_first_message_returns_cancelled() {
        let provider = mock_provider_with_responses(Vec::new());
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        tx.send(RunnerInput::Cancel).await.expect("send cancel");

        let mut state = TurnState::default();

        let result = runner
            .run_input_loop(
                &mut state.messages,
                rx,
                &mut |_output| {},
                Vec::new,
            )
            .await;

        let err = result.expect_err("run should be cancelled");
        assert!(is_cancellation_error(&err));
    }

    #[tokio::test]
    async fn run_input_loop_returns_none_when_input_channel_closes_without_message() {
        let provider = mock_provider_with_responses(Vec::new());
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        drop(tx);

        let mut state = TurnState::default();

        let result = runner
            .run_input_loop(
                &mut state.messages,
                rx,
                &mut |_output| {},
                Vec::new,
            )
            .await
            .expect("run input loop");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn run_input_loop_emits_error_output_on_provider_failure() {
        let provider = mock_provider_with_responses(Vec::new());
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        tx.send(RunnerInput::Message(TestData::user_message("start")))
            .await
            .expect("send message");

        let mut state = TurnState::default();
        let mut outputs = Vec::new();

        let result = runner
            .run_input_loop(
                &mut state.messages,
                rx,
                &mut |output| outputs.push(output),
                Vec::new,
            )
            .await;

        assert!(result.is_err());
        assert!(
            outputs
                .iter()
                .any(|output| matches!(output, RunnerOutput::Error(_)))
        );
    }

    #[tokio::test]
    async fn run_input_loop_cancel_clears_pending_tool_calls() {
        let provider = mock_provider_with_responses(vec![TestData::provider_response(
            "",
            vec![TestData::tool_call("call-1", "slow_tool", json!({}))],
            false,
            None,
        )]);
        let tools = HangingNonBlockingTools {
            dropped: Arc::new(AtomicBool::new(false)),
        };
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        tx.send(RunnerInput::Message(TestData::user_message("start")))
            .await
            .expect("send message");
        tokio::spawn(async move {
            sleep(Duration::from_millis(25)).await;
            let _ = tx.send(RunnerInput::Cancel).await;
        });

        let mut state = TurnState::default();

        let result = timeout(
            Duration::from_millis(250),
            runner.run_input_loop(
                &mut state.messages,
                rx,
                &mut |_output| {},
                Vec::new,
            ),
        )
        .await
        .expect("run loop should resolve quickly");

        let err = result.expect_err("run should be cancelled");
        assert!(is_cancellation_error(&err));
        assert!(!runner.has_pending_tool_calls());
    }

    #[tokio::test]
    async fn run_input_loop_includes_messages_from_pending_drain_between_turns() {
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let provider = CapturingProvider {
            responses: Mutex::new(VecDeque::from(vec![
                TestData::provider_response(
                    "",
                    vec![TestData::tool_call("call-1", "todo_read", json!({}))],
                    false,
                    None,
                ),
                TestData::provider_response("done", Vec::new(), true, None),
            ])),
            requests: Arc::clone(&captured_requests),
        };

        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let (tx, rx) = mpsc::channel(8);
        tx.send(RunnerInput::Message(TestData::user_message("initial")))
            .await
            .expect("send initial message");

        let mut state = TurnState::default();
        let drain_call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let answer = runner
            .run_input_loop(
                &mut state.messages,
                rx,
                &mut |_output| {},
                {
                    let drain_call_count = Arc::clone(&drain_call_count);
                    move || {
                        let call_index =
                            drain_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if call_index == 0 {
                            return Vec::new();
                        }

                        vec![TestData::user_message("follow up")]
                    }
                },
            )
            .await
            .expect("run input loop");

        assert_eq!(answer.as_deref(), Some("done"));

        let requests = captured_requests.lock().expect("captured requests");
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1]
                .messages
                .iter()
                .any(|message| message.role == Role::User && message.content == "follow up")
        );
    }

    #[tokio::test]
    async fn process_tool_calls_emits_snapshot_updated_after_tool_result() {
        let provider = mock_provider_with_responses(vec![TestData::provider_response(
            "",
            vec![TestData::tool_call("call-1", "todo_read", json!({}))],
            false,
            None,
        )]);
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let mut state = test_turn_state_with_user("run");

        let turn = runner
            .complete_turn(&state.messages, |_output| {})
            .await
            .expect("complete turn");

        let mut outputs = Vec::new();
        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = test_ask_question_not_expected;

        runner
            .process_tool_calls(
                &mut state.messages,
                turn.tool_calls,
                &mut approve,
                &mut ask_question,
                &mut |output| outputs.push(output),
            )
            .await
            .expect("process tool calls");

        assert!(
            outputs
                .iter()
                .any(|output| matches!(output, RunnerOutput::SnapshotUpdated(_)))
        );
    }

    #[tokio::test]
    async fn execute_turn_emits_snapshot_updated_before_turn_complete() {
        let provider = mock_provider_with_responses(vec![TestData::provider_response(
            "done",
            Vec::new(),
            true,
            Some(5),
        )]);
        let tools = DelayedNonBlockingTools;
        let approvals = TestApprovals;
        let core = AgentCore::new(&provider, "test".to_string(), String::new(), Vec::new(), 10);
        let mut runner = AgentRunner::new(core, &tools, &approvals, RunnerState::default());

        let mut state = test_turn_state_with_user("run");

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = test_ask_question_not_expected;

        let (answer, outputs) = runner
            .execute_turn_with_outputs(
                &mut state.messages,
                &mut state.step,
                &mut approve,
                &mut ask_question,
            )
            .await
            .expect("execute turn");

        assert_eq!(answer.as_deref(), Some("done"));
        let snapshot_index = outputs
            .iter()
            .position(|output| matches!(output, RunnerOutput::SnapshotUpdated(_)))
            .expect("snapshot updated output present");
        let turn_complete_index = outputs
            .iter()
            .position(|output| matches!(output, RunnerOutput::TurnComplete))
            .expect("turn complete output present");
        assert!(snapshot_index < turn_complete_index);
    }
}
