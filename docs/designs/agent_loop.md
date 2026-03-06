# Agent Loop Design

This document details the architecture of the Agent execution model.

## Overview
The architecture implements an Event-Driven Actor Model via a two-layer design. This separation prioritizes explicit data flow, testability, and safety by completely decoupling LLM state transitions from system side-effects and typed state management.

### Layer 1: Agent Core (The Pure State Machine)
The inner layer is responsible *solely* for managing the LLM interaction loop and conversation state.

- **Responsibilities:** 
  - Streams tokens and tool calls from the LLM Provider.
  - Maintains the conversational context (`Vec<Message>`).
  - Emits pure events representing intent (e.g., "The LLM wants to execute tool X").
  - Accepts external ephemeral text to inject into the LLM context window.
- **Knowledge:** It has zero knowledge of the OS, the filesystem, UI/CLI, persistence, or typed application state (like TODOs). To the Core, all tools are identical black boxes. It only knows about tool schemas provided to it during initialization.
- **Interface:** Communicates purely via `CoreInput` and `CoreOutput` channels.

### Layer 2: Agent Runner (The Orchestrator)
The outer layer wraps the Core and manages side-effects, typed state, concurrency, and security policies.

- **Responsibilities:**
  - Owns the strongly-typed `RunnerState` (e.g., TODO items, active subagents).
  - Evaluates tool calls against the `ApprovalPolicy`.
  - Manages concurrent, non-blocking tool execution (`FuturesUnordered`).
  - Routes interactive tools (like `"question"`) and security approvals to the UI layer.
  - Executes tools and applies returned `StatePatch` values to `RunnerState` sequentially.
  - Translates the new `RunnerState` into a text string (`state_for_llm`) and feeds it to the Core.
  - Emits the entire `RunnerState` to the UI via `RunnerOutput` whenever it changes.
- **Interface:** Communicates with the Core via inner channels, and with the CLI/TUI via `RunnerInput` and `RunnerOutput` channels.

---

## State Management and Data Flow

### Core Initialization (Configuration)
The `AgentCore` needs configuration data (tool schemas and system prompts) to send to the provider, but it does not execute tools itself.
```rust
pub struct AgentCore<P: Provider> {
    pub provider: P,
    pub model: String,
    pub system_prompt: String,
    pub tool_schemas: Vec<ToolSchema>, // Extracted from Runner's ToolExecutor
    pub max_steps: usize,
}
```

### The Runner State
The system relies on a globally known typed state managed by the Orchestrator. 

```rust
#[derive(Debug, Clone, Default)]
pub struct RunnerState {
    pub todo_items: Vec<TodoItem>,
    pub context_tokens: usize,
    // ... future typed state elements
}
```

### Tool Execution Flow (State as a Reducer)
Tools are treated as state patch producers.
1. The `AgentRunner` receives `ToolCallRequested` from the Core.
2. The Runner executes each tool call via the `ToolExecutor`.
3. The `Tool` returns `ToolResult` plus a typed `StatePatch` (or no-op patch).
4. The Runner applies each patch to `RunnerState` in a single-threaded reducer path.

### Turn Ordering and Non-Blocking Tool Completions
Turn boundaries are implicit and enforced by protocol invariants (no explicit `TurnStarted`/`TurnToolsCompleted` events required).

- For each provider turn, the Core tracks a pending set of tool `call_id`s.
- The Core does not issue the next provider request until every `call_id` in the current turn has a corresponding `CoreInput::ToolResult` (or the turn is cancelled).
- Blocking tools may complete immediately; non-blocking tools (for example, `task`) may complete out of order.
- Out-of-order completions are valid because correlation is by `call_id`, not arrival order.
- The Runner may execute non-blocking tools concurrently, but patch application to `RunnerState` is always sequential.

### Cancellation Model
Cancellation is whole-turn and best-effort immediate.

- `RunnerInput::Cancel` causes the Runner to cancel provider streaming, all in-flight tool futures, and pending approval/question waits.
- No new tool executions are started after cancellation begins.
- The Core resolves the turn as cancelled instead of waiting for missing tool results.
- The Runner emits a terminal lifecycle signal (`TurnComplete` with cancelled context, or equivalent) so UI/CLI can clear processing state deterministically.

### Replay and Resumption
Replay uses session events plus a persisted runner-state snapshot.

- The session metadata stores the latest serialized `RunnerState` snapshot.
- On resume, the Runner loads this snapshot before accepting new input.
- New tool results continue to be appended as session events for inspectability and debugging.
- Snapshot writes happen after each successful patch application and at turn completion.

### Backpressure and Channel Policy
Channels between Core/Runner/UI are bounded.

- Delta-style streams (thinking/assistant deltas) use bounded queues with coalescing under pressure.
- Control-plane events (tool start/end, approvals, questions, cancel) are never dropped.
- A small bound is acceptable for control channels; delta channels should use a higher bound to avoid excessive token loss.

### State Projection
The Runner acts as the translator between the typed system state and the LLM's text-only world, as well as the UI's visual world.
- **To the UI:** The Runner emits `RunnerOutput::StateUpdated(RunnerState)`. The UI blindly mirrors this state to its components (e.g., drawing the sidebar).
- **To the Core:** The Runner formats the `RunnerState` into a string (e.g., "1 pending out of 2 tasks...") and sends `CoreInput::SetEphemeralState`. The Core appends this text to the end of its next LLM request, ensuring the LLM always has the canonical ground truth.

---

## Event Models

### Core Channels
```rust
pub enum CoreInput {
    Message(Message),
    ToolResult { call_id: String, name: String, result: ToolResult },
    SetEphemeralState(Option<Message>),
    Cancel,
}

pub enum CoreOutput {
    ThinkingDelta(String),
    AssistantDelta(String),
    ContextUsage(usize),
    ToolCallRequested(ToolCall),
    TurnComplete,
    Error(String),
}
```

### Runner Channels
```rust
pub enum RunnerInput {
    Message(Message),
    ApprovalDecision { call_id: String, choice: ApprovalChoice },
    QuestionAnswered { call_id: String, answers: QuestionAnswers },
    Cancel,
}

pub enum RunnerOutput {
    // LLM & Turn state
    ThinkingDelta(String),
    AssistantDelta(String),
    MessageAdded(Message), 
    
    // Emitted whenever tools mutate the state, or token usage updates
    StateUpdated(RunnerState),
    
    // Interactive requests to the UI
    ApprovalRequired(ApprovalRequest),
    QuestionRequired { call_id: String, prompts: Vec<QuestionPrompt> },
    
    // Lifecycle events
    ToolStart { call_id: String, name: String, args: Value },
    ToolEnd { call_id: String, name: String, result: ToolResult },
    
    TurnComplete,
    Error(String),
}
```
