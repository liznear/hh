pub mod output_channel;
pub mod runner;
pub mod subagent_manager;
pub mod types;

pub use types::{ErrorPayload, RunnerInput, RunnerOutput, RunnerState, StateOp, StatePatch};

use self::runner::AgentRunner;
use crate::core::{ApprovalPolicy, Message, Provider, SessionReader, SessionSink, ToolExecutor};
use crate::session::{SessionEvent, event_id};

type EmitErrorSlot = std::sync::Arc<std::sync::Mutex<Option<String>>>;

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
    pub async fn run<O, D>(
        &self,
        input_rx: tokio::sync::mpsc::Receiver<RunnerInput>,
        emit_output: &mut O,
        drain_pending_messages: &mut D,
    ) -> anyhow::Result<String>
    where
        O: FnMut(RunnerOutput) -> anyhow::Result<()> + Send,
        D: FnMut() -> Vec<Message>,
    {
        let replayed_events = self.session.replay_events()?;
        let loaded_snapshot = self.session.load_runner_state_snapshot()?;
        let runner_snapshot = loaded_snapshot.clone().unwrap_or_default();
        let mut messages = self.session.replay_messages()?;
        let mut runner = AgentRunner::new(
            &self.tools,
            &self.approvals,
            RunnerState {
                todo_items: runner_snapshot.todo_items.clone(),
                context_tokens: runner_snapshot.context_tokens,
            },
        );
        runner
            .hydrate_state_from_replayed_tool_results(&replayed_events, loaded_snapshot.is_some());

        let config = hh_agent::AgentConfig {
            model: self.model.clone(),
            system_prompt: self.system_prompt.clone(),
            max_steps: self.max_steps,
        };

        let emit_error = EmitErrorSlot::default();
        let emit_error_for_emit = emit_error.clone();
        let error_notify = std::sync::Arc::new(tokio::sync::Notify::new());
        let error_notify_for_emit = error_notify.clone();
        let mut emit_wrapped = move |output: RunnerOutput| {
            if let Err(err) = emit_output(output)
                && let Ok(mut slot) = emit_error_for_emit.lock()
                && slot.is_none()
            {
                *slot = Some(err.to_string());
                error_notify_for_emit.notify_one();
            }
        };

        let run_future = runner.run_input_loop(
            &self.provider,
            config,
            &mut messages,
            input_rx,
            &mut emit_wrapped,
            drain_pending_messages,
        );
        tokio::pin!(run_future);

        let run_result = tokio::select! {
            biased;
            _ = error_notify.notified() => {
                Err(anyhow::anyhow!(take_emit_error(&emit_error).unwrap_or_else(|| "Unknown emit error".to_string())))
            }
            result = &mut run_future => result,
        };

        if let Some(err) = take_emit_error(&emit_error) {
            return Err(anyhow::anyhow!(err));
        }

        match run_result {
            Ok(Some(final_answer)) => Ok(final_answer),
            Ok(None) => anyhow::bail!("runner input loop ended without final answer"),
            Err(err) => Err(err),
        }
    }
}

fn take_emit_error(emit_error: &EmitErrorSlot) -> Option<String> {
    let Ok(mut slot) = emit_error.lock() else {
        return Some("runner emit error slot poisoned".to_string());
    };
    slot.take()
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
