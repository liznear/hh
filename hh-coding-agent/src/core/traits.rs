use crate::core::types::Message;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Re-export Provider trait from hh-agent crate.
pub use hh_agent::Provider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalChoice {
    AllowOnce,
    AllowSession,
    AllowAlways,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub title: String,
    pub body: String,
    pub action: Value,
}

#[derive(Debug, Clone)]
pub struct QueuedUserMessage {
    pub message: Message,
    pub message_index: Option<usize>,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn schemas(&self) -> Vec<crate::tool::schema::ToolSchema>;
    async fn execute(&self, name: &str, args: Value) -> crate::tool::ToolExecution;
    fn apply_approval_decision(
        &self,
        _action: &Value,
        _choice: ApprovalChoice,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
    fn is_non_blocking(&self, _name: &str) -> bool {
        false
    }
}

pub trait ApprovalPolicy: Send + Sync {
    fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> ApprovalDecision;
}

/// Trait for persisting session events.
///
/// Implementations can choose their own persistence strategy (file-based, database, in-memory, etc.)
pub trait SessionSink: Send + Sync {
    /// Append an event to the session log.
    /// The event is a serializable value that the implementation can persist.
    fn append(&self, event: &crate::session::SessionEvent) -> anyhow::Result<()>;

    /// Save a snapshot of the runner state.
    fn save_runner_state_snapshot(
        &self,
        _snapshot: &crate::core::agent::RunnerState,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Trait for reading session data.
///
/// Implementations can choose their own storage strategy.
pub trait SessionReader: Send + Sync {
    /// Replay all messages from the session.
    fn replay_messages(&self) -> anyhow::Result<Vec<Message>>;

    /// Replay all events from the session.
    fn replay_events(&self) -> anyhow::Result<Vec<crate::session::SessionEvent>>;

    /// Load a previously saved runner state snapshot.
    fn load_runner_state_snapshot(
        &self,
    ) -> anyhow::Result<Option<crate::core::agent::RunnerState>> {
        Ok(None)
    }
}

impl<S: SessionSink + Send + Sync + ?Sized> SessionSink for &S {
    fn append(&self, event: &crate::session::SessionEvent) -> anyhow::Result<()> {
        (**self).append(event)
    }

    fn save_runner_state_snapshot(
        &self,
        snapshot: &crate::core::agent::RunnerState,
    ) -> anyhow::Result<()> {
        (**self).save_runner_state_snapshot(snapshot)
    }
}
