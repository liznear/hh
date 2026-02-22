use crate::provider::Role;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionEvent {
    Message {
        id: String,
        role: Role,
        content: String,
        tool_call_id: Option<String>,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        id: String,
        is_error: bool,
        output: String,
    },
    Approval {
        id: String,
        tool_name: String,
        approved: bool,
    },
}
