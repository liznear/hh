use crate::core::{ApprovalChoice, ApprovalRequest, Message, QuestionPrompt, TodoItem, ToolCall};
use crate::tool::ToolResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed runner-owned state that tools can mutate through `StatePatch` operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerState {
    pub todo_items: Vec<TodoItem>,
    pub context_tokens: usize,
}

impl RunnerState {
    pub fn apply_patch(&mut self, patch: StatePatch) -> bool {
        let mut changed = false;
        for op in patch.ops {
            match op {
                StateOp::SetTodoItems { items } => {
                    if self.todo_items != items {
                        self.todo_items = items;
                        changed = true;
                    }
                }
                StateOp::SetContextTokens { tokens } => {
                    if self.context_tokens != tokens {
                        self.context_tokens = tokens;
                        changed = true;
                    }
                }
            }
        }
        changed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StatePatch {
    pub ops: Vec<StateOp>,
}

impl StatePatch {
    pub fn none() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn with_op(op: StateOp) -> Self {
        Self { ops: vec![op] }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StateOp {
    SetTodoItems { items: Vec<TodoItem> },
    SetContextTokens { tokens: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum CoreInput {
    ToolResult {
        call_id: String,
        name: String,
        result: ToolResult,
    },
    SetEphemeralState(Option<Message>),
    Cancel,
}

#[derive(Debug, Clone)]
pub enum CoreOutput {
    ThinkingDelta(String),
    AssistantDelta(String),
    ContextUsage(usize),
    ToolCallRequested(ToolCall),
    MessageAdded(Message),
    TurnComplete,
    Error(ErrorPayload),
}

#[derive(Debug, Clone)]
pub enum RunnerInput {
    Message(Message),
    Cancel,
}

#[derive(Debug, Clone)]
pub enum RunnerOutput {
    ThinkingDelta(String),
    ThinkingRecorded(String),
    AssistantDelta(String),
    MessageAdded(Message),

    ToolCallRecorded(ToolCall),

    StateUpdated(RunnerState),

    ApprovalRequired {
        call_id: String,
        request: ApprovalRequest,
    },
    ApprovalRecorded {
        tool_name: String,
        approved: bool,
        action: Option<Value>,
        choice: Option<ApprovalChoice>,
    },
    QuestionRequired {
        call_id: String,
        prompts: Vec<QuestionPrompt>,
    },

    ToolStart {
        call_id: String,
        name: String,
        args: Value,
    },
    ToolEnd {
        call_id: String,
        name: String,
        result: ToolResult,
    },

    SnapshotUpdated(RunnerState),
    Cancelled,
    TurnComplete,
    Error(ErrorPayload),
}

// Protocol invariants:
// - Exactly one `CoreInput::ToolResult` must be sent for each requested tool `call_id`.
// - The core will not start the next provider call until all tool results for the current turn
//   are resolved (or the turn is cancelled).
