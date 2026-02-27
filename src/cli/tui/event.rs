use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::core::agent::AgentEvents;

#[derive(Debug, Clone)]
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
    ContextUsage(usize),
    AssistantDone,
    SessionTitle(String),
    CompactionStart,
    CompactionDone(String),
    Error(String),
    Key(crossterm::event::KeyEvent),
    Tick,
}

/// Sends agent events to a channel for the TUI to consume
#[derive(Clone)]
pub struct TuiEventSender {
    tx: Arc<mpsc::UnboundedSender<TuiEvent>>,
}

impl TuiEventSender {
    pub fn new(tx: mpsc::UnboundedSender<TuiEvent>) -> Self {
        Self { tx: Arc::new(tx) }
    }

    pub fn send(&self, event: TuiEvent) {
        let _ = self.tx.send(event);
    }
}

impl AgentEvents for TuiEventSender {
    fn on_thinking(&self, text: &str) {
        let _ = self.tx.send(TuiEvent::Thinking(text.to_string()));
    }

    fn on_tool_start(&self, name: &str, args: &Value) {
        let _ = self.tx.send(TuiEvent::ToolStart {
            name: name.to_string(),
            args: args.clone(),
        });
    }

    fn on_tool_end(&self, name: &str, result: &crate::tool::ToolResult) {
        let _ = self.tx.send(TuiEvent::ToolEnd {
            name: name.to_string(),
            result: result.clone(),
        });
    }

    fn on_assistant_delta(&self, delta: &str) {
        let _ = self.tx.send(TuiEvent::AssistantDelta(delta.to_string()));
    }

    fn on_context_usage(&self, tokens: usize) {
        let _ = self.tx.send(TuiEvent::ContextUsage(tokens));
    }

    fn on_assistant_done(&self) {
        let _ = self.tx.send(TuiEvent::AssistantDone);
    }
}
