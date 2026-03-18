# hh-coding-agent

A coding agent runtime crate that orchestrates tool execution, manages approvals, and handles session events. This crate builds on `hh-agent` to provide a complete coding agent with tool implementations, permission policies, and output sanitization.

## What Is This Crate?

`hh-coding-agent` implements the coding agent runtime that:

- Executes tools (bash, file operations, edits, web fetch, etc.)
- Manages approval workflows for sensitive operations
- Orchestrates sub-agent tasks
- Emits session events for persistence
- Sanitizes tool outputs for safety

**This crate does NOT:**
- Persist sessions to disk (provides `SessionSink` trait instead)
- Implement LLM providers (uses `hh-agent`'s `Provider` trait)
- Provide a UI or CLI (pure runtime only)
- Load configuration from files (accepts `Settings` struct)

## Responsibilities

The crate provides a complete coding agent runtime with tools and safety.

### In Scope
- Tool execution via `ToolExecutor` trait implementation
- Tool registry with all standard coding tools (bash, read, write, edit, etc.)
- Approval policy via `ApprovalPolicy` trait
- Permission matching and rule evaluation
- Sub-agent task orchestration via `SubagentManager`
- Session event types (`SessionEvent`, `SessionMetadata`)
- Output sanitization (`sanitize_tool_output`)
- Agent runner that integrates `hh-agent` loop with tool execution
- System prompts for coding assistant behavior

### Out of Scope
- Session persistence implementations (file-based storage)
- LLM provider implementations (OpenAI, Anthropic, etc.)
- TUI/CLI rendering
- Configuration file loading
- Agent registry and configuration management

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     hh-coding-agent                              │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │   Runner    │  │    Tools     │  │     Permissions      │   │
│  │             │  │              │  │                      │   │
│  │ AgentRunner │  │ BashTool     │  │ PermissionMatcher    │   │
│  │ RunnerState │  │ ReadTool     │  │ PermissionPolicy     │   │
│  │ RunnerInput │  │ WriteTool    │  │ ApprovalDecision     │   │
│  │ RunnerOutput│  │ EditTool     │  │                      │   │
│  │             │  │ TaskTool     │  │                      │   │
│  │             │  │ ...          │  │                      │   │
│  └──────┬──────┘  └──────┬───────┘  └──────────┬───────────┘   │
│         │                │                      │               │
│         └────────────────┼──────────────────────┘               │
│                          │                                      │
│                          ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Session Events                         │  │
│  │                                                           │  │
│  │  SessionEvent (ToolCall, ToolResult, SubAgent*, etc.)    │  │
│  │  SessionMetadata                                          │  │
│  └──────────────────────────────────────────────────────────┘  │
│                          │                                      │
│                          ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                     Traits                                │  │
│  │                                                           │  │
│  │  SessionSink      - persist events                        │  │
│  │  SessionReader    - replay events                         │  │
│  │  ToolExecutor     - execute tools                         │  │
│  │  ApprovalPolicy   - decide approval for tool calls        │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                       hh-agent                                  │
│                                                                 │
│  AgentLoop, Provider, ToolRegistry, Message, ToolCall, etc.    │
└─────────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. AgentRunner

The main orchestrator that integrates `hh-agent`'s loop with tool execution:

```rust
pub struct AgentRunner<'a, T, A>
where
    T: ToolExecutor,
{
    pub tools: &'a T,
    pub approvals: &'a A,
    pub state: RunnerState,
    pub session_allowed_actions: HashSet<String>,
    pub session_allowed_bash_rules: HashSet<String>,
}
```

The runner:
- Receives `RunnerInput` (messages, tool results, cancellation)
- Emits `RunnerOutput` (deltas, tool calls, state patches)
- Manages `RunnerState` (messages, TODOs, pending tool calls)
- Integrates with `hh-agent::AgentLoop` for LLM communication

### 2. Tools

All standard coding tools are implemented:

| Tool | Description | Blocking |
|------|-------------|----------|
| `bash` | Execute shell commands | Yes |
| `read` | Read file contents | Yes |
| `write` | Write/create files | Yes |
| `edit` | Edit files with exact matching | Yes |
| `diff` | Show unified diff | Yes |
| `todo` | Manage TODO list | No |
| `task` | Spawn sub-agent tasks | Yes |
| `question` | Ask user questions | Yes |
| `web_fetch` | Fetch web content | Yes |
| `skill` | Load/install skills | No |

Tools implement the `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: Value) -> ToolResult;
}
```

### 3. ToolRegistry

Manages tool registration and execution:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    non_blocking: HashSet<String>,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, tool: Box<dyn Tool>);
    pub fn schemas(&self) -> Vec<ToolSchema>;
    pub async fn execute(&self, name: &str, args: Value) -> ToolExecution;
}
```

### 4. Permissions

Permission system for approving sensitive operations:

```rust
pub trait ApprovalPolicy: Send + Sync {
    fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> ApprovalDecision;
}

pub enum ApprovalDecision {
    Allow,  // Auto-approve
    Ask,    // Request user approval
    Deny,   // Auto-deny
}
```

Permission rules support wildcards and patterns:

```rust
pub struct PermissionMatcher {
    rules: Vec<PermissionRule>,
}

// Match patterns like:
// - "fs/read"           → exact match
// - "fs/*"              → prefix match
// - "bash:rm -rf *"     → action pattern
```

### 5. Session Events

Types for recording agent activity:

```rust
pub enum SessionEvent {
    // Messages
    MessageAdded { message: Message },
    
    // Tool lifecycle
    ToolCall { call: ToolCall },
    ToolResult { id: String, result: Option<ToolResult> },
    
    // Sub-agent lifecycle
    SubAgentStart { task_id, name, status, ... },
    SubAgentProgress { task_id, content, ... },
    SubAgentResult { task_id, status, summary, ... },
    
    // State snapshots
    StatePatch { todos, pending_tool_calls, ... },
}

pub struct SessionMetadata {
    pub session_id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub message_count: usize,
}
```

### 6. SessionSink / SessionReader

Traits for persistence integration:

```rust
pub trait SessionSink: Send + Sync {
    fn append(&self, event: &SessionEvent) -> anyhow::Result<()>;
    fn save_runner_state_snapshot(&self, snapshot: &RunnerState) -> anyhow::Result<()>;
}

pub trait SessionReader: Send + Sync {
    fn replay_messages(&self) -> anyhow::Result<Vec<Message>>;
    fn replay_events(&self) -> anyhow::Result<Vec<SessionEvent>>;
    fn load_runner_state_snapshot(&self) -> anyhow::Result<Option<RunnerState>>;
}
```

### 7. SubagentManager

Orchestrates sub-agent tasks:

```rust
pub struct SubagentManager {
    inner: Arc<Mutex<SubagentManagerState>>,
    queue: Arc<Semaphore>,
    max_depth: usize,
    executor: SubagentExecutor,
}

impl SubagentManager {
    pub async fn start_or_resume(
        &self,
        request: SubagentRequest,
        parent_session: Arc<dyn SessionSink + Send + Sync>,
    ) -> anyhow::Result<SubagentAcceptance>;
    
    pub async fn wait_for_terminal(&self, parent_session_id: &str, task_id: &str) 
        -> anyhow::Result<SubagentNode>;
}
```

### 8. Output Sanitization

Sanitizes tool outputs for safety:

```rust
pub fn sanitize_tool_output(output: &str) -> String {
    // Removes potentially sensitive information:
    // - File paths containing usernames
    // - Environment variables
    // - API keys and tokens
}
```

## Input/Output Types

### RunnerInput

```rust
pub enum RunnerInput {
    /// User message to add to conversation
    UserMessage(Message),
    
    /// Tool result to resolve pending call
    ToolResult { call_id: String, result: ToolResult },
    
    /// User approval decision
    Approval { request_id: String, choice: ApprovalChoice },
    
    /// Queue a user message for next turn
    QueuedUserMessage(QueuedUserMessage),
    
    /// Cancel the current run
    Cancel,
}
```

### RunnerOutput

```rust
pub enum RunnerOutput {
    // Streaming content
    ThinkingDelta(String),
    AssistantDelta(String),
    
    // Tool calls
    ToolCallRequested { call: ToolCall, blocking: bool },
    
    // Approval requests
    ApprovalRequest(ApprovalRequest),
    
    // State updates
    StatePatch(StatePatch),
    
    // Lifecycle events
    TurnComplete,
    Cancelled,
    Error(ErrorPayload),
}
```

### RunnerState

```rust
pub struct RunnerState {
    pub messages: Vec<Message>,
    pub todos: Vec<TodoItem>,
    pub pending_tool_calls: HashSet<String>,
    pub question_prompt: Option<QuestionPrompt>,
    pub step: usize,
}
```

## Integration Guide

### Basic Setup

1. **Create tools and registry:**

```rust
use hh_coding_agent::tool::ToolRegistry;

let mut registry = ToolRegistry::new();
registry.register(Box::new(BashTool::new()));
registry.register(Box::new(ReadTool::new()));
registry.register(Box::new(WriteTool::new()));
// ... more tools
```

2. **Implement approval policy:**

```rust
use hh_coding_agent::{ApprovalPolicy, ApprovalDecision};

struct MyApprovalPolicy;

impl ApprovalPolicy for MyApprovalPolicy {
    fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> ApprovalDecision {
        match tool_name {
            "bash" => ApprovalDecision::Ask,  // Always ask for bash
            "read" => ApprovalDecision::Allow, // Auto-allow reads
            _ => ApprovalDecision::Ask,
        }
    }
}
```

3. **Create session sink:**

```rust
use hh_coding_agent::{SessionSink, SessionEvent};

struct MySessionSink;

impl SessionSink for MySessionSink {
    fn append(&self, event: &SessionEvent) -> anyhow::Result<()> {
        // Persist event to your storage
        Ok(())
    }
}
```

4. **Run the agent:**

```rust
let runner = AgentRunner::new(&registry, &policy, RunnerState::default());

// Use input/output channels
let (input_tx, input_rx) = mpsc::channel(256);
let (output_tx, output_rx) = mpsc::channel(256);

// Run the agent loop
runner.run(input_rx, output_tx, session_sink).await?;
```

### Handling Outputs

```rust
while let Some(output) = output_rx.recv().await {
    match output {
        RunnerOutput::ThinkingDelta(text) => {
            // Display thinking
        }
        RunnerOutput::AssistantDelta(text) => {
            // Display response
        }
        RunnerOutput::ToolCallRequested { call, blocking } => {
            // Tool execution happens automatically in runner
            // This is for notification/UI updates
        }
        RunnerOutput::ApprovalRequest(request) => {
            // Present approval request to user
            // User responds via RunnerInput::Approval
        }
        RunnerOutput::StatePatch(patch) => {
            // Update UI state (todos, etc.)
        }
        RunnerOutput::TurnComplete => {
            // Turn finished
        }
        RunnerOutput::Cancelled => {
            // Run was cancelled
            break;
        }
        RunnerOutput::Error(err) => {
            // Handle error
        }
    }
}
```

## Feature Placement Guide

| Feature | Belongs In | Reason |
|---------|-----------|--------|
| Tool implementations | **hh-coding-agent** | Core agent functionality |
| Permission rules/matching | **hh-coding-agent** | Security policy |
| Session event types | **hh-coding-agent** | Event definitions |
| Sub-agent orchestration | **hh-coding-agent** | Task management |
| Output sanitization | **hh-coding-agent** | Safety concern |
| Agent runner/loop | **hh-coding-agent** | Runtime orchestration |
| System prompts | **hh-coding-agent** | Agent behavior |
| LLM communication | **hh-agent** | Provider protocol |
| File-based session storage | **Application** | Infrastructure |
| Provider implementations | **Application** | Backend integration |
| TUI/CLI | **Application** | Presentation |
| Config file loading | **Application** | Infrastructure |

### Decision Questions

When adding a feature, ask:

1. **Is this a tool that the LLM can call?**
   - Yes → hh-coding-agent (tool implementation)

2. **Is this about approving/denying operations?**
   - Yes → hh-coding-agent (permission system)

3. **Is this about recording what happened?**
   - Yes → hh-coding-agent (session events)

4. **Is this about HOW we talk to the LLM?**
   - Yes → hh-agent (Provider trait)

5. **Is this about WHERE we store data?**
   - Yes → Application (persistence implementation)

6. **Is this about HOW the user interacts?**
   - Yes → Application (UI/CLI)

## API Stability

The crate maintains stable APIs for:

- `Tool` trait and all tool implementations
- `ToolRegistry` and `ToolExecutor`
- `ApprovalPolicy`, `ApprovalDecision`, `ApprovalChoice`
- `SessionEvent`, `SessionMetadata`
- `SessionSink`, `SessionReader` traits
- `RunnerInput`, `RunnerOutput`, `RunnerState`
- `SubagentManager` and related types
- `sanitize_tool_output` function

Breaking changes will be versioned appropriately. The crate is designed to be a stable foundation for building coding agent applications.
