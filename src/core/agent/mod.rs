pub mod core;
pub mod output_channel;
pub mod runner;
pub mod subagent_manager;
pub mod types;

pub use types::{
    CoreInput, CoreOutput, ErrorPayload, RunnerInput, RunnerOutput, RunnerState, StateOp,
    StatePatch,
};

use self::core::AgentCore as EngineCore;
use self::runner::AgentRunner;
use crate::core::{
    ApprovalChoice, ApprovalPolicy, ApprovalRequest, Message, Provider, QuestionAnswers,
    QuestionPrompt, SessionReader, SessionSink, ToolExecutor,
};
use crate::session::{SessionEvent, event_id};
use std::collections::VecDeque;
use std::future::Future;

type RunnerOutputQueue = std::sync::Arc<std::sync::Mutex<VecDeque<RunnerOutput>>>;
type EmitErrorSlot = std::sync::Arc<std::sync::Mutex<Option<String>>>;
type RunnerInputSender = tokio::sync::mpsc::Sender<RunnerInput>;

pub trait RunnerOutputObserver: Send + Sync {
    fn on_thinking(&self, _text: &str) {}
    fn on_tool_start(&self, _name: &str, _args: &serde_json::Value) {}
    fn on_tool_end(&self, _name: &str, _result: &crate::tool::ToolResult) {}
    fn on_approval_required(&self, _call_id: &str, _request: &crate::core::ApprovalRequest) {}
    fn on_question_required(&self, _call_id: &str, _prompts: &[crate::core::QuestionPrompt]) {}
    fn on_cancelled(&self) {}
    fn on_runner_state_updated(&self, _state: &crate::core::agent::RunnerState) {}
    fn on_assistant_delta(&self, _delta: &str) {}
    fn on_error(&self, _message: &str) {}
    fn on_assistant_done(&self) {}
}

impl RunnerOutputObserver for () {}

pub struct AgentCore<P, T, A, S>
where
    P: Provider,
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
}

impl<P, T, A, S> AgentCore<P, T, A, S>
where
    P: Provider,
    T: ToolExecutor,
    A: ApprovalPolicy,
    S: SessionSink + SessionReader,
{
    pub async fn run<AP, APFut>(
        &self,
        initial_messages: Vec<Message>,
        mut approve: AP,
    ) -> anyhow::Result<String>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
    {
        let mut ask_question = |_questions: Vec<QuestionPrompt>| async {
            anyhow::bail!("question tool is unavailable in this mode; provide a question handler")
        };
        let initial_runner_state = self
            .session
            .load_runner_state_snapshot()?
            .unwrap_or_default();
        let mut last_emitted_todo_items = initial_runner_state.todo_items;
        self.run_with_runner_output_sink_cancellable(
            initial_messages,
            &mut approve,
            &mut ask_question,
            &mut || std::future::pending::<()>(),
            &mut |output| {
                apply_runner_output_to_observer(
                    &(),
                    &self.session,
                    output,
                    &mut last_emitted_todo_items,
                )
            },
            &mut Vec::new,
        )
        .await
    }

    pub async fn run_with_runner_output_sink_cancellable<AP, APFut, Q, QFut, C, CFut, O, D>(
        &self,
        initial_messages: Vec<Message>,
        approve: &mut AP,
        ask_question: &mut Q,
        cancel: &mut C,
        emit_output: &mut O,
        drain_pending_messages: &mut D,
    ) -> anyhow::Result<String>
    where
        AP: FnMut(ApprovalRequest) -> APFut,
        APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
        Q: FnMut(Vec<QuestionPrompt>) -> QFut,
        QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
        C: FnMut() -> CFut,
        CFut: Future<Output = ()> + Send,
        O: FnMut(RunnerOutput) -> anyhow::Result<()> + Send,
        D: FnMut() -> Vec<Message>,
    {
        let replayed_events = self.session.replay_events()?;
        let loaded_snapshot = self.session.load_runner_state_snapshot()?;
        let runner_snapshot = loaded_snapshot.clone().unwrap_or_default();
        let mut messages = self.session.replay_messages()?;
        let core = EngineCore::new(
            &self.provider,
            self.model.clone(),
            self.system_prompt.clone(),
            self.tools.schemas(),
            self.max_steps,
        );
        let mut runner = AgentRunner::new(
            core,
            &self.tools,
            &self.approvals,
            RunnerState {
                todo_items: runner_snapshot.todo_items.clone(),
                context_tokens: runner_snapshot.context_tokens,
            },
        );
        runner
            .hydrate_state_from_replayed_tool_results(&replayed_events, loaded_snapshot.is_some());

        if runner.core.should_inject_system_prompt(&messages) {
            push_message_and_record(
                &self.session,
                &mut messages,
                runner.core.system_prompt_message(),
            )?;
        }

        let (input_tx, input_rx) = tokio::sync::mpsc::channel(64);
        for message in initial_messages {
            input_tx
                .try_send(RunnerInput::Message(message))
                .map_err(|_| anyhow::anyhow!("failed to enqueue initial runner input"))?;
        }

        let request_queue = RunnerOutputQueue::default();
        let request_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let emit_error = EmitErrorSlot::default();
        let queue_for_emit = request_queue.clone();
        let notify_for_emit = request_notify.clone();
        let emit_error_for_emit = emit_error.clone();
        let mut emit_wrapped = move |output: RunnerOutput| {
            let requires_follow_up = matches!(
                output,
                RunnerOutput::ApprovalRequired { .. } | RunnerOutput::QuestionRequired { .. }
            );

            if let Err(err) = emit_output(output.clone()) {
                if let Ok(mut slot) = emit_error_for_emit.lock()
                    && slot.is_none()
                {
                    *slot = Some(err.to_string());
                }
                notify_for_emit.notify_one();
                return;
            }

            if requires_follow_up && let Ok(mut queue) = queue_for_emit.lock() {
                queue.push_back(output);
                notify_for_emit.notify_one();
            }
        };

        let run_future = runner.run_input_loop(
            &mut messages,
            input_rx,
            &mut emit_wrapped,
            drain_pending_messages,
        );
        tokio::pin!(run_future);

        let mut cancel_sent = false;
        let run_result = loop {
            if let Some(err) = take_emit_error(&emit_error) {
                let _ = input_tx.send(RunnerInput::Cancel).await;
                return Err(anyhow::anyhow!(err));
            }

            drain_follow_up_requests(&request_queue, &input_tx, approve, ask_question).await?;

            tokio::select! {
                biased;
                _ = request_notify.notified() => {}
                _ = cancel(), if !cancel_sent => {
                    cancel_sent = true;
                    let _ = input_tx.send(RunnerInput::Cancel).await;
                }
                result = &mut run_future => break result,
            }
        };

        if let Some(err) = take_emit_error(&emit_error) {
            return Err(anyhow::anyhow!(err));
        }

        drain_follow_up_requests(&request_queue, &input_tx, approve, ask_question).await?;

        match run_result {
            Ok(Some(final_answer)) => Ok(final_answer),
            Ok(None) => anyhow::bail!("runner input loop ended without final answer"),
            Err(err) => Err(err),
        }
    }
}

fn pop_output_queue(output_queue: &RunnerOutputQueue) -> anyhow::Result<Option<RunnerOutput>> {
    let Ok(mut queue) = output_queue.lock() else {
        return Err(anyhow::anyhow!("runner output queue poisoned"));
    };
    Ok(queue.pop_front())
}

fn take_emit_error(emit_error: &EmitErrorSlot) -> Option<String> {
    let Ok(mut slot) = emit_error.lock() else {
        return Some("runner emit error slot poisoned".to_string());
    };
    slot.take()
}

async fn drain_follow_up_requests<AP, APFut, Q, QFut>(
    request_queue: &RunnerOutputQueue,
    input_tx: &RunnerInputSender,
    approve: &mut AP,
    ask_question: &mut Q,
) -> anyhow::Result<()>
where
    AP: FnMut(ApprovalRequest) -> APFut,
    APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
    Q: FnMut(Vec<QuestionPrompt>) -> QFut,
    QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
{
    while let Some(output) = pop_output_queue(request_queue)? {
        handle_follow_up_request(output, input_tx, approve, ask_question).await?;
    }

    Ok(())
}

async fn handle_follow_up_request<AP, APFut, Q, QFut>(
    output: RunnerOutput,
    input_tx: &RunnerInputSender,
    approve: &mut AP,
    ask_question: &mut Q,
) -> anyhow::Result<()>
where
    AP: FnMut(ApprovalRequest) -> APFut,
    APFut: Future<Output = anyhow::Result<ApprovalChoice>> + Send,
    Q: FnMut(Vec<QuestionPrompt>) -> QFut,
    QFut: Future<Output = anyhow::Result<QuestionAnswers>> + Send,
{
    match output {
        RunnerOutput::ApprovalRequired { call_id, request } => {
            let choice = approve(request).await?;
            input_tx
                .send(RunnerInput::ApprovalDecision { call_id, choice })
                .await
                .map_err(|_| anyhow::anyhow!("runner input channel closed"))?;
        }
        RunnerOutput::QuestionRequired { call_id, prompts } => {
            let answers = ask_question(prompts).await?;
            input_tx
                .send(RunnerInput::QuestionAnswered { call_id, answers })
                .await
                .map_err(|_| anyhow::anyhow!("runner input channel closed"))?;
        }
        _ => {}
    }

    Ok(())
}

fn push_message_and_record<S: SessionSink>(
    session: &S,
    messages: &mut Vec<Message>,
    message: Message,
) -> anyhow::Result<()> {
    messages.push(message.clone());
    session.append(&SessionEvent::Message {
        id: event_id(),
        message,
    })
}

pub fn apply_runner_output_to_observer<E: RunnerOutputObserver>(
    events: &E,
    session: &impl SessionSink,
    output: RunnerOutput,
    last_emitted_todo_items: &mut Vec<crate::core::TodoItem>,
) -> anyhow::Result<()> {
    match output {
        RunnerOutput::ThinkingDelta(delta) => events.on_thinking(&delta),
        RunnerOutput::ThinkingRecorded(content) => {
            session.append(&SessionEvent::Thinking {
                id: event_id(),
                content,
            })?;
        }
        RunnerOutput::AssistantDelta(delta) => events.on_assistant_delta(&delta),
        RunnerOutput::ToolCallRecorded(call) => {
            session.append(&SessionEvent::ToolCall { call })?;
        }
        RunnerOutput::ToolStart { name, args, .. } => events.on_tool_start(&name, &args),
        RunnerOutput::ToolEnd {
            call_id,
            name,
            result,
        } => {
            session.append(&SessionEvent::ToolResult {
                id: call_id,
                is_error: result.is_error,
                output: result.output.clone(),
                result: Some(result.clone()),
            })?;
            events.on_tool_end(&name, &result)
        }
        RunnerOutput::SnapshotUpdated(snapshot) => {
            session.save_runner_state_snapshot(&snapshot)?;
        }
        RunnerOutput::StateUpdated(updated) => {
            events.on_runner_state_updated(&updated);
            if updated.todo_items != *last_emitted_todo_items {
                *last_emitted_todo_items = updated.todo_items;
            }
        }
        RunnerOutput::TurnComplete => events.on_assistant_done(),
        RunnerOutput::Cancelled => events.on_cancelled(),
        RunnerOutput::ApprovalRequired { call_id, request } => {
            events.on_approval_required(&call_id, &request)
        }
        RunnerOutput::ApprovalRecorded {
            tool_name,
            approved,
            action,
            choice,
        } => {
            session.append(&SessionEvent::Approval {
                id: event_id(),
                tool_name,
                approved,
                action,
                choice,
            })?;
        }
        RunnerOutput::QuestionRequired { call_id, prompts } => {
            events.on_question_required(&call_id, &prompts)
        }
        RunnerOutput::MessageAdded(message) => {
            session.append(&SessionEvent::Message {
                id: event_id(),
                message,
            })?;
        }
        RunnerOutput::Error(_) => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::core::agent::runner::is_cancellation_error;
    use crate::core::{ProviderRequest, Role, ToolCall};
    use crate::permission::PermissionMatcher;
    use crate::provider::ProviderResponse;
    use crate::session::{SessionEvent, SessionStore};
    use crate::tool::registry::ToolRegistry;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::tempdir;
    use tokio::sync::watch;
    use tokio::time::{Duration, sleep, timeout};

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

    struct HangingProvider;

    #[async_trait]
    impl Provider for HangingProvider {
        async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
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

    struct BurstStreamProvider {
        response: ProviderResponse,
        bursts: usize,
    }

    #[async_trait]
    impl Provider for BurstStreamProvider {
        async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
            Ok(self.response.clone())
        }

        async fn complete_stream<F>(
            &self,
            req: ProviderRequest,
            mut on_event: F,
        ) -> anyhow::Result<ProviderResponse>
        where
            F: FnMut(crate::core::ProviderStreamEvent) + Send,
        {
            for _ in 0..self.bursts {
                on_event(crate::core::ProviderStreamEvent::ThinkingDelta(
                    "t".to_string(),
                ));
                on_event(crate::core::ProviderStreamEvent::AssistantDelta(
                    "a".to_string(),
                ));
            }
            self.complete(req).await
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

        let agent = AgentCore {
            provider,
            tools,
            approvals,
            max_steps: 10,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
        };

        let approval_count = Arc::new(Mutex::new(0usize));
        let approval_count_for_closure = approval_count.clone();

        let result = agent
            .run(
                vec![Message {
                    role: Role::User,
                    content: "run checks".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
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

        let agent = AgentCore {
            provider,
            tools,
            approvals,
            max_steps: 10,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
        };

        let approval_count = Arc::new(Mutex::new(0usize));
        let approval_count_for_closure = approval_count.clone();

        let result = agent
            .run(
                vec![Message {
                    role: Role::User,
                    content: "run bash commands".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
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

        let queued = Arc::new(Mutex::new(VecDeque::new()));
        let consumed = Arc::new(Mutex::new(Vec::<Vec<crate::core::QueuedUserMessage>>::new()));
        let enqueue_after_tool_end = crate::core::QueuedUserMessage {
            message: Message {
                role: Role::User,
                content: "queued follow-up".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
            message_index: Some(7),
        };

        let agent = AgentCore {
            provider,
            tools,
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session,
        };

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = |_questions: Vec<QuestionPrompt>| async {
            anyhow::bail!("question tool should not be called in this test")
        };

        let result = agent
            .run_with_runner_output_sink_cancellable(
                vec![Message {
                    role: Role::User,
                    content: "initial prompt".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                &mut approve,
                &mut ask_question,
                &mut || std::future::pending::<()>(),
                &mut |output| {
                    if matches!(output, RunnerOutput::ToolEnd { .. })
                        && let Ok(mut queue) = queued.lock()
                        && queue.is_empty()
                    {
                        queue.push_back(enqueue_after_tool_end.clone());
                    }
                    Ok(())
                },
                &mut || {
                    let drained = {
                        let Ok(mut queue) = queued.lock() else {
                            return Vec::new();
                        };
                        queue.drain(..).collect::<Vec<_>>()
                    };
                    if !drained.is_empty()
                        && let Ok(mut seen) = consumed.lock()
                    {
                        seen.push(drained.clone());
                    }
                    drained.into_iter().map(|queued| queued.message).collect()
                },
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

    #[tokio::test]
    async fn resume_prefers_runner_snapshot_for_todo_state_in_llm_context() {
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
            Some("snapshot resume".to_string()),
        )
        .expect("session store");

        session
            .save_runner_state_snapshot(&RunnerState {
                todo_items: vec![crate::core::TodoItem {
                    content: "from snapshot".to_string(),
                    status: crate::core::TodoStatus::Pending,
                    priority: crate::core::TodoPriority::Medium,
                }],
                context_tokens: 42,
            })
            .expect("save snapshot");

        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![ProviderResponse {
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
                context_tokens: Some(100),
            }]))),
            captured_requests: Arc::clone(&captured_requests),
        };

        let agent = AgentCore {
            provider,
            tools,
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
        };

        let result = agent
            .run(
                vec![Message {
                    role: Role::User,
                    content: "resume".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                |_request| async { Ok(ApprovalChoice::AllowOnce) },
            )
            .await;

        assert!(result.is_ok());

        let requests = captured_requests.lock().expect("captured requests");
        assert_eq!(requests.len(), 1);
        let has_snapshot_message = requests[0].messages.iter().any(|message| {
            message.role == Role::System
                && message
                    .content
                    .contains("Runtime TODO state: use this as the canonical plan snapshot.")
                && message.content.contains("from snapshot")
        });
        assert!(has_snapshot_message);

        let saved = session
            .load_runner_state_snapshot()
            .expect("load snapshot")
            .expect("snapshot present");
        assert_eq!(saved.context_tokens, 100);
        assert_eq!(saved.todo_items.len(), 1);
        assert_eq!(saved.todo_items[0].content, "from snapshot");
    }

    #[tokio::test]
    async fn runner_outputs_persist_tool_call_and_result_events_via_loop_adapter() {
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
            Some("tool event persistence".to_string()),
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
            captured_requests: Arc::new(Mutex::new(Vec::new())),
        };

        let agent = AgentCore {
            provider,
            tools,
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
        };

        let result = agent
            .run(
                vec![Message {
                    role: Role::User,
                    content: "run todo read".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                |_request| async { Ok(ApprovalChoice::AllowOnce) },
            )
            .await;

        assert!(result.is_ok());

        let events = session.replay_events().expect("replay events");
        let tool_call_ids = events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::ToolCall { call } => Some(call.id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_result_ids = events
            .iter()
            .filter_map(|event| match event {
                SessionEvent::ToolResult { id, result, .. } if result.is_some() => Some(id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_call_ids, vec!["call-1".to_string()]);
        assert_eq!(tool_result_ids, vec!["call-1".to_string()]);
    }

    #[tokio::test]
    async fn runner_state_snapshot_round_trips_todo_state_across_runs() {
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
            Some("snapshot roundtrip".to_string()),
        )
        .expect("session store");

        let first_provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: String::new(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: vec![ToolCall {
                            id: "call-1".to_string(),
                            name: "todo_write".to_string(),
                            arguments: json!({
                                "todos": [{
                                    "content": "from first run",
                                    "status": "pending",
                                    "priority": "high"
                                }]
                            }),
                        }],
                    },
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "todo_write".to_string(),
                        arguments: json!({
                            "todos": [{
                                "content": "from first run",
                                "status": "pending",
                                "priority": "high"
                            }]
                        }),
                    }],
                    done: false,
                    thinking: None,
                    context_tokens: Some(21),
                },
                ProviderResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content: "first done".to_string(),
                        attachments: Vec::new(),
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    },
                    tool_calls: Vec::new(),
                    done: true,
                    thinking: None,
                    context_tokens: Some(34),
                },
            ]))),
            captured_requests: Arc::new(Mutex::new(Vec::new())),
        };

        let first_agent = AgentCore {
            provider: first_provider,
            tools: ToolRegistry::new(&settings, &workspace),
            approvals: PermissionMatcher::new(settings.clone(), &tools.schemas(), &workspace),
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
        };

        let first_result = first_agent
            .run(
                vec![Message {
                    role: Role::User,
                    content: "seed todo state".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                |_request| async { Ok(ApprovalChoice::AllowOnce) },
            )
            .await;
        assert!(first_result.is_ok());

        let saved_snapshot = session
            .load_runner_state_snapshot()
            .expect("load snapshot")
            .expect("snapshot present");
        assert_eq!(saved_snapshot.context_tokens, 34);
        assert_eq!(saved_snapshot.todo_items.len(), 1);
        assert_eq!(saved_snapshot.todo_items[0].content, "from first run");

        let second_captured_requests = Arc::new(Mutex::new(Vec::new()));
        let second_provider = TestProvider {
            responses: Arc::new(Mutex::new(VecDeque::from(vec![ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "second done".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                tool_calls: Vec::new(),
                done: true,
                thinking: None,
                context_tokens: Some(55),
            }]))),
            captured_requests: Arc::clone(&second_captured_requests),
        };

        let second_agent = AgentCore {
            provider: second_provider,
            tools: ToolRegistry::new(&settings, &workspace),
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session: session.clone(),
        };

        let second_result = second_agent
            .run(
                vec![Message {
                    role: Role::User,
                    content: "resume and continue".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                |_request| async { Ok(ApprovalChoice::AllowOnce) },
            )
            .await;
        assert!(second_result.is_ok());

        let second_requests = second_captured_requests.lock().expect("captured requests");
        assert_eq!(second_requests.len(), 1);
        let has_snapshot_message = second_requests[0].messages.iter().any(|message| {
            message.role == Role::System
                && message
                    .content
                    .contains("Runtime TODO state: use this as the canonical plan snapshot.")
                && message.content.contains("from first run")
        });
        assert!(has_snapshot_message);
    }

    #[tokio::test]
    async fn cancellation_emits_single_cancelled_event_from_runner_output_path() {
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
            Some("cancelled event".to_string()),
        )
        .expect("session store");

        let cancelled_count = Arc::new(AtomicUsize::new(0));

        let agent = AgentCore {
            provider: HangingProvider,
            tools,
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session,
        };

        let (cancel_tx, cancel_rx) = watch::channel(false);
        tokio::spawn(async move {
            sleep(Duration::from_millis(25)).await;
            let _ = cancel_tx.send(true);
        });

        let mut cancel = {
            let cancel_rx = cancel_rx.clone();
            move || {
                let mut cancel_rx = cancel_rx.clone();
                async move {
                    if *cancel_rx.borrow() {
                        return;
                    }
                    let _ = cancel_rx.changed().await;
                }
            }
        };

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = |_questions: Vec<QuestionPrompt>| async {
            anyhow::bail!("question tool should not be called in this test")
        };

        let result = timeout(
            Duration::from_millis(250),
            agent.run_with_runner_output_sink_cancellable(
                vec![Message {
                    role: Role::User,
                    content: "cancel this run".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                &mut approve,
                &mut ask_question,
                &mut cancel,
                &mut |output| {
                    if matches!(output, RunnerOutput::Cancelled) {
                        cancelled_count.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(())
                },
                &mut Vec::new,
            ),
        )
        .await
        .expect("agent cancellation should resolve quickly");

        let err = result.expect_err("run should be cancelled");
        assert!(is_cancellation_error(&err));
        assert_eq!(cancelled_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bounded_output_channel_keeps_turn_complete_under_delta_burst() {
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
            Some("burst output".to_string()),
        )
        .expect("session store");

        let done_count = Arc::new(AtomicUsize::new(0));

        let agent = AgentCore {
            provider: BurstStreamProvider {
                response: ProviderResponse {
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
                    context_tokens: Some(12),
                },
                bursts: 200,
            },
            tools,
            approvals,
            max_steps: 5,
            model: "test".to_string(),
            system_prompt: String::new(),
            session,
        };

        let mut approve = |_request: ApprovalRequest| async { Ok(ApprovalChoice::AllowOnce) };
        let mut ask_question = |_questions: Vec<QuestionPrompt>| async {
            anyhow::bail!("question tool should not be called in this test")
        };

        let result = agent
            .run_with_runner_output_sink_cancellable(
                vec![Message {
                    role: Role::User,
                    content: "burst".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                }],
                &mut approve,
                &mut ask_question,
                &mut || std::future::pending::<()>(),
                &mut |output| {
                    if matches!(output, RunnerOutput::TurnComplete) {
                        done_count.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(())
                },
                &mut Vec::new,
            )
            .await;

        assert_eq!(result.expect("run result"), "a".repeat(200));
        assert_eq!(done_count.load(Ordering::SeqCst), 1);
    }
}
