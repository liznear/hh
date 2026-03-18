//! Core coding agent functionality for LLM-based coding assistants.
//!
//! This crate provides the core agent loop, tool execution, approval workflow,
//! and event channels for building coding agents. It is designed to be used
//! by higher-level crates that provide UI and persistence.

pub mod config;
pub mod core;
pub mod permission;
pub mod safety;
pub mod session;
pub mod tool;

// Re-exports for convenience
pub use core::{
    AgentCore, RunnerOutputObserver,
    agent::{RunnerInput, RunnerOutput, RunnerState, StatePatch},
    traits::{
        ApprovalChoice, ApprovalDecision, ApprovalPolicy, ApprovalRequest, Provider,
        QueuedUserMessage, SessionReader, SessionSink, ToolExecutor,
    },
    types::{
        Message, MessageAttachment, ProviderRequest, ProviderResponse, ProviderStreamEvent,
        QuestionAnswer, QuestionAnswers, QuestionOption, QuestionPrompt, Role, SubAgentCall,
        SubAgentResult, TodoItem, TodoPriority, TodoStatus, ToolCall, ToolSchema,
    },
};
pub use permission::{Decision as PermissionDecision, PermissionMatcher};
pub use safety::sanitize_tool_output;
pub use session::{SessionEvent, SessionMetadata, event_id, user_message};
pub use tool::{Tool, ToolExecution, ToolRegistry, ToolResult, ToolSchema as ToolSchemaType};
