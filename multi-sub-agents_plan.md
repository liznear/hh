# Parallel Sub-Agents Implementation Plan

## Goals

1. Allow the LLM to spawn sub-agents using registered agents where `mode = subagent`.
2. Maintain and render state for all running/completed sub-agents (tree model, sidebar UI).
3. Run sub-agents in parallel in the background.
4. Reuse main agent runtime code for sub-agents (including skill loading).

## Scope and Non-Goals

### In scope

- Runtime orchestration for parallel sub-agents.
- `task` tool for spawning/polling sub-agents.
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

## High-Level Architecture

### 1) Runtime orchestration service

Add a `SubagentManager` that owns in-memory execution state and concurrency control.

Responsibilities:

- Track sub-agent metadata by `task_id`.
- Track parent/child relationships and expose a tree view.
- Start sub-agent background tasks.
- Accept status/poll/result requests.
- Emit lifecycle events to session store and TUI.

Suggested shape:

- `SubagentManager` (Arc, shared)
- `SubagentState` map keyed by `task_id`
- parent->children adjacency index for fast tree rendering
- optional join-handle map for live tasks

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
- `parent_id`
- `depth`
- `agent_name`
- child `session_id`
- result status/output

This must allow complete reconstruction of tree/state when resuming sessions.

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

Deliverable: multiple children can run concurrently and report status.

## Phase 3: Session replay and state reconstruction

1. Extend replay logic in `handle_session_selection` (`src/cli/chat.rs`) to ingest sub-agent events.
2. Rebuild tree from persisted events on resume.
3. Preserve compatibility with old sessions lacking sub-agent events.

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

## Event payloads

If needed, enrich existing `SessionEvent` sub-agent variants with:

- `agent_name`
- `session_id`
- `status`
- optional `summary`

Do this additively with serde defaults to keep backward compatibility.

## Permissions and Policy

Add a first-class `task` permission capability.

Recommended defaults:

- Main build agent: `task = allow`.
- Main plan agent: `task = allow` (read-focused subagents still useful).
- Subagents: tool permissions inherited from selected subagent config.

Optional advanced policy (later): per-agent task allowlist with glob matching.

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

Keyboard extension (optional in first pass):

- cycle parent/child threads via keybinds.

## Testing Plan

## Unit tests

- `task` tool validates unknown/non-subagent mode.
- depth and max-parallel checks.
- manager tree insert/update semantics.
- permission decision for `task` capability.

## Integration tests

- parent spawns N children in parallel and receives all results.
- one child fails, others succeed, parent remains healthy.
- resumed session reconstructs tree/status correctly.

## TUI tests

- sidebar renders subagent section and statuses.
- click mapping selects expected child thread.
- input disabled in child view and enabled in parent view.

## Rollout Strategy

1. Ship behind config flag (recommended):
   - `agent.parallel_subagents = false` default initially.
2. Enable in dev builds and gather feedback.
3. Promote to default after stability and UX validation.

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

## Notes from External Product Patterns

The plan aligns with current Codex/OpenCode multi-agent patterns:

- role-based sub-agent spawning
- parent orchestration with parallel children
- depth/thread guardrails
- thread/session navigation for child workflows
- summary-first parent context strategy

This gives a practical path to feature parity while preserving this repository's existing architecture boundaries.
