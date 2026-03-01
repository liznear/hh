pub mod agent;
pub mod system_prompt;
pub mod traits;
pub mod types;

pub use agent::AgentLoop;
pub use traits::{
    AgentEvents, ApprovalChoice, ApprovalDecision, ApprovalPolicy, ApprovalRequest, NoopEvents,
    Provider, SessionReader, SessionSink, ToolExecutor,
};
pub use types::{
    Message, MessageAttachment, ProviderRequest, ProviderResponse, ProviderStreamEvent,
    QuestionAnswer, QuestionAnswers, QuestionOption, QuestionPrompt, Role, SubAgentCall,
    SubAgentResult, TodoItem, TodoPriority, TodoStatus, ToolCall,
};
