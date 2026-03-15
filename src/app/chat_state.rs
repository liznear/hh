use std::time::Instant;

use tokio::sync::{oneshot, watch};

use crate::core::MessageAttachment;

pub struct RunningAgentTask {
    pub handle: tokio::task::JoinHandle<()>,
    pub cancel_tx: watch::Sender<bool>,
}

pub type QuestionResponder = std::sync::Arc<
    std::sync::Mutex<Option<oneshot::Sender<anyhow::Result<crate::core::QuestionAnswers>>>>,
>;

#[derive(Debug, Clone, Copy)]
pub struct ScrollState {
    pub offset: usize,
    pub auto_follow: bool,
}

impl ScrollState {
    pub const fn new(auto_follow: bool) -> Self {
        Self {
            offset: 0,
            auto_follow,
        }
    }

    pub fn effective_offset(&self, total_lines: usize, visible_height: usize) -> usize {
        let max_offset = total_lines.saturating_sub(visible_height);
        if self.auto_follow {
            max_offset
        } else {
            self.offset.min(max_offset)
        }
    }

    pub fn scroll_up_steps(&mut self, total_lines: usize, visible_height: usize, steps: usize) {
        if steps == 0 {
            return;
        }

        if self.auto_follow {
            self.offset = total_lines.saturating_sub(visible_height);
            self.auto_follow = false;
        }

        self.offset = self.offset.saturating_sub(steps);
        self.auto_follow = false;
    }

    pub fn scroll_down_steps(&mut self, total_lines: usize, visible_height: usize, steps: usize) {
        if steps == 0 {
            return;
        }

        let max_offset = total_lines.saturating_sub(visible_height);
        self.offset = self.effective_offset(total_lines, visible_height);
        self.offset = self.offset.saturating_add(steps).min(max_offset);
        self.auto_follow = self.offset >= max_offset;
    }

    pub fn reset(&mut self, auto_follow: bool) {
        self.offset = 0;
        self.auto_follow = auto_follow;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItemView {
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatusView {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentItemView {
    pub task_id: String,
    pub session_id: String,
    pub name: String,
    pub parent_task_id: Option<String>,
    pub agent_name: String,
    pub prompt: String,
    pub summary: Option<String>,
    pub depth: usize,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: SubagentStatusView,
}

#[derive(Debug, Clone)]
pub struct SubagentSessionView {
    pub task_id: String,
    pub session_id: String,
    pub title: String,
    pub previous_messages: Vec<ChatMessage>,
    pub previous_scroll: ScrollState,
}

#[derive(Debug, Clone)]
pub struct TaskSessionTarget {
    pub task_id: String,
    pub session_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        text: String,
        queued: bool,
    },
    Assistant(String),
    CompactionPending,
    Compaction(String),
    Thinking(String),
    ToolCall {
        name: String,
        args: String,
        output: Option<String>,
        is_error: Option<bool>,
    },
    Error(String),
    Footer {
        agent_display_name: String,
        provider_name: String,
        model_name: String,
        duration: String,
        interrupted: bool,
    },
}

#[derive(Debug, Clone)]
pub struct PendingQuestionView {
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOptionView>,
    pub selected_index: usize,
    pub custom_mode: bool,
    pub custom_value: String,
    pub question_index: usize,
    pub total_questions: usize,
    pub multiple: bool,
}

#[derive(Debug, Clone)]
pub struct QuestionOptionView {
    pub label: String,
    pub description: String,
    pub selected: bool,
    pub active: bool,
    pub custom: bool,
    pub submit: bool,
}

#[derive(Debug)]
pub struct PendingQuestionState {
    pub questions: Vec<crate::core::QuestionPrompt>,
    pub answers: crate::core::QuestionAnswers,
    pub custom_values: Vec<String>,
    pub question_index: usize,
    pub selected_index: usize,
    pub custom_mode: bool,
    pub responder: Option<QuestionResponder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKeyResult {
    NotHandled,
    Handled,
    Submitted,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ClipboardNotice {
    pub x: u16,
    pub y: u16,
    pub expires_at: Instant,
}

impl SelectionPosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSelection {
    None,
    InProgress {
        start: SelectionPosition,
    },
    Active {
        start: SelectionPosition,
        end: SelectionPosition,
    },
}

impl TextSelection {
    pub fn is_none(&self) -> bool {
        matches!(self, TextSelection::None)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, TextSelection::Active { .. })
    }

    pub fn get_range(&self) -> Option<(SelectionPosition, SelectionPosition)> {
        match self {
            TextSelection::Active { start, end } => {
                let (start_pos, end_pos) = if start.line < end.line
                    || (start.line == end.line && start.column <= end.column)
                {
                    (*start, *end)
                } else {
                    (*end, *start)
                };
                Some((start_pos, end_pos))
            }
            _ => None,
        }
    }

    pub fn get_active_start(&self) -> Option<SelectionPosition> {
        match self {
            TextSelection::Active { start, .. } | TextSelection::InProgress { start } => {
                Some(*start)
            }
            TextSelection::None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOptionView {
    pub name: String,
    pub display_name: String,
    pub color: Option<String>,
    pub mode: String,
}

pub struct SubmittedInput {
    pub text: String,
    pub attachments: Vec<MessageAttachment>,
    pub message_index: Option<usize>,
    pub queued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOptionView {
    pub full_id: String,
    pub provider_name: String,
    pub model_name: String,
    pub modality: String,
    pub max_context_size: usize,
}

impl TodoStatus {
    pub fn from_core(status: crate::core::TodoStatus) -> Self {
        match status {
            crate::core::TodoStatus::Pending => Self::Pending,
            crate::core::TodoStatus::InProgress => Self::InProgress,
            crate::core::TodoStatus::Completed => Self::Completed,
            crate::core::TodoStatus::Cancelled => Self::Cancelled,
        }
    }

    pub fn from_wire(status: &str) -> Option<Self> {
        match status {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl TodoPriority {
    pub fn from_core(priority: crate::core::TodoPriority) -> Self {
        match priority {
            crate::core::TodoPriority::High => Self::High,
            crate::core::TodoPriority::Medium => Self::Medium,
            crate::core::TodoPriority::Low => Self::Low,
        }
    }

    pub fn from_wire(priority: &str) -> Option<Self> {
        match priority {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

impl SubagentStatusView {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    pub fn from_wire(status: &str) -> Option<Self> {
        match status {
            "pending" | "queued" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" | "done" => Some(Self::Completed),
            "failed" | "error" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn from_lifecycle(status: crate::session::types::SubAgentLifecycleStatus) -> Self {
        match status {
            crate::session::types::SubAgentLifecycleStatus::Pending => Self::Pending,
            crate::session::types::SubAgentLifecycleStatus::Running => Self::Running,
            crate::session::types::SubAgentLifecycleStatus::Completed => Self::Completed,
            crate::session::types::SubAgentLifecycleStatus::Failed => Self::Failed,
            crate::session::types::SubAgentLifecycleStatus::Cancelled => Self::Cancelled,
        }
    }
}
