use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MessageAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageAttachment {
    Image {
        media_type: String,
        data_base64: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutating: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub assistant_message: Message,
    pub tool_calls: Vec<ToolCall>,
    pub done: bool,
    pub thinking: Option<String>,
    pub context_tokens: Option<usize>,
}

#[derive(Debug, Clone)]
pub enum ProviderStreamEvent {
    AssistantDelta(String),
    ThinkingDelta(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub is_error: bool,
    pub summary: String,
    pub content_type: String,
    pub payload: Value,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub result: ToolResult,
}

impl ToolExecution {
    pub fn from_result(result: ToolResult) -> Self {
        Self { result }
    }
}

impl ToolResult {
    pub fn ok_text(summary: impl Into<String>, text: impl Into<String>) -> Self {
        let output = text.into();
        Self {
            is_error: false,
            summary: summary.into(),
            content_type: "text/plain".to_string(),
            payload: Value::String(output.clone()),
            output,
        }
    }

    pub fn err_text(summary: impl Into<String>, text: impl Into<String>) -> Self {
        let output = text.into();
        Self {
            is_error: true,
            summary: summary.into(),
            content_type: "text/plain".to_string(),
            payload: Value::String(output.clone()),
            output,
        }
    }

    pub fn ok_json(summary: impl Into<String>, payload: Value) -> Self {
        let output = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        Self {
            is_error: false,
            summary: summary.into(),
            content_type: "application/json".to_string(),
            payload,
            output,
        }
    }

    pub fn err_json(summary: impl Into<String>, payload: Value) -> Self {
        let output = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        Self {
            is_error: true,
            summary: summary.into(),
            content_type: "application/json".to_string(),
            payload,
            output,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AgentInput {
    Message(Message),
    ToolResult { call_id: String, result: ToolResult },
    Cancel,
}

#[derive(Debug, Clone)]
pub enum AgentOutput {
    ThinkingDelta(String),
    AssistantDelta(String),
    MessageAdded(Message),
    ToolCallRequested { call: ToolCall, blocking: bool },
    TurnComplete,
    ContextUsage(usize),
    Cancelled,
    Error(String),
}
