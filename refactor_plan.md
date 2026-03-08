# Agent Loop Refactor Plan

This document outlines the step-by-step plan to refactor the runtime into a two-layered, event-driven architecture utilizing a `RunnerState`.

## Step 1: Define Domain Types (Events/Channels/State) - COMPLETE
Create the explicit data flow types for both layers.
- **Target File:** `src/core/agent/types.rs` (update existing file).
- **Tasks:**
  - [x] Finalize `RunnerState` (initially `todo_items`, `context_tokens`, plus future typed fields).
  - [x] Finalize `CoreInput` and `CoreOutput` enums (transport-safe error payloads).
  - [x] Finalize `RunnerInput` and `RunnerOutput` enums, including cancellation path.
  - [x] Document protocol invariants: one tool result per `call_id`, and next provider call only after all tool calls in the current turn resolve.

## Step 2: Refactor Tool Interfaces - COMPLETE
Update the tool execution pipeline to treat state as a reducer.
- **Target Files:** `src/core/traits.rs`, `src/tool/mod.rs`, `src/tool/todo.rs`, etc.
- **Tasks:**
  - [x] Introduce a typed `StatePatch`/`StateOp` model.
  - [x] Update `Tool::execute` and `ToolExecutor::execute` to return `ToolResult` + `StatePatch`.
  - [x] Keep patch generation inside tools; keep patch application centralized in the Runner. *(Patch generation remains in tools; patch application is centralized in `runner::apply_tool_outcome` and runner-owned reduction paths.)*
  - [x] Implement `TodoWriteTool` to emit a patch that updates canonical TODO state.

## Step 3: Extract `AgentCore` (The Pure State Machine) - COMPLETE
Strip side-effects and typed state from the current runtime orchestration layer.
- **Target File:** `src/core/agent/core.rs` (extracted from `mod.rs`).
- **Tasks:**
  - [x] Rename `AgentLoop` to `AgentCore`. *(Core runtime orchestration type is now `core::agent::AgentCore`; runtime construction/call sites (including CLI factory and core/agent tests) now use `AgentCore`, and the temporary `AgentLoop` alias has been removed.)*
  - [x] Remove generic bounds for `ToolExecutor`, `ApprovalPolicy`, and `SessionSink`. *(`AgentCore` is provider-only, and `AgentRunner` no longer depends on `SessionSink`.)*
  - [x] Add `tool_schemas: Vec<ToolSchema>` and `system_prompt: String` fields.
  - [x] Replace the complex closures with a `tokio::select!` channel loop. *(Turn orchestration uses `tokio::select!` to multiplex cancellation with live `CoreOutput` delivery in runner execution, and runner input handling uses direct `RunnerInput` multiplexing without a forwarding task. Core remains a provider-facing state machine while runner owns channel-loop orchestration.)*
  - [x] Manage a simple `Vec<Message>` instead of `AgentState`. *(Runtime orchestration now uses direct `Vec<Message>` ownership at the loop boundary; turn-progress (`step`) is maintained as a loop-local counter, and `TurnState` has been removed from shared domain types and runtime APIs (retained only as test-only helper struct inside `runner.rs`). Typed runtime state (`todo_items`, `context_tokens`) remains in `RunnerState` and flows via `RunnerOutput::StateUpdated`.)*
  - [x] Implement `CoreInput::SetEphemeralState` logic to inject text at the end of the Provider request. *(`AgentCore` now stores ephemeral state via `handle_input(CoreInput::SetEphemeralState(..))` and appends it to provider request messages in `request_messages`.)*
  - [x] Track pending tool `call_id`s per provider turn and block the next provider request until all results for the turn are received (or cancellation occurs). *(Pending `call_id` tracking + next-turn guard are enforced in `AgentCore`; acknowledgements/cancellation flow through `AgentCore::handle_input(CoreInput::ToolResult|Cancel)` with coverage in core tests and runner-loop cancellation cleanup (`run_input_loop_cancel_clears_pending_tool_calls`).)*

## Step 4: Implement `AgentRunner` (The Orchestrator) - COMPLETE
Create the new side-effect and concurrency manager.
- **Target File:** `src/core/agent/runner.rs` (new file).
- **Tasks:**
  - [x] Create `AgentRunner` struct that owns `AgentCore` (initialized with schemas/prompts), `ToolExecutor`, `ApprovalPolicy`, and `RunnerState`. *(Expanded `AgentRunner` with wrappers for approvals/tool execution/tool-result recording, migrated blocking/non-blocking flows, approval-required handling, session approval-state restore/check/record logic, tool-approval request construction, per-call dispatch (`handle_tool_call`), per-turn tool-call orchestration (`process_tool_calls`), and typed loop APIs (`execute_turn_with_outputs`, `run_input_loop`). Runner now performs live reduction of streamed `CoreOutput` signals and emits `RunnerOutput` through adapter-owned sinks.)*
  - [x] Implement the main loop multiplexing `RunnerInput` and `CoreOutput`. *(`AgentRunner::run_input_loop` directly multiplexes `RunnerInput` (`Message`/`Cancel`) with in-flight turn execution (no forwarding task), feeds queued messages between turns, and emits `RunnerOutput` in real time (including error outputs). Turn execution multiplexes cancellation with streamed `CoreOutput` delivery via `tokio::select!`, and now consumes stream outputs without redundant post-turn field override bookkeeping.)*
  - [x] Implement tool execution: evaluate approvals, route questions to UI, run blocking tools inline, and run non-blocking tools concurrently.
  - [x] Accept out-of-order completion of non-blocking tools and correlate by `call_id`. *(`process_tool_calls` accepts `FuturesUnordered` completions out of order and records tool outputs by `call_id`; covered by `process_tool_calls_correlates_out_of_order_non_blocking_results_by_call_id`.)*
  - [x] Apply `StatePatch` values sequentially on completion; emit `RunnerOutput::StateUpdated` after each effective state change.
  - [x] Upon each tool completion, send `CoreInput::ToolResult` to the Core. *(`AgentRunner::record_tool_result` now sends `core.handle_input(CoreInput::ToolResult { .. })` for per-call acknowledgement.)*
  - [x] Format state for the LLM and send `SetEphemeralState` to the Core after state changes. *(`AgentRunner::complete_turn` now sends `CoreInput::SetEphemeralState(self.state_for_llm())` before each provider turn.)*
  - [x] Implement cancellation that cancels provider stream, all in-flight tool futures, and pending question/approval waits. *(Cancellable runner APIs (`execute_turn_with_outputs_cancellable`, `process_tool_calls_cancellable`) now gate provider turns, tool execution, and approval/question waits via cooperative cancel `select!`; cancellation clears pending tool-call invariants, emits terminal `RunnerOutput::Cancelled`, and is wired from TUI with timed abort fallback. Coverage includes in-flight non-blocking tools, provider futures, and provider-stream futures.)*
  - [x] Emit `RunnerOutput::StateUpdated` to the UI whenever the state changes.

## Step 5: Adapt Session Persistence - COMPLETE
Move session writing out of the core and into a pure observer.
- **Target Files:** `src/cli/chat/agent_run.rs`, `src/cli/tui/app.rs`, session store/types modules.
- **Tasks:**
  - [x] Remove `session.append(...)` calls from the core logic. *(Message/thinking/tool-call/tool-result/approval/snapshot persistence now flows through the `AgentCore` output observer path; `AgentCore`/`AgentRunner` no longer append session events directly.)*
  - [x] Create a mechanism in the UI/CLI layer that listens to `RunnerOutput` and writes corresponding events to `SessionSink`. *(Interactive TUI, single-prompt CLI, and subagent execution now use `AgentCore::run_with_runner_output_sink_cancellable` with adapter-owned `RunnerOutput` listeners that persist messages/tool lifecycle/thinking/approvals/snapshots to `SessionSink`. Snapshot persistence continues through explicit `RunnerOutput::SnapshotUpdated`. Output-to-observer mapping is centralized in shared `core::agent::apply_runner_output_to_observer` (used by both core adapter path and CLI adapters) with `RunnerOutputObserver::on_error`; duplicate per-mode mappers were removed.)*
  - [x] Persist a serialized `RunnerState` snapshot in session metadata.
  - [x] Update the snapshot after each applied `StatePatch` and at turn completion. *(Runner emits `RunnerOutput::SnapshotUpdated` after tool completion and final turn completion; observer persists snapshot updates.)*
  - [x] On resume, load snapshot first, then continue from new incoming events. *(Current `AgentCore` now prefers snapshot as initial typed state.)*

## Step 6: Update UI Adapters (CLI & TUI) - COMPLETE
Wire the existing frontends to the new event-driven runner.
- **Target Files:** `src/cli/chat/agent_run.rs`, `src/cli/tui/app.rs`.
- **Tasks:**
  - [x] Swap the old generic callback logic for a channel-based listener. *(Runner remains channel-driven (`RunnerInput` + streamed `RunnerOutput`), and interactive TUI/single-prompt/subagent orchestration consume `RunnerOutput` through adapter-owned sinks (including queued-message drain/consume hooks) instead of routing through legacy loop callback wiring. The old `chat::run_single_prompt_with_events`, `AgentCore::run_with_question_tool`, and `AgentCore::run_with_question_tool_cancellable` callback APIs are removed; `AgentCore` no longer stores an event sink field/generic; and `create_agent_loop` now builds `AgentCore<provider, tools, approvals, session>` directly. Observer usage moved from deprecated `AgentEvents` to `RunnerOutputObserver` in `core::agent`; obsolete `TodoItemsChanged`/`ContextUsage` callback-event plumbing is removed in favor of `RunnerStateUpdated`; queue drain/consume methods are removed from observer hooks (queue wiring now lives in explicit adapter sinks); and output mapping logic is de-duplicated around shared `core::agent::apply_runner_output_to_observer`.)*
  - [x] In the TUI, listen for `RunnerOutput::StateUpdated` and bind it directly to the UI's render state (e.g. `self.todo_items = state.todo_items`). *(Added observer hook `on_runner_state_updated`, mapped `RunnerOutput::StateUpdated` to it, introduced `TuiEvent::RunnerStateUpdated`, and wired `ChatApp` to update TODO + context render state directly from `RunnerState`.)*
  - [x] Ensure cancel UX always receives the terminal turn signal and exits processing state cleanly. *(Interrupt uses cooperative cancel signal first (with timed abort fallback), runner-level `RunnerOutput::Cancelled` maps to UI `TuiEvent::Cancelled`, cancellation classification reuses `runner::is_cancellation_error`, and adapter-path coverage verifies a single terminal cancel signal.)*

## Step 7: Cleanup and Verification - COMPLETE
- **Tasks:**
  - [x] Delete `src/core/agent/state.rs` (replaced by `RunnerState` and ephemeral text injection).
  - [x] Remove deprecated traits (`AgentEvents`, `NoopEvents`). *(Removed `NoopEvents` entirely. Removed legacy `AgentRunner::execute_turn(..., events)` callback adapter and introduced `AgentCore::run_with_runner_output_sink_cancellable` to reduce orchestration coupling to callback sinks. Fully removed deprecated `AgentEvents` and replaced usage with `core::agent::RunnerOutputObserver`; switched call sites/tests to unit `()` observer when no UI sink is needed. Trimmed observer hooks by removing deprecated queue drain/consume and redundant state-delta hooks (`on_todo_items_changed`, `on_context_usage`) in favor of explicit adapter queue wiring + `on_runner_state_updated`.)*
  - [x] Add tests for out-of-order non-blocking tool completion with deterministic `call_id` correlation. *(Added `runner::process_tool_calls_correlates_out_of_order_non_blocking_results_by_call_id` to verify `ToolEnd` and tool messages stay correlated by `call_id` when non-blocking completions arrive out of order.)*
  - [x] Add regression coverage for RunnerOutput-based session persistence. *(Added `runner_outputs_persist_tool_call_and_result_events_via_loop_adapter` for tool call/result observer persistence and runner tests `process_tool_calls_emits_snapshot_updated_after_tool_result` + `execute_turn_emits_snapshot_updated_before_turn_complete` for snapshot output emission ordering.)*
  - [x] Add tests for cancellation while tools are in-flight (no leaks, no deadlock, deterministic terminal signal). *(Added `execute_turn_cancellation_stops_inflight_non_blocking_tools_and_clears_pending_calls`, `execute_turn_cancellation_drops_inflight_provider_future`, `execute_turn_cancellation_drops_inflight_provider_stream_future`, and `run_input_loop_cancel_clears_pending_tool_calls` to validate fast cancellation resolution plus drop/cleanup across tool, provider, provider-stream, and runner-loop pending-call cleanup paths.)*
  - [x] Add tests for snapshot persistence/resume of `RunnerState`. *(Added `runner_state_snapshot_round_trips_todo_state_across_runs` to verify snapshot save/load, todo/context token roundtrip, and ephemeral TODO injection on resumed run.)*
  - [x] Add bounded-channel/backpressure tests (delta coalescing allowed, control events not dropped). *(Added `core/agent/output_channel.rs` with bounded-channel tests for delta coalescing and control-event retention, plus `bounded_output_channel_keeps_turn_complete_under_delta_burst` to validate terminal turn signal behavior under high delta volume.)*
  - [x] Run the test suite (`cargo test`) and fix any mock/adapter breakages.
  - [x] Run `cargo clippy -- -D warnings`.
