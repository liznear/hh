use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::core::agent::AgentEvents;

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
    TodoItemsChanged(Vec<crate::core::TodoItem>),
    AssistantDelta(String),
    ContextUsage(usize),
    AssistantDone,
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
}

impl AgentEvents for TuiEventSender {
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

    fn on_todo_items_changed(&self, items: &[crate::core::TodoItem]) {
        self.send(TuiEvent::TodoItemsChanged(items.to_vec()));
    }

    fn on_assistant_delta(&self, delta: &str) {
        self.send(TuiEvent::AssistantDelta(delta.to_string()));
    }

    fn on_context_usage(&self, tokens: usize) {
        self.send(TuiEvent::ContextUsage(tokens));
    }

    fn on_assistant_done(&self) {
        self.send(TuiEvent::AssistantDone);
    }

    fn drain_queued_user_messages(&self) -> Vec<crate::core::QueuedUserMessage> {
        let Ok(mut queued) = self.queued_user_messages.lock() else {
            return Vec::new();
        };
        queued.drain(..).collect()
    }

    fn on_queued_user_messages_consumed(&self, messages: &[crate::core::QueuedUserMessage]) {
        let consumed_indexes = messages
            .iter()
            .filter_map(|message| message.message_index)
            .collect::<Vec<_>>();
        if !consumed_indexes.is_empty() {
            self.send(TuiEvent::QueuedMessagesConsumed(consumed_indexes));
        }
    }
}
