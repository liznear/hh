# Interaction Architecture and Implementation Plan

## Goal

Design and implement an interaction-capable agent runtime that can:

1. pause for user-facing dialogs (question tool),
2. accept and apply user steering messages while a run is in progress,
3. enforce approval gates for sensitive tools (for example, bash).

The design should preserve correctness, explicit control flow, and debuggability.

Reference docs:

- `docs/run_id.md` defines run identity semantics, lifecycle, and correlation rules.

Implementation constraint: deliver features strictly in sequence with no interleaving feature work:

1. Question tool,
2. Mid-run user steering,
3. Tool approvals.

## Design Overview

### 1) Runtime model: sequential loop with shared steering queue

Keep the core agent loop sequential. Add a thread-safe per-run steering queue shared between `AgentRunner` and TUI.

The loop remains explicit:

- model call -> optional tool execution -> queue drain -> next turn.

### 2) Steering integration: 3-step turn boundary design

Steering is applied with a deterministic 3-step pattern:

1. enqueue steering messages into a per-run queue while run is active,
2. when a turn completes, execute tool calls if present and update message history,
3. drain all queued steering messages into conversation history before the next turn.

This keeps steering behavior simple, inspectable, and deterministic.

Queued-message UI behavior:

- TUI stores busy-time steering submissions in a runtime-only queued list.
- Runner emits `EventTypeMessage` when queued steering is drained into transcript history.
- TUI clears the whole queued list on the first user-role message event received while busy.
- This depends on the invariant that runner drains all queued steering messages as a single batch at turn boundary.

### 3) Generic interaction abstraction

Introduce a generic interaction object, used by question dialogs and approvals.

Suggested shape:

```ts
type InteractionKind = "question" | "approval" | "confirm";

type InteractionRequest = {
  interaction_id: string;
  run_id: string;
  tool_call_id?: string;
  kind: InteractionKind;
  title: string;
  content?: string;
  content_type?: string;
  options: Array<{ id: string; title: string; description: string }>;
  allow_custom_option: boolean;
  metadata?: Record<string, unknown>; // policy info, risk tags, etc.
  created_at: string;
  expires_at?: string;
};

type InteractionResponse = {
  interaction_id: string;
  run_id: string;
  selected_option_id?: string;
  custom_text?: string;
  submitted_at: string;
};
```

### 4) Interaction manager

Add an `InteractionManager` in the runner:

- create and register pending interactions,
- emit `interaction_requested`,
- wait on a resolver/future keyed by `interaction_id`,
- accept and validate `interaction_responded`,
- resolve once (idempotent),
- timeout/expire stale interactions.

### 5) Steering queue in context

Add a queue object to run context and expose enqueue API from runner.

- queue item: message content, timestamp, monotonically increasing `event_seq`, run correlation,
- queue ownership: created by `AgentRunner` at run start, destroyed at run end,
- queue semantics: FIFO, drain-all at turn boundary.

### 6) Turn-boundary drain model

Drain steering queue at deterministic points only:

- after tool execution path in a turn,
- after non-tool turn completion,
- and once more after turn-end callbacks to avoid late-enqueue drop.

Do not preempt mid-stream or mid-tool execution.

### 7) Policy gate for tools

Before executing a tool call, run tool policy evaluation:

`policyDecision = evaluateToolPolicy(tool_name, args, context)`

Decisions:

- `allow`
- `deny`
- `require_approval` (creates approval interaction)

### 8) Event schema and observability

Standardize events with correlation fields:

- `run_id`, `turn_id`, `tool_call_id`, `interaction_id`, `event_seq`, timestamps.

Minimum events:

- `run_started`, `run_checkpoint`, `run_completed`, `run_failed`
- `interaction_requested`, `interaction_responded`, `interaction_expired`
- `tool_call_started`, `tool_call_allowed`, `tool_call_denied`, `tool_call_completed`
- `user_message_received`
- `message` (including drained steering messages)

### 9) Safety and failure handling

- Unknown/expired interaction response: reject with explicit error event.
- Duplicate responses: first wins, subsequent ignored and logged.
- Runner restart with pending interactions: restore from persisted state or fail run explicitly.
- In-flight cancellation: deterministic cancellation path (no half-applied state).

## Implementation Roadmap (Sequential, Non-Interleaved)

This roadmap intentionally avoids parallel feature development across question, steering, and approvals.
Only complete shared runtime primitives and the current feature wave before starting the next one.

Pre-Wave 2 refactor requirement:

- Before implementing steering queue behavior, refactor baseline message submission to a single append path: transcript messages are appended from runner events only (including the initial user message for a new run).
- TUI should stop directly appending submitted user messages to persisted turn items.
- Runner should emit `EventTypeMessage` for the initial user prompt at run start.
- This keeps one authoritative transcript mutation path and reduces steering integration complexity.

### Wave 0 - Shared primitives required for Question Tool only

Scope rule: implement only the minimum shared runtime needed for question interactions. Do not add steering or approval logic in this wave.

1. Add event envelopes with correlation IDs (`run_id`, `tool_call_id`, `interaction_id`, timestamps).
2. Implement `InteractionRequest`/`InteractionResponse` schemas and validators.
3. Implement `InteractionManager` (register, wait, resolve once, timeout, cleanup).
4. Add external event ingress for interaction responses only.
5. Add tests for interaction lifecycle edge cases (duplicate, unknown, expired responses).

Exit criteria:

- A synthetic interaction can pause/resume the run deterministically.
- Duplicate/unknown interaction responses are handled safely.
- No steering or approval code paths exist yet.

### Wave 1 - Question Tool end-to-end

Scope rule: only question tool functionality and hardening. No user steering inbox behavior. No tool policy gates.

1. Implement question tool schema mapping into generic interactions.
2. Emit `tool_call_started` and `interaction_requested` for question prompts.
3. Implement TUI dialog rendering and input behavior per spec.
4. Normalize responses and return deterministic tool results to the model loop.
5. Add comprehensive tests (unit, integration, TUI behavior).
6. Add question-specific metrics and runbook notes.

Exit criteria:

- Question tool is fully functional and stable in production-like runs.
- Regressions on baseline loop/tool behavior are cleared.
- Feature flag for question tool can be independently toggled.

### Wave 2 - Mid-run user steering

Scope rule: only steering behavior. Reuse existing interaction plumbing; do not add approval policy logic in this wave.

1. Refactor baseline submit workflow to runner-authoritative `EventTypeMessage` append path.
2. Add steering message queue in `AgentRunner.State` and pass it via `Context`.
3. Add `AgentRunner.SubmitSteeringMessage` for TUI to submit messages.
4. On turn completion, execute tool calls if any, then drain queue into conversation history.
5. Add turn-end post-callback drain check to avoid late-enqueue loss.
6. Add concurrency safeguards (FIFO ordering, idempotent enqueue validation, terminal-run rejection).
7. Add logs/events for enqueue and drained steering message actions.
8. Add tests for steering during streaming/tool/waiting states and turn-end timing.
9. Emit `EventTypeMessage` for drained steering.

Exit criteria:

- User steering works reliably without destabilizing question flow.
- Ordering guarantees are validated under concurrent event loads.
- Feature flag for steering can be independently toggled.

### Wave 3 - Tool approvals

Scope rule: only approval gating and policy. Do not change steering semantics except compatibility fixes.

1. Implement policy evaluator interface and default policies.
2. Add pre-tool policy check hook.
3. Implement approval interactions (`allow_once`, `allow_for_run`, `deny`).
4. Add approval decision caching scoped to run.
5. Add audit logs, redaction, and prompt rate limiting.
6. Add allow/deny/adversarial tests and operational docs.

Exit criteria:

- Approval gating works end-to-end for sensitive tools.
- Deny paths are safe and explicit.
- Feature flag for approvals can be independently toggled.

### Wave 4 - Final hardening and rollout

1. Add durability for pending interactions if required by runtime SLOs.
2. Add dashboards/alerts for interaction latency and failure rates.
3. Execute staged rollout: question -> steering -> approvals.
4. Run rollback drills for each feature flag.
5. Finalize migration and on-call runbooks.

## Progress Checklist (Markdown Todos)

Use this section as the single source of truth for implementation progress.
Mark items as done by changing `- [ ]` to `- [x]`.

### Wave 0 - Shared primitives (Question only)

- [x] Add event envelope fields (`run_id`, `tool_call_id`, `interaction_id`, timestamps).
- [x] Implement `InteractionRequest`/`InteractionResponse` schemas and validation.
- [x] Implement `InteractionManager` register/wait/resolve/timeout lifecycle.
- [x] Add external ingress for interaction responses.
- [x] Add tests for duplicate/unknown/expired responses.
- [x] Confirm Wave 0 exit criteria.

### Wave 1 - Question tool

- [x] Implement question tool input schema and mapping.
- [x] Emit `tool_call_started` and `interaction_requested` events.
- [x] Render question tool line as `Question: "<question title>"`.
- [x] Render dialog options + optional custom answer path.
- [x] Implement enter-to-select and enter-to-submit behavior.
- [x] Normalize answer payload to deterministic tool result shape.
- [x] Add unit/integration/TUI behavior tests.
- [x] Add question-specific metrics and diagnostics.
- [ ] Confirm Wave 1 exit criteria.

### Wave 2 - Mid-run steering

- [x] Refactor normal submit flow to append user messages from runner `EventTypeMessage` only.
- [ ] Add per-run steering queue to `AgentRunner` and `Context`.
- [ ] Add enqueue API for `user_message_received` with ordered sequence IDs.
- [ ] Drain queue after each completed turn before next turn.
- [ ] Add turn-end post-callback drain check.
- [ ] Add explicit conversation append path for drained steering messages.
- [ ] Add ordering/idempotency/terminal-run safeguards.
- [ ] Add logs for steering enqueue/drain lifecycle.
- [ ] Add streaming/tool/wait-state steering tests.
- [ ] Emit `EventTypeMessage` for drained steering and steering latency metrics.
- [ ] Confirm Wave 2 exit criteria.

### Wave 3 - Tool approvals

- [ ] Implement policy evaluator interface and default decision rules.
- [ ] Add pre-tool policy check hook.
- [ ] Implement approval interaction flow (`allow_once`, `allow_for_run`, `deny`).
- [ ] Add run-scoped approval cache with invalidation.
- [ ] Add audit events and sensitive-field redaction.
- [ ] Add rate limiting for repeated approval prompts.
- [ ] Add allow/deny/adversarial tests.
- [ ] Confirm Wave 3 exit criteria.

### Wave 4 - Hardening and rollout

- [ ] Add durability strategy for pending interactions.
- [ ] Add dashboards and alerts for interaction/steering/approval health.
- [ ] Execute staged rollout (question -> steering -> approvals).
- [ ] Run rollback drills per feature flag.
- [ ] Finalize operator runbooks and migration notes.
- [ ] Confirm Wave 4 exit criteria.

---

## Section 1: Implement Question Tool on Top of the New Design

### How the design enables it

Question tool becomes a specific `InteractionKind = "question"` flow:

1. Tool is invoked in `running` state.
2. Runner emits `tool_call_started`.
3. Tool creates `InteractionRequest` with question title/options.
4. Runner transitions to `waiting_for_interaction`.
5. TUI renders dialog from `interaction_requested`.
6. User answers; TUI emits `interaction_responded`.
7. Runner validates and resolves pending interaction.
8. Tool returns normalized answer to model and run resumes.

### Detailed task breakdown

1. Tool contract and validation
- Define question tool input schema (`question`, `options`, `allow_custom_option`).
- Enforce required fields and non-empty constraints.
- Keep `content_type` passthrough for future rendering.

2. Tool execution path
- Map question tool input into generic `InteractionRequest`.
- Generate unique `interaction_id` and attach `tool_call_id`.
- Suspend tool completion until `InteractionResponse` resolves.

3. TUI rendering and UX
- Render tool line: `Question: "<question title>"`.
- Render numbered options and optional custom answer slot.
- Support enter-to-select and enter-to-submit for custom input.

4. Answer normalization
- Convert response to deterministic tool result shape.
- Return either selected option metadata or custom text.
- Reject empty custom input with explicit validation message.

5. Testing
- Unit tests: schema checks, response normalization, duplicate response handling.
- Integration tests: tool call start -> interaction requested -> response -> loop resume.
- UX tests: custom input behavior and keyboard interaction.

6. Observability
- Emit question-specific metrics (time to answer, validation failures).
- Add logs keyed by `interaction_id` and `tool_call_id`.

---

## Section 2: Implement Mid-Run User Steering

### How the design enables it

Steering uses a shared queue. New user messages are enqueued while a run is active, then drained into conversation history at deterministic turn boundaries.
The loop stays sequential and does not require mid-stream preemption.

Steering flow:

1. User sends a new message during streaming/execution.
2. Runner enqueues it into the per-run queue with sequence number.
3. Current turn completes (including tool execution if any).
4. Loop drains queued messages and appends them.
5. Runner emits `EventTypeMessage` for drained steering messages.
6. TUI clears queued runtime list on first user-role message event while busy.
7. Next turn starts with updated context.

### Detailed task breakdown

1. Inbound API and event model
- Add endpoint/method for `user_message_received` while run is active.
- Assign monotonic `event_seq` and timestamp.
- Validate payload size and content constraints.

0. Baseline submit refactor (required before steering)
- Move initial user message append to runner event emission path.
- Ensure TUI no longer directly appends submitted user messages to persisted turn items.
- Verify transcript rendering is unchanged after refactor.

2. Turn boundary processing
- Drain all queued steering messages after each completed turn.
- Ensure tool calls are executed before draining queue for that turn.
- Add post-turn callback drain check so late enqueues are not dropped.

3. Conversation state mutation
- Add explicit append path for steering messages.
- Ensure no hidden rewriting of prior model/tool history.

4. TUI queued rendering
- Store queued steering submissions in runtime-only state while run is busy.
- Render queued list after current turn items with `Queued` badge.
- Clear whole queued list on first user-role message event while busy.
- Document invariant: runner drains all queued steering as one batch.

5. Concurrency and correctness
- Handle multiple steering messages in order.
- Handle steering while waiting for interaction.
- Reject enqueue for terminal/non-active runs.
- Prevent late-enqueue loss around turn-end boundaries.

6. Testing
- Integration tests for steering during streaming and tool phases.
- Property tests for ordering/idempotency.
- Failure tests for malformed/late events.

7. UX feedback
- Use drained steering `EventTypeMessage` as UI confirmation that steering took effect.
- Optionally show "applied at turn boundary" indicator.

---

## Section 3: Implement Tool Approvals (Future-Proofed)

### How the design enables it

Approvals reuse the same interaction framework with `InteractionKind = "approval"`.
Instead of direct tool execution, risky calls pass through policy gating first.

Approval flow:

1. Model requests tool call (for example, bash).
2. Policy evaluator marks it `require_approval`.
3. Runner emits approval interaction with risk/context metadata.
4. User chooses `Allow` / `Deny` (and optional scope).
5. Runner executes or blocks tool accordingly.
6. Decision is logged and optionally cached for run scope.

### Detailed task breakdown

1. Policy evaluator
- Build policy interface and default policy rules.
- Add risk classification by tool and argument patterns.
- Add explicit denylist/high-risk path support.

2. Approval interaction schema
- Define options:
  - `allow_once`
  - `allow_for_run` (optional)
  - `deny`
- Include tool name, summarized args, risk level in interaction metadata.

3. Execution gating
- Enforce policy check before every tool execution.
- Block tool until approval resolves.
- Prevent bypass via retries or reentrant tool path.

4. Decision caching and scope
- Add short-lived approval cache keyed by (`run_id`, policy fingerprint).
- Respect scope chosen by user.
- Invalidate cache on run end.

5. Audit and security
- Emit immutable audit events for approval requested/decision/applied.
- Redact sensitive arg fields in displayed metadata and logs.
- Add rate limiting for repeated approval prompts.

6. Testing
- Unit tests for policy decisions and cache behavior.
- Integration tests for allow/deny branches.
- Adversarial tests for replayed/forged approval responses.

7. UX and governance
- Keep approval dialogs concise but informative.
- Include "why approval is needed" text from policy engine.
- Document operator override strategy (if ever needed).

---

## Suggested Delivery Order

1. Complete Wave 0 and Wave 1 (Question Tool only).
2. Freeze question scope except bug fixes; complete Wave 2 (Steering only).
3. Freeze steering scope except bug fixes; complete Wave 3 (Approvals only).
4. Perform Wave 4 rollout hardening and staged enablement.

No interleaving rule:

- Do not begin implementation tasks for steering before question exit criteria are met.
- Do not begin implementation tasks for approvals before steering exit criteria are met.
- Cross-wave changes are allowed only for blocker bug fixes discovered in the current wave.
