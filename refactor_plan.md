# Agent Loop Refactor Plan

This document outlines the step-by-step plan to refactor the `AgentLoop` into a two-layered, event-driven architecture utilizing a `RunnerState`.

## Step 1: Define Domain Types (Events/Channels/State)
Create the explicit data flow types for both layers.
- **Target File:** `src/core/agent/types.rs` (new file).
- **Tasks:**
  - Define `RunnerState` containing `todo_items`, `context_tokens`, etc.
  - Define `CoreInput` and `CoreOutput` enums.
  - Define `RunnerInput` and `RunnerOutput` enums.

## Step 2: Refactor Tool Interfaces
Update the tool execution pipeline to treat state as a reducer.
- **Target Files:** `src/core/traits.rs`, `src/tool/mod.rs`, `src/tool/todo.rs`, etc.
- **Tasks:**
  - Update `Tool::execute` to accept `RunnerState` and return `(ToolResult, RunnerState)`.
  - Update `ToolExecutor::execute` to pass the state through.
  - Implement the state mutation logic in `TodoWriteTool`.

## Step 3: Extract `AgentCore` (The Pure State Machine)
Strip side-effects and typed state from the current `AgentLoop`.
- **Target File:** `src/core/agent/core.rs` (extracted from `mod.rs`).
- **Tasks:**
  - Rename `AgentLoop` to `AgentCore`.
  - Remove generic bounds for `ToolExecutor`, `ApprovalPolicy`, and `SessionSink`.
  - Add `tool_schemas: Vec<ToolSchema>` and `system_prompt: String` fields.
  - Replace the complex closures with a `tokio::select!` channel loop.
  - Manage a simple `Vec<Message>` instead of `AgentState`.
  - Implement `CoreInput::SetEphemeralState` logic to inject text at the end of the Provider request.

## Step 4: Implement `AgentRunner` (The Orchestrator)
Create the new side-effect and concurrency manager.
- **Target File:** `src/core/agent/runner.rs` (new file).
- **Tasks:**
  - Create `AgentRunner` struct that owns `AgentCore` (initialized with schemas/prompts), `ToolExecutor`, `ApprovalPolicy`, and `RunnerState`.
  - Implement the main loop multiplexing `RunnerInput` and `CoreOutput`.
  - Implement tool execution: Evaluate approvals, route questions to UI, run tools via executor.
  - Upon tool completion, update `RunnerState`, format the state for the LLM, and send `SetEphemeralState` to the Core.
  - Emit `RunnerOutput::StateUpdated` to the UI whenever the state changes.

## Step 5: Adapt Session Persistence
Move session writing out of the core and into a pure observer.
- **Target Files:** `src/cli/chat/agent_run.rs`, `src/cli/tui/app.rs`.
- **Tasks:**
  - Remove `session.append(...)` calls from the core logic.
  - Create a mechanism in the UI/CLI layer that listens to `RunnerOutput` and writes corresponding events to `SessionSink`.
  - Note: Resumption will still replay historical `ToolResult`s through the Runner to rebuild the `RunnerState`.

## Step 6: Update UI Adapters (CLI & TUI)
Wire the existing frontends to the new event-driven runner.
- **Target Files:** `src/cli/chat/agent_run.rs`, `src/cli/tui/app.rs`.
- **Tasks:**
  - Swap the old generic callback logic for a channel-based listener.
  - In the TUI, listen for `RunnerOutput::StateUpdated` and bind it directly to the UI's render state (e.g. `self.todo_items = state.todo_items`).

## Step 7: Cleanup and Verification
- **Tasks:**
  - Delete `src/core/agent/state.rs` (replaced by `RunnerState` and ephemeral text injection).
  - Remove deprecated traits (`AgentEvents`, `NoopEvents`).
  - Run the test suite (`cargo test`) and fix any mock/adapter breakages.
  - Run `cargo clippy -- -D warnings`.
