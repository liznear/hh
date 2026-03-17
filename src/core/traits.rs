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

pub trait SessionSink: Send + Sync {
    fn append(&self, event: &crate::session::SessionEvent) -> anyhow::Result<()>;

    fn save_runner_state_snapshot(
        &self,
        _snapshot: &crate::core::agent::RunnerState,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub trait SessionReader: Send + Sync {
    fn replay_messages(&self) -> anyhow::Result<Vec<Message>>;
    fn replay_events(&self) -> anyhow::Result<Vec<crate::session::SessionEvent>>;

    fn load_runner_state_snapshot(
        &self,
    ) -> anyhow::Result<Option<crate::core::agent::RunnerState>> {
        Ok(None)
    }
}
