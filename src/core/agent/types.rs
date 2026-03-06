use crate::core::{ApprovalChoice, ApprovalRequest, Message, QuestionAnswers, QuestionPrompt, TodoItem, ToolCall};
use crate::tool::ToolResult;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct RunnerState {
    pub todo_items: Vec<TodoItem>,
    pub context_tokens: usize,
}

#[derive(Debug, Clone)]
pub enum CoreInput {
    Message(Message),
    ToolResult { call_id: String, name: String, result: ToolResult },
    SetEphemeralState(Option<Message>),
    Cancel,
}

#[derive(Debug, Clone)]
pub enum CoreOutput {
    ThinkingDelta(String),
    AssistantDelta(String),
    ContextUsage(usize),
    ToolCallRequested(ToolCall),
    TurnComplete,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum RunnerInput {
    Message(Message),
    ApprovalDecision { call_id: String, choice: ApprovalChoice },
    QuestionAnswered { call_id: String, answers: QuestionAnswers },
    Cancel,
}

#[derive(Debug, Clone)]
pub enum RunnerOutput {
    ThinkingDelta(String),
    AssistantDelta(String),
    MessageAdded(Message),
    
    StateUpdated(RunnerState),
    
    ApprovalRequired(ApprovalRequest),
    QuestionRequired { call_id: String, prompts: Vec<QuestionPrompt> },
    
    ToolStart { call_id: String, name: String, args: Value },
    ToolEnd { call_id: String, name: String, result: ToolResult },
    
    TurnComplete,
    Error(String),
}
