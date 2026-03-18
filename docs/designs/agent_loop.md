# Agent Loop Design

This document details the architecture of the Agent execution model.

## Overview

The architecture implements an Event-Driven Actor Model via a two-crate, two-layer design. This separation prioritizes explicit data flow, testability, and safety by completely decoupling LLM state transitions from system side-effects and typed state management.

### Crate Structure

- **`hh-agent` crate**: The pure LLM interaction loop. Contains `AgentLoop` and core types.
- **`hh-cli` crate**: The application layer. Contains `AgentRunner`, tool execution, session persistence, and UI.

### Layer 1: AgentLoop (hh-agent crate - The Pure State Machine)

The inner layer is responsible *solely* for managing the LLM interaction loop and conversation state.

- **Responsibilities:**
  - Streams tokens from the LLM Provider via `Provider::complete_stream`.
  - Tracks pending tool `call_id`s per turn and enforces turn completion invariants.
  - Distinguishes blocking vs non-blocking tools based on `ToolRegistry::is_blocking`.
  - Injects system prompt if absent from message history.
  - Emits pure events representing intent (e.g., `ToolCallRequested`).
  - Accepts external `SetEphemeralState` messages to inject into the LLM context window.
- **Knowledge:** It has zero knowledge of the OS, the filesystem, UI/CLI, persistence, or typed application state (like TODOs). To the loop, all tools are identical black boxes identified only by name and blocking status.
- **Interface:** Communicates via `AgentInput` and `AgentOutput` channels.

### Layer 2: AgentRunner (hh-cli crate - The Orchestrator)

The outer layer wraps `AgentLoop` and manages side-effects, typed state, concurrency, and security policies.

- **Responsibilities:**
  - Owns the canonical conversation transcript (`Vec<Message>`) via session replay.
  - Owns strongly-typed `RunnerState` (e.g., TODO items, context token count).
  - Evaluates tool calls against the `ApprovalPolicy` trait.
  - Executes tools via the `ToolExecutor` trait and applies `StatePatch` values.
  - Manages concurrent, non-blocking tool execution (`FuturesUnordered`).
  - Routes interactive tools (like `question`) and approval requests to the UI layer.
  - Persists events via `SessionSink` and replays via `SessionReader` traits.
  - Manages sub-agent lifecycle via `SubagentManager`.
  - Emits `RunnerOutput` events to UI and session.
- **Interface:** Communicates with `AgentLoop` via inner channels, and with the CLI/TUI via `RunnerInput` and `RunnerOutput` channels.

---

## State Management and Data Flow

### AgentLoop Configuration

The `AgentLoop` is configured with provider, tool registry, and config:

```rust
pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_steps: usize,
}

pub struct AgentLoop<P: Provider, R: ToolRegistry> {
    provider: P,
    tool_registry: R,
    config: AgentConfig,
    pending_tool_call_ids: HashSet<String>,
    tool_results: HashMap<String, ToolResult>,
    blocking_tools: HashSet<String>,
    ephemeral_state: Option<Message>,
}
```

### The Runner State

The system relies on typed state managed by the Runner. Tools can mutate this state through `StatePatch` operations.

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerState {
    pub todo_items: Vec<TodoItem>,
    pub context_tokens: usize,
}

#[derive(Debug, Clone, Default)]
pub struct StatePatch {
    pub ops: Vec<StateOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StateOp {
    SetTodoItems { items: Vec<TodoItem> },
    SetContextTokens { tokens: usize },
}
```

### Tool Execution Flow (State as a Reducer)

Tools are treated as state patch producers.

1. The `AgentRunner` receives `ToolCallRequested` from the `AgentLoop`.
2. The Runner evaluates approval via `ApprovalPolicy::decision_for_tool_call`.
3. If approval required, the Runner emits `ApprovalRequired` and waits for `ApprovalDecision`.
4. The Runner executes each tool call via `ToolExecutor::execute`.
5. The `ToolExecution` returns `ToolResult` plus an optional `StatePatch`.
6. The Runner applies each patch to `RunnerState` via `RunnerState::apply_patch`.

### Turn Ordering and Non-Blocking Tool Completions

Turn boundaries are enforced by `AgentLoop` tracking pending tool call IDs.

- For each provider turn, `AgentLoop` tracks a pending set of tool `call_id`s.
- The loop does not issue the next provider request until every `call_id` has a corresponding `AgentInput::ToolResult` (or cancellation).
- Blocking tools are processed sequentially; the loop waits for each result before proceeding.
- Non-blocking tools may complete out of order.
- Out-of-order completions are valid because correlation is by `call_id`, not arrival order.
- The Runner may execute non-blocking tools concurrently via `FuturesUnordered`, but patch application to `RunnerState` is always sequential.

### Cancellation Model

Cancellation is whole-turn and best-effort immediate.

- `RunnerInput::Cancel` causes the Runner to emit `RunnerOutput::Cancelled` and bail with a cancellation error.
- Dropping the `AgentLoop` future cancels any in-flight provider calls.
- No new tool executions are started after cancellation begins.
- The `is_cancellation_error` function identifies cancellation errors by message content.

### Replay and Resumption

Replay uses session events plus a persisted runner-state snapshot.

- `SessionStore` persists events to JSONL and metadata (including `runner_state_snapshot`) to JSON.
- On resume, `AgentRunner` loads the snapshot via `SessionReader::load_runner_state_snapshot`.
- `AgentRunner::hydrate_state_from_replayed_tool_results` rebuilds state from events if no snapshot exists.
- New tool results continue to be appended as session events for inspectability.
- Snapshot writes happen after each state change via `RunnerOutput::SnapshotUpdated`.

### Backpressure and Channel Policy

Channels between AgentLoop/Runner/UI are bounded.

- `RunnerOutputChannel` uses bounded queues with coalescing under pressure.
- Delta-style streams (`ThinkingDelta`, `AssistantDelta`) are coalescible and dropped when full.
- Control-plane events (`ToolStart`, `ToolEnd`, `ApprovalRequired`, `TurnComplete`, `Cancelled`) are never dropped; the channel waits if full.
- `AgentLoop` uses a 512-slot output channel; Runner input uses 256 slots.

### State Projection

The Runner acts as the translator between the typed system state and the LLM's text-only world.

- **To the UI:** The Runner emits `RunnerOutput::StateUpdated(RunnerState)`. The UI mirrors this state to components (e.g., sidebar).
- **To the AgentLoop:** The Runner formats `RunnerState` into a system message via `state_for_llm()` and sends `AgentInput::SetEphemeralState`. The loop appends this to the next LLM request.

---

## Event Models

### AgentLoop Channels (hh-agent crate)

```rust
pub enum AgentInput {
    Message(Message),
    ToolResult { call_id: String, result: ToolResult },
    SetEphemeralState(Option<Message>),
    Cancel,
}

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
```

### Runner Channels (hh-cli crate)

```rust
pub enum RunnerInput {
    Message(Message),
    ApprovalDecision { call_id: String, choice: ApprovalChoice },
    QuestionAnswered { call_id: String, answers: QuestionAnswers },
    Cancel,
}

pub enum RunnerOutput {
    // LLM streaming
    ThinkingDelta(String),
    ThinkingRecorded(String),     // Persisted thinking content
    AssistantDelta(String),
    MessageAdded(Message),

    // Tool lifecycle
    ToolCallRecorded(ToolCall),   // Recorded to session
    ToolStart { call_id: String, name: String, args: Value },
    ToolEnd { call_id: String, name: String, result: ToolResult },

    // State management
    StateUpdated(RunnerState),    // Changed state to UI
    SnapshotUpdated(RunnerState), // Persisted snapshot

    // Interactive requests
    ApprovalRequired { call_id: String, request: ApprovalRequest },
    ApprovalRecorded { tool_name: String, approved: bool, action: Option<Value>, choice: Option<ApprovalChoice> },
    QuestionRequired { call_id: String, prompts: Vec<QuestionPrompt> },

    // Lifecycle
    TurnComplete,
    Cancelled,
    Error(ErrorPayload),
}
```

---

## Core Traits

### Provider Trait (hh-agent crate)

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse>;

    async fn complete_stream<F>(
        &self,
        req: ProviderRequest,
        on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send;
}
```

### ToolRegistry Trait (hh-agent crate)

```rust
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    fn schemas(&self) -> Vec<ToolSchema>;
    fn is_blocking(&self, tool_name: &str) -> bool;
}
```

### ToolExecutor Trait (hh-cli crate)

```rust
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn schemas(&self) -> Vec<ToolSchema>;
    async fn execute(&self, name: &str, args: Value) -> ToolExecution;
    fn apply_approval_decision(&self, action: &Value, choice: ApprovalChoice) -> anyhow::Result<bool>;
    fn is_non_blocking(&self, name: &str) -> bool;
}
```

### ApprovalPolicy Trait (hh-cli crate)

```rust
pub trait ApprovalPolicy: Send + Sync {
    fn decision_for_tool_call(&self, tool_name: &str, args: &Value) -> ApprovalDecision;
}
```

### Session Traits (hh-cli crate)

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

---

## Sub-Agent Management

The `SubagentManager` orchestrates sub-agent task execution with lifecycle persistence.

### SubagentManager

```rust
pub struct SubagentManager {
    inner: Arc<Mutex<SubagentManagerState>>,
    queue: Arc<Semaphore>,      // Limits parallel execution
    max_depth: usize,            // Maximum nesting depth
    executor: SubagentExecutor,  // Callback for execution
}
```

### Subagent Status

```rust
pub enum SubagentStatus {
    Pending,    // UI label: "queued"
    Running,    // UI label: "running"
    Completed,  // UI label: "done"
    Failed,     // UI label: "error"
    Cancelled,  // UI label: "cancelled"
}
```

### Lifecycle Events

Sub-agent lifecycle is persisted via `SessionEvent`:

- `SubAgentStart`: Created with `Pending` status, depth, and prompt.
- `SubAgentProgress`: Progress updates with sequence numbers.
- `SubAgentResult`: Terminal status, summary, and optional failure reason.

---

## Session Event Types

```rust
pub enum SessionEvent {
    Message { id: String, message: Message },
    ToolCall { call: ToolCall },
    ToolResult { id: String, is_error: bool, output: String, result: Option<ToolResult> },
    Approval { id: String, tool_name: String, approved: bool, action: Option<Value>, choice: Option<ApprovalChoice> },
    Thinking { id: String, content: String },
    Compact { id: String, summary: String },
    SubAgentStart { id: String, task_id: Option<String>, name: Option<String>, parent_id: Option<String>, parent_session_id: Option<String>, agent_name: Option<String>, session_id: Option<String>, status: SubAgentLifecycleStatus, created_at: u64, updated_at: u64, prompt: String, depth: usize },
    SubAgentProgress { id: String, task_id: Option<String>, seq: u64, content: String },
    SubAgentResult { id: String, task_id: Option<String>, status: SubAgentLifecycleStatus, summary: Option<String>, failure_reason: Option<SubAgentFailureReason>, is_error: bool, output: String },
}
```

---

## Bridging AgentLoop to RunnerOutput

The `AgentRunner::run_input_loop` method bridges between the two layers:

1. Creates `AgentLoop` with provider and `ToolRegistryAdapter`.
2. Spawns the loop's `run()` future pinned with `tokio::pin!`.
3. Selects over:
   - Agent loop completion
   - `AgentOutput` from the loop (translated to `RunnerOutput`)
   - `RunnerInput` from external (translated to `AgentInput`)
   - Non-blocking tool completions
4. When `ToolCallRequested` is received, the Runner handles tool execution with approval/question bridging.
5. Tool results are sent back via `AgentInput::ToolResult`.
6. State changes emit `StateUpdated` and `SnapshotUpdated`.

The `apply_runner_output_to_observer` helper function applies `RunnerOutput` to:
- A `RunnerOutputObserver` trait implementation (UI callbacks)
- A `SessionSink` for persistence
