use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<crate::tool::schema::ToolSchema>,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub assistant_message: Message,
    pub tool_calls: Vec<ToolCall>,
    pub done: bool,
    pub thinking: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ProviderStreamEvent {
    AssistantDelta(String),
    ThinkingDelta(String),
}
