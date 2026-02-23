use crate::core::{Message, ToolCall};
use serde::{Deserialize, Serialize};

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
    },
    Approval {
        id: String,
        tool_name: String,
        approved: bool,
    },
    Thinking {
        id: String,
        content: String,
    },
}
