# hh-agent

A provider-agnostic LLM agent loop crate. This is the pure core of an agent runtime, responsible solely for orchestrating the conversation with an LLM provider and managing tool call turn semantics.

## What Is This Crate?

`hh-agent` implements a pure state machine that:

- Streams responses from LLM providers
- Manages conversation message history
- Tracks pending tool calls per turn
- Enforces turn completion invariants (all tool calls must be resolved before next turn)
- Distinguishes blocking vs non-blocking tools

**This crate does NOT:**
- Execute tools
- Persist sessions
- Manage typed application state
- Handle user approvals or interactive prompts
- Know about the filesystem, OS, or UI

## Responsibilities

The crate has a single, well-scoped responsibility: **manage the LLM conversation loop and turn semantics**.

### In Scope
- Provider communication via the `Provider` trait
- Message history management
- System prompt injection
- Tool schema collection via `ToolRegistry` trait
- Turn boundaries and pending tool call tracking
- Blocking vs non-blocking tool distinction
- Streaming delta emission
- Cancellation signal handling

### Out of Scope
- Tool execution
- Session persistence
- Typed state management (TODOs, counters, etc.)
- Approval workflows and policy
- UI rendering or event handling
- Sub-agent orchestration

## Core Loop Flow

The ```
                         ┌─────────────────────────┐
                         │     Initialize      │
                         │  AgentLoop::new()   │
                         └──────────┬──────────┘
                                    │
                                    ▼
                         ┌─────────────────────────┐
                         │   Wait for first      │
                         │   AgentInput::Message   │
                         └──────────┬──────────────┘
                                    │
                                    ▼
                         ┌─────────────────────────┐
                         │  Inject system prompt   │
                         │  (if not present)       │
                         └──────────┬──────────────┘
                                    │
                                    ▼
                    ┌─────────────────────────────────────────┐
                    │              MAIN STEP LOOP                   │
                    │  ┌────────────────────────────────────────┐  │
                    │  │ Check max_steps                        │  │
                    │  │    if step >= max: return error        │  │
                    │  └────────────────────────────────────────┘  │
                    │  ┌────────────────────────────────────────┐  │
                    │  │ Drain pending messages                 │  │
                    │  └────────────────────────────────────────┘  │
                    │  ┌────────────────────────────────────────┐  │
                    │  │ Execute Turn (see below)              │  │
                    │  └────────────────────────────────────────┘  │
                    │  ┌────────────────────────────────────────┐  │
                    │  │ Turn returned final answer?              │  │
                    │  │   Yes → return answer, done              │  │
                    │  │   No → step++, continue loop             │  │
                    │  └────────────────────────────────────────┘  │
                    └─────────────────────────────────────────────────┘
    ```

### Turn Execution

    ```
                         ┌─────────────────────────────────────────┐
                         │       Build Provider Request          │
                         │  messages + ephemeral_state + tools    │
                         └────────────────┬──────────────────────┘
                                          │
                                          ▼
                         ┌─────────────────────────────────────────┐
                         │    provider.complete_stream()           │
                         │                                       │
                         │  While streaming:                      │
                         │    ThinkingDelta → emit                │
                         │    AssistantDelta → emit               │
                         └────────────────┬──────────────────────┘
                                          │
                                          ▼
                         ┌─────────────────────────────────────────┐
                         │        Process Response               │
                         │                                       │
                         │  1. Add assistant message to history   │
                         │  2. Emit MessageAdded                  │
                         └────────────────┬──────────────────────┘
                                          │
                                          ▼
                         ┌─────────────────────────────────────────┐
                         │    response.done == true?               │
                         │                                       │
                         │  Yes → emit TurnComplete               │
                         │       return final content             │
                         │                                       │
                         │  No → process tool calls (below)       │
                         └────────────────┬──────────────────────┘
                                          │
                                          ▼
                         ┌─────────────────────────────────────────┐
                         │      Process Tool Calls                │
                         │                                       │
                         │  For each tool_call:                  │
                         │    is_blocking = registry.is_blocking()│
                         │    emit ToolCallRequested { blocking }  │
                         │                                       │
                         │    if is_blocking:                     │
                         │      WAIT for ToolResult (blocks loop)  │
                         │    else:                               │
                         │      CONTINUE (don't wait)              │
                         │                                       │
                         │  After all blocking tools resolved:     │
                         │    Wait for non-blocking results        │
                         │    When all resolved: return to step    │
                         └─────────────────────────────────────────┘
    ```

### Blocking vs Non-Blocking Tools

    ```
    Tool Call Received
           │
           ▼
    ┌──────────────────┐
    │ is_blocking?     │
    └────────┬─────────┘
             │
      ┌──────┴──────┐
      │             │
   NO               YES
      │             │
      ▼             ▼
┌──────────────┐   ┌────────────────────┐
│ Non-Blocking  │   │    Blocking       │
│              │   │                  │
│ Emit event   │   │ Emit event        │
│ immediately  │   │                  │
│              │   │ WAIT for result   │
│ Loop         │   │ (loop blocks)    │
│ continues    │   │                  │
│ processing   │   │ Register result   │
└──────────────┘   └────────────────────┘
      │                    │
      │                    │
      ▼                    ▼
    ┌───────────────────────────────┐
    │  Both: Add call_id to        │
    │  pending_tool_call_ids       │
    │                              │
    │  Wait for all to resolve     │
    │  before next provider call   │
    └───────────────────────────────┘
    ```

### Cancellation Path

    ```
    At any point:
           │
           ▼
    ┌───────────────────────┐
    │ AgentInput::Cancel    │
    │ received              │
    └───────────┬───────────┘
                │
                ▼
    ┌───────────────────────┐
    │ 1. cancelled = true   │
    │ 2. Emit Cancelled     │
    │ 3. Clear pending ids  │
    │ 4. Return error       │
    └───────────────────────┘
    ```

### Key Invariants

1. **Turn Completion**: A turn only completes when:
   - `response.done == true` (LLM finished without more tool calls), OR
   - All `pending_tool_call_ids` have been resolved

2. **Message Ordering**: Messages are always appended in order:
   - System prompt (if injected) at index 0
   - User/assistant/tool messages appended chronologically

3. **Tool Result Correlation**: Each `ToolResult` must match a pending `call_id`

4. **Blocking Guarantee**: For blocking tools, the loop guarantees the tool result is received before processing the next tool call in the same turn

5. **Cancellation Immediacy**: `Cancel` input causes immediate termination regardless of current state

### Loop State

```
┌───────────────────────────────────────────────────────────────┐
│                         AgentLoop                             │
├───────────────────────────────────────────────────────────────┤
│  messages: Vec<Message>         -- Conversation history        │
│  pending_tool_call_ids: HashSet<String>  -- Unresolved calls   │
│  tool_results: HashMap<String, ToolResult> -- Cached results   │
│  ephemeral_state: Option<Message>       -- State for next req  │
│  cancelled: bool                        -- Cancellation flag    │
└───────────────────────────────────────────────────────────────┘
```

**State Transitions:**

| Event | State Change |
|------|-------------|
| `Message` received | `messages.push(message)` |
| `ToolCallRequested` | `pending_tool_call_ids.insert(call_id)` |
| `ToolResult` received | `tool_results.insert(call_id, result)` |
| Tool result registered | `pending_tool_call_ids.remove(call_id)` |
| `SetEphemeralState` | `ephemeral_state = state` |
| `Cancel` | `cancelled = true` |

## Interface

The crate exposes three main components:

### 1. Types

```rust
// Core domain types
pub struct Message { role: Role, content: String, attachments: Vec<MessageAttachment>, ... }
pub struct ToolCall { id: String, name: String, arguments: Value }
pub struct ToolSchema { name: String, description: String, blocking: bool, parameters: Value, ... }
pub struct ToolResult { is_error: bool, summary: String, output: String, ... }

// Provider types
pub struct ProviderRequest { model: String, messages: Vec<Message>, tools: Vec<ToolSchema> }
pub struct ProviderResponse { assistant_message: Message, tool_calls: Vec<ToolCall>, done: bool, ... }
pub enum ProviderStreamEvent { AssistantDelta(String), ThinkingDelta(String) }
```

### 2. Traits

#### Provider Trait

Implement this to integrate with different LLM backends:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Non-streaming completion (optional, has default impl)
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse>;

    /// Streaming completion (required)
    async fn complete_stream<F>(
        &self,
        req: ProviderRequest,
        on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send;
}
```

#### ToolRegistry Trait

Implement this to provide tool schemas and blocking behavior:

```rust
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// Return all available tool schemas
    fn schemas(&self) -> Vec<ToolSchema>;

    /// Return true if the tool should block the loop until its result is ready
    fn is_blocking(&self, tool_name: &str) -> bool;
}
```

**Blocking vs Non-Blocking:**
- **Blocking**: The loop waits for `AgentInput::ToolResult` before processing the next tool call or turn. Use for tools that must complete before the LLM can continue.
- **Non-blocking**: The loop emits `ToolCallRequested` and continues. Results may be provided out of order. Use for long-running tasks that can complete asynchronously.

### 3. AgentLoop

The main entry point:

```rust
pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_steps: usize,
}

impl<P: Provider, R: ToolRegistry> AgentLoop<P, R> {
    pub fn new(provider: P, tool_registry: R, config: AgentConfig) -> Self;

    pub async fn run<D>(
        &mut self,
        messages: &mut Vec<Message>,
        input_rx: mpsc::Receiver<AgentInput>,
        emit_output: &mut (impl FnMut(AgentOutput) + Send),
        drain_pending_messages: D,
    ) -> anyhow::Result<Option<String>>
    where
        D: FnMut() -> Vec<Message>;
}
```

### Event Types

#### AgentInput (sent TO the loop)

```rust
pub enum AgentInput {
    /// Add a user or tool message to the conversation
    Message(Message),

    /// Provide the result of a tool execution
    ToolResult {
        call_id: String,
        result: ToolResult,
    },

    /// Inject ephemeral state (e.g., current TODO list) into the next LLM request
    SetEphemeralState(Option<Message>),

    /// Cancel the current run
    Cancel,
}
```

#### AgentOutput (emitted FROM the loop)

```rust
pub enum AgentOutput {
    /// Streaming thinking content delta
    ThinkingDelta(String),

    /// Streaming assistant content delta
    AssistantDelta(String),

    /// A complete message was added to history
    MessageAdded(Message),

    /// The LLM requested a tool call
    /// - `blocking`: whether this tool requires a result before continuing
    ToolCallRequested {
        call: ToolCall,
        blocking: bool,
    },

    /// Turn completed (LLM finished without more tool calls)
    TurnComplete,

    /// Context token usage update
    ContextUsage(usize),

    /// Run was cancelled
    Cancelled,

    /// Error occurred
    Error(String),
}
```

## Integration Guide

### Basic Integration

To use `hh-agent` in your application:

1. **Implement `Provider`** for your LLM backend

2. **Implement `ToolRegistry`** to expose your tool schemas:
   ```rust
   struct MyTools { schemas: Vec<ToolSchema> }

   impl ToolRegistry for MyTools {
       fn schemas(&self) -> Vec<ToolSchema> { self.schemas.clone() }
       fn is_blocking(&self, name: &str) -> bool {
           matches!(name, "bash" | "write" | "edit")
       }
   }
   ```

3. **Create channels** and run the loop:
   ```rust
   let (input_tx, input_rx) = mpsc::channel(256);

   let mut loop = AgentLoop::new(provider, tools, config);
   let result = loop.run(
       &mut messages,
       input_rx,
       &mut |output| handle_output(output),
       &mut drain_pending,
   ).await;
   ```

4. **Handle outputs** and respond with inputs:
   - `ToolCallRequested` → execute tool in your application, send `AgentInput::ToolResult`
   - `ThinkingDelta`/`AssistantDelta` → stream to your UI
   - `TurnComplete` → finalize turn state in your application
   - User input → send `AgentInput::Message`

### Handling Tool Calls

When you receive `ToolCallRequested`:

```rust
match output {
    AgentOutput::ToolCallRequested { call, blocking } => {
        // Execute the tool in your application layer
        let result = execute_tool(&call.name, call.arguments).await;

        // Send the result back to the loop
        input_tx.send(AgentInput::ToolResult {
            call_id: call.id,
            result,
        }).await?;

        // For non-blocking tools, you might spawn execution
        // and send results when ready
    }
    _ => {}
}
```

### Ephemeral State

To inject runtime state (like a TODO list) into the LLM context:

```rust
let state_message = Message {
    role: Role::System,
    content: format!("Current state: {:?}", my_state),
    ..Default::default()
};
input_tx.send(AgentInput::SetEphemeralState(Some(state_message))).await?;
```

This message is appended to the request on the next provider call, but not persisted to message history.

### Cancellation

To cancel an in-progress run:

```rust
input_tx.send(AgentInput::Cancel).await?;
```

The loop will emit `AgentOutput::Cancelled` and return a cancellation error.

## Feature Placement Guide

Use this guide to decide if a feature belongs in `hh-agent` or your application layer:

| Feature | Belongs In | Reason |
|---------|-----------|--------|
| New provider (Anthropic, Gemini, etc.) | **hh-agent** (Provider impl) | Core LLM communication |
| Tool schema format changes | **hh-agent** | Affects Provider contract |
| Streaming delta format | **hh-agent** | Core output type |
| Turn completion invariants | **hh-agent** | Core loop semantics |
| Message/role types | **hh-agent** | Core domain types |
| Tool execution | **Application** | Side effects |
| Session persistence | **Application** | Infrastructure |
| Approval workflows | **Application** | Security policy |
| Typed state (TODOs, etc.) | **Application** | Application domain |
| Sub-agent orchestration | **Application** | Orchestration logic |
| UI rendering | **Application** | Presentation |
| Error sanitization | **Application** | Security concern |

### Decision Questions

When adding a feature, ask:

1. **Does the LLM need to know about this?**
   - Yes → Consider hh-agent
   - No → Application layer

2. **Is this about HOW we talk to the LLM or WHAT we do with the response?**
   - HOW → hh-agent (Provider, streaming, message format)
   - WHAT → Application (execution, state, UI)

3. **Would this apply to ANY application using this crate?**
   - Yes → hh-agent
   - No, it's application-specific → Application layer

4. **Does this require side effects (filesystem, network, OS)?**
   - Yes → Application layer (hh-agent is pure)

## Expected Behavior

### Turn Semantics

1. **Turn Start**: Triggered by receiving the first `AgentInput::Message`
2. **System Prompt**: Injected automatically at index 0 if absent from message history
3. **Provider Call**: Loop makes request with messages + ephemeral state + tool schemas
4. **Tool Calls**:
   - Blocking: Emits `ToolCallRequested`, waits for `ToolResult` before continuing
   - Non-blocking: Emits `ToolCallRequested`, continues without waiting
5. **Turn End**: When all pending tool calls are resolved and LLM returns `done: true`
6. **Next Turn**: Loop waits for new input or drains pending messages

### Cancellation

1. `AgentInput::Cancel` received at any point
2. Loop immediately emits `AgentOutput::Cancelled`
3. Loop returns `Err` with cancellation message
4. `is_cancellation_error(err)` returns `true` for cancellation errors

### Channel Behavior

- **Input channel closes**: Loop treats this as termination signal
- **Output callback fails**: Error propagates to caller
- **No pending tool results for blocking tools**: Loop waits indefinitely

### Message History

- Loop owns messages via mutable reference
- System prompt injected once at index 0 if absent
- All messages (user, assistant, tool results) are appended
- Application layer is responsible for persistence/replay

## Testing Guide

### Unit Testing the Loop

Test the loop in isolation with mock provider and registry:

```rust
struct MockProvider {
    responses: VecDeque<ProviderResponse>,
}

impl Provider for MockProvider {
    async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
        self.responses.lock().unwrap().pop_front().unwrap()
    }
}

struct MockRegistry;

impl ToolRegistry for MockRegistry {
    fn schemas(&self) -> Vec<ToolSchema> { vec![] }
    fn is_blocking(&self, _name: &str) -> bool { true }
}

#[tokio::test]
async fn test_final_answer() {
    let provider = MockProvider {
        responses: vec![ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: "Done!".into(),
                ..Default::default()
            },
            tool_calls: vec![],
            done: true,
            thinking: None,
            context_tokens: None,
        }].into(),
    };

    let mut loop = AgentLoop::new(provider, MockRegistry, AgentConfig::default());
    let (tx, rx) = mpsc::channel(10);
    tx.send(AgentInput::Message(Message {
        role: Role::User,
        content: "Hello".into(),
        ..Default::default()
    })).await.unwrap();

    let mut messages = vec![];
    let mut outputs = vec![];
    let result = loop.run(&mut messages, rx, &mut |o| outputs.push(o), &mut Vec::new).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some("Done!".to_string()));
}
```

### Key Test Scenarios

1. **Final Answer**: LLM returns `done: true` with no tool calls → loop returns content
2. **Tool Calls**: LLM returns tool calls → loop emits `ToolCallRequested`, waits for results
3. **Blocking Tools**: Blocking tool call → loop waits before processing next
4. **Non-Blocking Tools**: Non-blocking tool call → loop continues without waiting
5. **Cancellation**: `Cancel` input → loop emits `Cancelled`, returns error
6. **Max Steps**: Exceeds `max_steps` → loop returns error
7. **System Prompt**: No system message in history → loop injects it
8. **Ephemeral State**: `SetEphemeralState` → included in next provider request
9. **Channel Close**: Input channel closes → loop terminates
10. **Out-of-Order Results**: Tool results arrive out of order → loop handles correctly

### Testing Provider Implementations

Test your `Provider` implementation independently:

```rust
#[tokio::test]
async fn test_my_provider() {
    let provider = MyProvider::new(...);
    let req = ProviderRequest {
        model: "test".into(),
        messages: vec![],
        tools: vec![],
    };

    let mut events = vec![];
    let response = provider.complete_stream(req, |e| events.push(e)).await.unwrap();

    assert!(response.done);
    assert!(!events.is_empty());
}
```

### Testing ToolRegistry Implementations

```rust
#[test]
fn test_my_registry() {
    let registry = MyTools::new();
    assert!(registry.is_blocking("bash"));
    assert!(!registry.is_blocking("task"));
}
```

## API Stability

The crate maintains a stable API for:

- `AgentLoop::new()` and `AgentLoop::run()`
- `AgentInput` and `AgentOutput` enums
- `Provider` and `ToolRegistry` traits
- Core types: `Message`, `ToolCall`, `ToolResult`, `ToolSchema`

Breaking changes will be versioned appropriately. The crate is designed to be a stable foundation for building agent systems.
