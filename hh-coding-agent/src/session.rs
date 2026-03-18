//! Session event types for the agent runtime.
//!
//! These types describe the events that the agent runtime produces.
//! The `SessionSink` and `SessionReader` traits allow custom persistence implementations.

use crate::core::traits::ApprovalChoice;
use crate::core::types::{Message, ToolCall};
use crate::tool::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_subagent_status_running() -> SubAgentLifecycleStatus {
    SubAgentLifecycleStatus::Running
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentLifecycleStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentFailureReason {
    ToolError,
    ApprovalDenied,
    RuntimeError,
    InterruptedByRestart,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionEvent {
    Message {
        id: String,
        #[serde(flatten)]
        message: Message,
    },
    ToolCall {
        #[serde(flatten)]
        call: ToolCall,
    },
    ToolResult {
        id: String,
        is_error: bool,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<ToolResult>,
    },
    Approval {
        id: String,
        tool_name: String,
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        choice: Option<ApprovalChoice>,
    },
    Thinking {
        id: String,
        content: String,
    },
    Compact {
        id: String,
        summary: String,
    },
    SubAgentStart {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default = "default_subagent_status_running")]
        status: SubAgentLifecycleStatus,
        #[serde(default)]
        created_at: u64,
        #[serde(default)]
        updated_at: u64,
        prompt: String,
        depth: usize,
    },
    SubAgentProgress {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(default)]
        seq: u64,
        content: String,
    },
    SubAgentResult {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(default = "default_subagent_status_running")]
        status: SubAgentLifecycleStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure_reason: Option<SubAgentFailureReason>,
        is_error: bool,
        output: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub last_updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub is_child_session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_state_snapshot: Option<crate::core::agent::RunnerState>,
}

/// Generate a unique event ID.
pub fn event_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Create a user message event.
pub fn user_message(content: impl Into<String>) -> SessionEvent {
    SessionEvent::Message {
        id: event_id(),
        message: Message {
            role: crate::core::Role::User,
            content: content.into(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        },
    }
}
