# Subagent and Task Tool Redesign

## Goals

- Make every rendered `task` row deterministically clickable.
- Ensure clicking a row always opens the correct child subagent session.
- Remove child-session resume complexity.
- Keep child sessions inspectable in filesystem and explicit in metadata.
- Preserve provider-agnostic core types; keep wiring details in runtime/adapters.

## Non-Goals

- No migration of historical sessions to new IDs.
- No cross-parent session sharing.
- No interactive resume/reattach flow for child sessions.

## Problem Summary

Current behavior derives row-to-session mapping from presentation/state heuristics (args matching, rank/order fallback, line hit tests against rendered text). Under parallel subagents, this causes:

- some rows not clickable,
- wrong row -> wrong session,
- transiently empty view that looks broken.

Root issue: missing stable identity chain from parent tool call to child session.

## Revised Identity Model

Use a single canonical identity chain:

- `call_id` (from `ToolCall.id`) is the root identity.
- `task_id = call_id`.
- `child_session_id = "{parent_session_id}-{call_id}"`.

Implications:

- A task row is keyed by `call_id`.
- Subagent manager state is keyed by `task_id` (= `call_id`).
- Child transcript lookup is keyed by `child_session_id`.

No text/rank matching is needed.

## Session Storage Design

Child session files use deterministic names for inspectability:

- session log: `{root}/{workspace}/{parent_session_id}-{call_id}.jsonl`
- metadata: `{root}/{workspace}/{parent_session_id}-{call_id}.meta.json`

Metadata remains canonical for linkage (filename is auxiliary):

- `id`: child session id
- `title`: child session title
- `parent_session_id`: parent session id
- `is_child_session: true`
- `parent_tool_call_id`: parent tool call id (`call_id`)
- optional: `subagent_type`, `depth`

`SessionMetadata` additions should be optional with defaults for backward compatibility.

## Task Tool Contract

The `task` tool must emit structured outputs that always include identity fields:

- `task_id` (same as `call_id`)
- `session_id` (child session id)
- `status` (`queued|running|done|error|cancelled` for UI aliasing)
- `name`, `agent_name`, `prompt`, `depth`
- `started_at`, optional `finished_at`
- optional `summary`, optional `error`

`session_id` should be present from first accepted result/update path, not only terminal output.

## Subagent Manager Design

Subagent manager remains lifecycle authority:

- statuses: `pending -> running -> terminal`
- node keyed by `task_id`

Node fields (authoritative):

- `task_id` (`call_id`)
- `parent_session_id`
- `session_id` (child)
- `name`, `agent_name`, `prompt`, `depth`
- `status`, `started_at`, `updated_at`
- optional `summary`, optional `error`, optional failure reason

No resume branches for child sessions.

## Event Wiring Changes

### Tool Events

Extend TUI event payloads to include tool call id:

- `ToolStart { call_id, name, args }`
- `ToolEnd { call_id, name, result }`

This prevents ambiguous matching when multiple same-name tools run concurrently.

### Subagent Lifecycle Updates

Emit/propagate fields needed for deterministic binding:

- `task_id` (= `call_id`)
- `session_id`
- `status` + timestamps + summary/error

## TUI State Model

Add explicit task row state, independent of rendered transcript text:

- `task_rows_by_call_id: HashMap<String, TaskRowView>`

`TaskRowView` fields:

- `call_id`
- `task_id`
- `session_id` (optional until available)
- label fields (`name`, `agent_name`)
- `status`, `started_at`, optional `finished_at`

Rendered message rows should carry stable interaction metadata for hit testing:

- visual line range -> `call_id`

Do not infer clickable targets from message strings.

## Click Behavior

On click:

1. map click to `call_id` via render metadata,
2. lookup `TaskRowView` by `call_id`,
3. open `session_id` if present,
4. if no session content yet, show explicit waiting placeholder.

All task rows display `(click to open)` consistently.

## Subagent Session View Behavior

- Input box hidden.
- Sidebar header chain:
  - parent session title
  - `-> Subagent Session: <title>`
- While subagent is non-terminal: bottom running footer with animated progress + duration + `esc back to main agent`.
- Once terminal: show standard footer style (`Agent  Provider Model  Duration`) and stop animation.
- While open, periodically refresh child transcript from child session store.
- Main-session events must continue updating parent state, but never mutate currently displayed child transcript.

## Concurrency and Queueing

- `agent.max_parallel_subagents` limits concurrent execution.
- Excess tasks are queued (`pending`) but still clickable via deterministic identity.
- Queued tasks may initially show placeholder until first child events exist.

## Backward Compatibility

- Existing sessions without new metadata fields remain readable.
- If old task rows lack `call_id` mapping metadata, fallback rendering remains view-only.
- New runs always use deterministic mapping path.

## Migration Plan

1. Add metadata fields to `SessionMetadata` as optional.
2. Plumb `call_id` into `TuiEvent::ToolStart/ToolEnd`.
3. Make `task_id = call_id` in subagent request path.
4. Generate deterministic child session id from parent session + call id.
5. Replace heuristic click mapping with `call_id` mapping.
6. Remove args/rank fallback logic.
7. Remove child resume-only branches.

## Test Plan

Unit tests:

- parallel 3-task run with identical agent type; each row maps to unique child session.
- clickability for pending/running/completed rows.
- task completion updates exact row by `call_id`.
- deterministic child session id generation.

Integration tests:

- queue scenario (`max_parallel_subagents=2`, launch 3): third row opens waiting state then auto-populates.
- subagent view while parent keeps emitting events: no transcript contamination.
- terminal footer transition from running animation to static footer.

Debug workflow:

- use `hh run ... --debug <dir>` and inspect frame progression for click/row binding.

## Risks and Mitigations

- Risk: partial rollout where some events have `call_id` and others do not.
  - Mitigation: gate deterministic click mapping on presence of `call_id`; keep read-only fallback for legacy.
- Risk: deterministic child id collision (unlikely).
  - Mitigation: include full `call_id` (UUID-like) and parent session id.
- Risk: metadata drift if filename and metadata disagree.
  - Mitigation: metadata is source of truth; filename is advisory.

## Acceptance Criteria

- Every visible `task` row is clickable from start.
- Clicking a row always opens its own subagent session.
- No row opens another row's child session under parallel load.
- Queued child sessions show explicit waiting state, then populate.
- Subagent running footer animates only while running and becomes standard footer when terminal.
