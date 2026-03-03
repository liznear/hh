use std::sync::Arc;
use std::sync::Mutex;

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
    session_epoch: u64,
    run_epoch: u64,
}

impl TuiEventSender {
    pub fn new(tx: mpsc::UnboundedSender<ScopedTuiEvent>) -> Self {
        Self {
            tx: Arc::new(tx),
            session_epoch: 0,
            run_epoch: 0,
        }
    }

    pub fn scoped(&self, session_epoch: u64, run_epoch: u64) -> Self {
        Self {
            tx: Arc::clone(&self.tx),
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
}
