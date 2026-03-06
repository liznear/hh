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
  - Passes `RunnerState` into `Tool::execute` and receives the mutated state back.
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
Tools are treated as pure state reducers.
1. The `AgentRunner` receives `ToolCallRequested` from the Core.
2. The Runner passes the arguments and the current `RunnerState` into the `ToolExecutor`.
3. The `Tool` executes its logic, mutates the state if necessary, and returns `(ToolResult, RunnerState)`.
4. The Runner replaces its internal state with the new state.

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
    Error(anyhow::Error),
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
    Error(anyhow::Error),
}
```
