# Agent Loop Refactor Plan

This document outlines the step-by-step plan to refactor the `AgentLoop` into a two-layered, event-driven architecture utilizing a `RunnerState`.

## Step 1: Define Domain Types (Events/Channels/State) - PARTIALLY COMPLETE
Create the explicit data flow types for both layers.
- **Target File:** `src/core/agent/types.rs` (update existing file).
- **Tasks:**
  - [x] Finalize `RunnerState` (initially `todo_items`, `context_tokens`, plus future typed fields).
  - [x] Finalize `CoreInput` and `CoreOutput` enums (transport-safe error payloads).
  - [x] Finalize `RunnerInput` and `RunnerOutput` enums, including cancellation path.
  - [x] Document protocol invariants: one tool result per `call_id`, and next provider call only after all tool calls in the current turn resolve.

## Step 2: Refactor Tool Interfaces - PARTIALLY COMPLETE
Update the tool execution pipeline to treat state as a reducer.
- **Target Files:** `src/core/traits.rs`, `src/tool/mod.rs`, `src/tool/todo.rs`, etc.
- **Tasks:**
  - [x] Introduce a typed `StatePatch`/`StateOp` model.
  - [x] Update `Tool::execute` and `ToolExecutor::execute` to return `ToolResult` + `StatePatch`.
  - [~] Keep patch generation inside tools; keep patch application centralized in the Runner. (Applied centrally in existing `AgentLoop`; pending `AgentRunner` extraction.)
  - [x] Implement `TodoWriteTool` to emit a patch that updates canonical TODO state.

## Step 3: Extract `AgentCore` (The Pure State Machine) - NOT STARTED
Strip side-effects and typed state from the current `AgentLoop`.
- **Target File:** `src/core/agent/core.rs` (extracted from `mod.rs`).
- **Tasks:**
  - [ ] Rename `AgentLoop` to `AgentCore`.
  - [ ] Remove generic bounds for `ToolExecutor`, `ApprovalPolicy`, and `SessionSink`.
  - [ ] Add `tool_schemas: Vec<ToolSchema>` and `system_prompt: String` fields.
  - [ ] Replace the complex closures with a `tokio::select!` channel loop.
  - [ ] Manage a simple `Vec<Message>` instead of `AgentState`.
  - [ ] Implement `CoreInput::SetEphemeralState` logic to inject text at the end of the Provider request.
  - [ ] Track pending tool `call_id`s per provider turn and block the next provider request until all results for the turn are received (or cancellation occurs).

## Step 4: Implement `AgentRunner` (The Orchestrator) - NOT STARTED
Create the new side-effect and concurrency manager.
- **Target File:** `src/core/agent/runner.rs` (new file).
- **Tasks:**
  - [ ] Create `AgentRunner` struct that owns `AgentCore` (initialized with schemas/prompts), `ToolExecutor`, `ApprovalPolicy`, and `RunnerState`.
  - [ ] Implement the main loop multiplexing `RunnerInput` and `CoreOutput`.
  - [ ] Implement tool execution: evaluate approvals, route questions to UI, run blocking tools inline, and run non-blocking tools concurrently.
  - [ ] Accept out-of-order completion of non-blocking tools and correlate by `call_id`.
  - [ ] Apply `StatePatch` values sequentially on completion; emit `RunnerOutput::StateUpdated` after each effective state change.
  - [ ] Upon each tool completion, send `CoreInput::ToolResult` to the Core.
  - [ ] Format state for the LLM and send `SetEphemeralState` to the Core after state changes.
  - [ ] Implement cancellation that cancels provider stream, all in-flight tool futures, and pending question/approval waits.
  - [ ] Emit `RunnerOutput::StateUpdated` to the UI whenever the state changes.

## Step 5: Adapt Session Persistence - PARTIALLY COMPLETE
Move session writing out of the core and into a pure observer.
- **Target Files:** `src/cli/chat/agent_run.rs`, `src/cli/tui/app.rs`, session store/types modules.
- **Tasks:**
  - [ ] Remove `session.append(...)` calls from the core logic.
  - [ ] Create a mechanism in the UI/CLI layer that listens to `RunnerOutput` and writes corresponding events to `SessionSink`.
  - [x] Persist a serialized `RunnerState` snapshot in session metadata.
  - [ ] Update the snapshot after each applied `StatePatch` and at turn completion.
  - [ ] On resume, load snapshot first, then continue from new incoming events.

## Step 6: Update UI Adapters (CLI & TUI) - NOT STARTED
Wire the existing frontends to the new event-driven runner.
- **Target Files:** `src/cli/chat/agent_run.rs`, `src/cli/tui/app.rs`.
- **Tasks:**
  - [ ] Swap the old generic callback logic for a channel-based listener.
  - [ ] In the TUI, listen for `RunnerOutput::StateUpdated` and bind it directly to the UI's render state (e.g. `self.todo_items = state.todo_items`).
  - [ ] Ensure cancel UX always receives the terminal turn signal and exits processing state cleanly.

## Step 7: Cleanup and Verification - PARTIALLY COMPLETE
- **Tasks:**
  - [ ] Delete `src/core/agent/state.rs` (replaced by `RunnerState` and ephemeral text injection).
  - [ ] Remove deprecated traits (`AgentEvents`, `NoopEvents`).
  - [ ] Add tests for out-of-order non-blocking tool completion with deterministic `call_id` correlation.
  - [ ] Add tests for cancellation while tools are in-flight (no leaks, no deadlock, deterministic terminal signal).
  - [ ] Add tests for snapshot persistence/resume of `RunnerState`.
  - [ ] Add bounded-channel/backpressure tests (delta coalescing allowed, control events not dropped).
  - [x] Run the test suite (`cargo test`) and fix any mock/adapter breakages.
  - [x] Run `cargo clippy -- -D warnings`.
