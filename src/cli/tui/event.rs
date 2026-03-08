use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::core::RunnerOutputObserver;

#[derive(Debug, Clone)]
pub struct SubagentEventItem {
    pub task_id: String,
    pub session_id: String,
    pub name: String,
    pub agent_name: String,
    pub status: String,
    pub prompt: String,
    pub depth: usize,
    pub parent_task_id: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub summary: Option<String>,
    pub error: Option<String>,
}

type QuestionResponder =
    Arc<Mutex<Option<oneshot::Sender<anyhow::Result<crate::core::QuestionAnswers>>>>>;

#[derive(Debug)]
pub struct ScopedTuiEvent {
    pub session_epoch: u64,
    pub run_epoch: u64,
    pub event: TuiEvent,
}

#[derive(Debug)]
pub enum TuiEvent {
    Thinking(String),
    ToolStart {
        name: String,
        args: Value,
    },
    ToolEnd {
        name: String,
        result: crate::tool::ToolResult,
    },
    AssistantDelta(String),
    RunnerStateUpdated(crate::core::agent::RunnerState),
    AssistantDone,
    Cancelled,
    ApprovalRequired {
        call_id: String,
        request: crate::core::ApprovalRequest,
    },
    QuestionRequired {
        call_id: String,
        prompts: Vec<crate::core::QuestionPrompt>,
    },
    QueuedMessagesConsumed(Vec<usize>),
    SessionTitle(String),
    CompactionStart,
    CompactionDone(String),
    QuestionPrompt {
        questions: Vec<crate::core::QuestionPrompt>,
        responder: QuestionResponder,
    },
    SubagentsChanged(Vec<SubagentEventItem>),
    Error(String),
    Key(crossterm::event::KeyEvent),
    Tick,
}

/// Sends agent events to a channel for the TUI to consume
#[derive(Clone)]
pub struct TuiEventSender {
    tx: Arc<mpsc::UnboundedSender<ScopedTuiEvent>>,
    queued_user_messages: Arc<Mutex<VecDeque<crate::core::QueuedUserMessage>>>,
    session_epoch: u64,
    run_epoch: u64,
}

impl TuiEventSender {
    pub fn new(tx: mpsc::UnboundedSender<ScopedTuiEvent>) -> Self {
        Self {
            tx: Arc::new(tx),
            queued_user_messages: Arc::new(Mutex::new(VecDeque::new())),
            session_epoch: 0,
            run_epoch: 0,
        }
    }

    pub fn scoped(&self, session_epoch: u64, run_epoch: u64) -> Self {
        Self {
            tx: Arc::clone(&self.tx),
            queued_user_messages: Arc::clone(&self.queued_user_messages),
            session_epoch,
            run_epoch,
        }
    }

    pub fn send(&self, event: TuiEvent) {
        let _ = self.tx.send(ScopedTuiEvent {
            session_epoch: self.session_epoch,
            run_epoch: self.run_epoch,
            event,
        });
    }

    pub fn enqueue_queued_user_message(&self, message: crate::core::QueuedUserMessage) {
        if let Ok(mut queued) = self.queued_user_messages.lock() {
            queued.push_back(message);
        }
    }

    pub fn drain_queued_user_messages(&self) -> Vec<crate::core::QueuedUserMessage> {
        let Ok(mut queued) = self.queued_user_messages.lock() else {
            return Vec::new();
        };
        queued.drain(..).collect()
    }

    pub fn on_queued_user_messages_consumed(&self, messages: &[crate::core::QueuedUserMessage]) {
        let consumed_indexes = messages
            .iter()
            .filter_map(|message| message.message_index)
            .collect::<Vec<_>>();
        if !consumed_indexes.is_empty() {
            self.send(TuiEvent::QueuedMessagesConsumed(consumed_indexes));
        }
    }
}

impl RunnerOutputObserver for TuiEventSender {
    fn on_thinking(&self, text: &str) {
        self.send(TuiEvent::Thinking(text.to_string()));
    }

    fn on_tool_start(&self, name: &str, args: &Value) {
        self.send(TuiEvent::ToolStart {
            name: name.to_string(),
            args: args.clone(),
        });
    }

    fn on_tool_end(&self, name: &str, result: &crate::tool::ToolResult) {
        self.send(TuiEvent::ToolEnd {
            name: name.to_string(),
            result: result.clone(),
        });
    }

    fn on_assistant_delta(&self, delta: &str) {
        self.send(TuiEvent::AssistantDelta(delta.to_string()));
    }

    fn on_runner_state_updated(&self, state: &crate::core::agent::RunnerState) {
        self.send(TuiEvent::RunnerStateUpdated(state.clone()));
    }

    fn on_assistant_done(&self) {
        self.send(TuiEvent::AssistantDone);
    }

    fn on_error(&self, message: &str) {
        self.send(TuiEvent::Error(message.to_string()));
    }

    fn on_approval_required(&self, call_id: &str, request: &crate::core::ApprovalRequest) {
        self.send(TuiEvent::ApprovalRequired {
            call_id: call_id.to_string(),
            request: request.clone(),
        });
    }

    fn on_question_required(&self, call_id: &str, prompts: &[crate::core::QuestionPrompt]) {
        self.send(TuiEvent::QuestionRequired {
            call_id: call_id.to_string(),
            prompts: prompts.to_vec(),
        });
    }

    fn on_cancelled(&self) {
        self.send(TuiEvent::Cancelled);
    }
}
