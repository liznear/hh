# Parallel Sub-Agents Implementation Plan

## Goals

1. Allow the LLM to spawn sub-agents using registered agents where `mode = subagent`.
2. Maintain and render state for all running/completed sub-agents (tree model, sidebar UI).
3. Run sub-agents in parallel in the background.
4. Reuse main agent runtime code for sub-agents (including skill loading).

## Scope and Non-Goals

### In scope

- Runtime orchestration for parallel sub-agents.
- `task` tool for spawning/resuming sub-agents (polling is out of scope for v1).
- Persistent sub-agent lifecycle events.
- TUI state and navigation for parent/sub-agent threads.
- Refactor to share runtime builder between main and sub-agents.

### Out of scope (first release)

- Multi-workspace distributed scheduling.
- Full cancellation/resume of in-flight subprocess handles across process restarts.
- Conflict-free concurrent write merging.

## Current Repository Baseline

- Agent modes already exist: `Primary` and `Subagent` in `src/agent/config.rs`.
- Registry filtering for primary agents exists in `src/agent/registry.rs`.
- Session events already include:
  - `SubAgentStart`
  - `SubAgentProgress`
  - `SubAgentResult`
  in `src/session/types.rs`.
- Core domain types already include `SubAgentCall` and `SubAgentResult` in `src/core/types.rs`.
- `sub_agent_max_depth` already exists in `src/config/settings.rs`.
- Missing pieces:
  - No `task` tool implementation in `src/tool/`.
  - No runtime manager/state store for live sub-agents.
  - No sidebar tree rendering/navigation for sub-agent sessions.
  - No parent/sub-agent orchestration glue in runtime loop.

## Design Principles

- Keep canonical LLM semantics in core domain types.
- Keep provider details out of core state and events.
- Make orchestration additive and reversible.
- Prefer explicit state transitions over implicit behavior.
- Keep sub-agent outputs summarized in parent thread to reduce context pollution.

## V1 Normative Decisions (Lock Before Coding)

To avoid ambiguity during implementation, treat the following as the first-release contract:

1. `task` tool supports **start/resume only** in v1.
   - Start: no `task_id` provided, new child thread is created.
   - Resume: `task_id` provided, existing child thread is continued.
   - Poll/status calls are not part of v1 tool input surface; status is obtained from emitted events and manager/TUI state.
2. Task IDs are generated as UUIDv7 strings and are globally unique.
3. Lifecycle ordering is strict and deterministic:
   - exactly one `SubAgentStart`
   - zero or more `SubAgentProgress`
   - exactly one terminal `SubAgentResult`
   - v1 queue semantics: `SubAgentStart` is emitted when a task is accepted into manager state (queued or running), not only when execution begins.
4. Restart reconciliation is deterministic:
   - any persisted non-terminal child (`Pending`/`Running`) without terminal event is reconstructed as `Failed` with reason `interrupted_by_restart`.
5. Parent context receives child summary only (bounded), while full child transcript/output remains in child session.
6. All new persisted fields are additive with serde defaults for backward compatibility.
7. Child sessions are hidden from the top-level resume-session list.
   - Only parent/root sessions appear in global resume discovery.
   - Child sessions are discoverable only through their parent sub-agent tree/navigation.
8. Resume safety scope:
   - resume by `task_id` is only valid within the current parent session context.
   - implementation resolves child by `(parent_session_id, task_id)` to prevent cross-session collisions.
9. `Cancelled` status is reserved in v1 for internal/system transitions only; user-facing cancellation workflow is out of scope.

## High-Level Architecture

### 1) Runtime orchestration service

Add a `SubagentManager` that owns in-memory execution state and concurrency control.

Responsibilities:

- Track sub-agent metadata by `task_id`.
- Track parent/child relationships and expose a tree view.
- Start sub-agent background tasks.
- Serve internal status/result lookups for replay and TUI state.
- Emit lifecycle events to session store and TUI.
- Reconcile interrupted in-flight tasks on startup/replay.

Suggested shape:

- `SubagentManager` (Arc, shared)
- `SubagentState` map keyed by `task_id`
- parent->children adjacency index for fast tree rendering
- optional join-handle map for live tasks
- bounded work queue + semaphore for parallelism caps

### 2) Task tool contract

Add `task` tool in `src/tool/` and register in `src/tool/registry.rs`.

Initial API:

- Inputs:
  - `description: string`
  - `prompt: string`
  - `subagent_type: string`
  - `task_id?: string` (resume existing child thread)
- Behavior:
  - Validate `subagent_type` exists and `mode == subagent`.
  - Enforce depth and concurrency limits.
  - Spawn asynchronously and return `task_id` immediately.
  - If `task_id` provided, continue existing child session.
- Output:
  - structured JSON: `{ task_id, status, message }`

V1 semantics:

- This API is start/resume only.
- `status` in the output indicates acceptance of request (`queued` or `running`), not final child completion.
- Final child completion is delivered through session events (`SubAgentResult`) and reconstructed manager state.
- `queued` means accepted into manager queue but not yet executing; `running` means worker execution has started.
- Resume lookup must be scoped to current parent session.
- Parent-ingest contract for child outcomes is bounded and structured: `{ task_id, status, summary, error? }`.

Future extension (optional): `wait` flag and bounded polling timeout.

### 3) Shared agent execution path

Refactor `src/cli/chat.rs` to extract reusable runtime creation/execution helpers.

Target outcome:

- Main agent and sub-agents use same execution builder:
  - provider
  - tool registry
  - approval matcher
  - session store
  - system prompt
- Sub-agents can load skills because they reuse existing `ToolRegistry` (`skill` tool already registered).

### 4) Persistence model

Use existing session event types and extend payloads if needed.

Required persisted fields:

- `task_id`
- `parent_task_id` (optional for nested sub-agent tasks)
- `parent_session_id` (required for root-session scoping and discovery filtering)
- `depth`
- `agent_name`
- child `session_id`
- result status/output

Recommended additional persisted metadata:

- `created_at` / `updated_at`
- `failure_reason` (structured, stable enum + freeform detail)
- optional bounded `result_summary`

This must allow complete reconstruction of tree/state when resuming sessions.

Session discovery rule:

- Mark child sessions with parent linkage metadata (for example `parent_session_id` or equivalent) so listing APIs can exclude them from top-level resume lists.
- Resume list shows only root sessions; child sessions are loaded lazily when opening a root session and reconstructing its sub-agent tree.
- Generic session-list APIs should default to root sessions only; child inclusion requires an explicit opt-in flag (for example `include_children=true`).

Event invariants (must hold in persistence and replay):

- A `task_id` has exactly one start event and exactly one terminal result event.
- Progress events are monotonic by per-task sequence number and only valid between start and terminal.
- Replay is idempotent; duplicate events do not create duplicate nodes or transition terminal nodes.

### 5) TUI model

Sidebar should include a `Subagents` section rendered as a tree.

Per-node display:

- agent display name
- status (`running`, `done`, `error`)
- short prompt/result summary

Interaction:

- Click sub-agent node => main panel shows that sub-agent thread.
- While viewing sub-agent thread, input box is disabled and clearly labeled read-only.
- Parent thread remains selectable.

## Detailed Phases

## Phase 0: Prep and seams

1. Introduce orchestration module location:
   - `src/core/agent/subagent_manager.rs` (or `src/core/subagent/`)
2. Define internal state structs and status enum.
3. Add minimal interfaces for:
   - spawn
   - query
   - list tree
4. Wire dependency injection path from CLI runtime setup.

Deliverable: compile-time scaffolding with no behavior change.

## Phase 1: Task tool and spawn path

1. Implement `TaskTool` in `src/tool/task.rs`.
2. Register schema in tool registry.
3. Add capability mapping and permissions:
   - new capability `task`
   - settings field and matcher handling
4. On execute:
   - validate agent mode
   - validate depth <= `sub_agent_max_depth`
   - spawn background run via `tokio::spawn`
   - emit `SubAgentStart`
   - return `task_id`
5. Add API contract tests for `task` JSON schema and backward compatibility snapshots.

Deliverable: parent can request sub-agent spawn through tool call.

## Phase 2: Parallel execution and lifecycle

1. Implement worker execution function:
   - accepts parent context and child config
   - creates child session store
   - runs shared agent loop
2. Emit lifecycle events:
   - `SubAgentProgress` for meaningful milestones
   - `SubAgentResult` at completion
3. Ensure parent loop is non-blocking while children run.
4. Enforce optional max parallel children.
5. Define and enforce output bounds:
   - max persisted bytes per progress/result payload
   - truncation marker for oversized payloads
   - child-to-parent summary size cap
6. Define progress event rate limiting/coalescing to avoid persistence/TUI spam.

Deliverable: multiple children can run concurrently and report status.

## Phase 3: Session replay and state reconstruction

1. Extend replay logic in `handle_session_selection` (`src/cli/chat.rs`) to ingest sub-agent events.
2. Rebuild tree from persisted events on resume.
3. Preserve compatibility with old sessions lacking sub-agent events.
4. Reconcile interrupted tasks as deterministic failures (`interrupted_by_restart`).
5. Update resume-session query/list logic to return root sessions only (exclude child sessions).

Deliverable: restart/resume restores sub-agent topology and statuses.

## Phase 4: TUI rendering and navigation

1. Extend `ChatApp` state (`src/cli/tui/app.rs`) with:
   - sub-agent tree items
   - selected thread id
   - view mode (parent vs selected child)
2. Render sidebar section in `src/cli/tui/ui.rs`.
3. Add click hit-testing for sub-agent rows in `src/cli/chat.rs` mouse handlers.
4. Display selected thread messages in main panel.
5. Disable input UI and submission while in child view.

Deliverable: UX requirement achieved (click child -> read-only thread view).

## Phase 5: Hardening and guardrails

1. Prevent recursive runaway:
   - depth check
   - max parallel check
2. Ensure deterministic failure behavior:
   - child failure does not crash parent
   - structured error result surfaced
3. Respect permission policy for `task` and child tools.
4. Sanitize child outputs before storing/rendering.
5. Add contention/race protections:
   - never hold manager write locks while awaiting I/O
   - idempotent terminal transition helper
   - safe handling for duplicate completion signals

Deliverable: safe behavior under load and failure conditions.

## Data Model Proposal

## Runtime state

```rust
enum SubagentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

struct SubagentNode {
    task_id: String,
    parent_task_id: Option<String>,
    agent_name: String,
    prompt: String,
    depth: usize,
    session_id: String,
    status: SubagentStatus,
    started_at: u64,
    updated_at: u64,
    summary: Option<String>,
    error: Option<String>,
}
```

## State Transition Rules

Allowed transitions:

- `Pending -> Running | Failed | Cancelled`
- `Running -> Completed | Failed | Cancelled`
- terminal states are immutable (`Completed`, `Failed`, `Cancelled`)

Operational rules:

- Start event writes `Pending`; `Pending` may persist while queued, and transitions to `Running` when worker execution actually begins.
- Exactly one terminal transition is accepted; subsequent terminal updates are ignored and logged.
- Terminal transition always updates `updated_at` and emits exactly one `SubAgentResult`.
- Progress events carry per-task sequence number (`seq`) assigned by manager at emit time.

## Event payloads

If needed, enrich existing `SessionEvent` sub-agent variants with:

- `agent_name`
- `session_id`
- `status`
- `seq` (for progress ordering)
- optional `summary`

Failure reason enum (v1 recommended baseline):

- `tool_error`
- `approval_denied`
- `runtime_error`
- `interrupted_by_restart`
- `unknown`

Do this additively with serde defaults to keep backward compatibility.

## Permissions and Policy

Add a first-class `task` permission capability.

Recommended defaults:

- Main build agent: `task = allow`.
- Main plan agent: `task = allow` (read-focused subagents still useful).
- Subagents: tool permissions inherited from selected subagent config.

Optional advanced policy (later): per-agent task allowlist with glob matching.

Inheritance and nesting rules:

- Parent must be allowed to call `task`.
- Spawned child uses its own configured capabilities (not implicit full inheritance from parent).
- Nested spawn requires both:
  - child agent capability allows `task`
  - depth check passes (`depth < sub_agent_max_depth`)
- Child execution uses the active approval policy snapshot resolved at child start (explicit, reproducible behavior).

## UI Behavior Specification

Sidebar `Subagents` section:

- Hierarchical indentation for depth.
- Status marker:
  - `...` running
  - `ok` completed
  - `err` failed
- Highlight currently selected node.

Main panel behavior:

- Parent selected: normal chat behavior.
- Child selected:
  - render child session transcript
  - disable input editing/submission
  - status line indicates read-only child thread

Tree ordering behavior:

- Siblings are sorted by `started_at`, then `task_id` for deterministic replay/UI stability.

Keyboard extension (optional in first pass):

- cycle parent/child threads via keybinds.

## Testing Plan

## Unit tests

- `task` tool validates unknown/non-subagent mode.
- depth and max-parallel checks.
- manager tree insert/update semantics.
- permission decision for `task` capability.
- state transition idempotency and duplicate terminal events.
- restart reconciliation (`Running` -> `Failed(interrupted_by_restart)`).
- resume lookup is scoped to `(parent_session_id, task_id)`.
- progress sequence ordering is monotonic even under concurrent emits.

## Integration tests

- parent spawns N children in parallel and receives all results.
- one child fails, others succeed, parent remains healthy.
- resumed session reconstructs tree/status correctly.
- parent exits/restarts with in-flight children, replay marks interrupted tasks deterministically.
- high-concurrency completion burst does not deadlock or corrupt tree.
- queued-to-running transition emits expected status semantics.
- progress rate limiting/coalescing prevents unbounded event growth.

## TUI tests

- sidebar renders subagent section and statuses.
- click mapping selects expected child thread.
- input disabled in child view and enabled in parent view.
- resume-session list excludes child sessions while parent thread still reconstructs and displays them.

## Rollout Strategy

1. Ship behind config flag (recommended):
   - `agent.parallel_subagents = false` default initially.
   - `agent.max_parallel_subagents = <small default>` (global process-wide cap)
   - `agent.max_parallel_subagents_per_parent = <small default>` (fairness cap per parent session)
2. Enable in dev builds and gather feedback.
3. Promote to default after stability and UX validation.

Suggested observability gates before default-on:

- spawn success rate
- child failure rate by reason
- queue depth and wait time percentiles
- replay reconciliation count

## Risks and Mitigations

- Context pollution in parent thread:
  - Mitigation: store full child output in child session; pass compact summaries to parent.
- Parallel write conflicts:
  - Mitigation: recommend read-focused subagents first; keep write-heavy child agents opt-in.
- Resource spikes with many children:
  - Mitigation: max parallel cap and bounded queue.
- Inconsistent replay state:
  - Mitigation: event-driven reconstruction only from persisted lifecycle events.

## Milestone Checklist

- [ ] Runtime manager module and interfaces.
- [ ] `task` tool implemented and registered.
- [ ] Shared runtime builder used by main + subagents.
- [ ] Background parallel execution path working.
- [ ] Session events persisted for full lifecycle.
- [ ] Resume reconstructs tree/state.
- [ ] Sidebar tree rendering + click navigation.
- [ ] Child thread read-only view with disabled input.
- [ ] Unit + integration + TUI tests passing.
- [ ] Documentation updates for config and usage.
- [ ] Event ordering and transition invariants documented and enforced.
- [ ] Restart reconciliation behavior implemented and tested.
- [ ] Payload bounding/truncation policy implemented and tested.

## Notes from External Product Patterns

The plan aligns with current Codex/OpenCode multi-agent patterns:

- role-based sub-agent spawning
- parent orchestration with parallel children
- depth/thread guardrails
- thread/session navigation for child workflows
- summary-first parent context strategy

This gives a practical path to feature parity while preserving this repository's existing architecture boundaries.
