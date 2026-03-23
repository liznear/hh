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

### 1) Runtime model: event-driven state machine

Refactor the runner/loop from a closed sequential loop into an event-driven state machine.

Core states:

- `idle`: no active run.
- `running`: model/tool execution active.
- `waiting_for_interaction`: blocked on external response for an interaction.
- `paused`: run intentionally paused (optional but useful).
- `completed` / `failed` / `cancelled`: terminal states.

State transitions are driven by explicit events and should be logged.

### 2) Two channels: data plane and control plane

- Data plane: model streaming, tool calls, tool results.
- Control plane: user messages, interaction responses, cancel/pause/resume.

This separation prevents hidden coupling and makes interruption behavior explicit.

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

### 5) Inbound event inbox

Add an inbox for external events:

- `user_message_received`
- `interaction_responded`
- `pause_requested`
- `cancel_requested`

Use ordered sequence numbers per run to preserve deterministic processing.

### 6) Checkpoint preemption model

Support preemption at safe checkpoints (recommended default):

- after streaming chunk boundary,
- after model turn completion,
- before tool execution,
- after tool result.

At each checkpoint, process control events before continuing.

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
- `user_message_received`, `user_message_applied`

### 9) Safety and failure handling

- Unknown/expired interaction response: reject with explicit error event.
- Duplicate responses: first wins, subsequent ignored and logged.
- Runner restart with pending interactions: restore from persisted state or fail run explicitly.
- In-flight cancellation: deterministic cancellation path (no half-applied state).

## Implementation Roadmap (Sequential, Non-Interleaved)

This roadmap intentionally avoids parallel feature development across question, steering, and approvals.
Only complete shared runtime primitives and the current feature wave before starting the next one.

### Wave 0 - Shared primitives required for Question Tool only

Scope rule: implement only the minimum shared runtime needed for question interactions. Do not add steering or approval logic in this wave.

1. Define state machine types and transitions for `idle`, `running`, `waiting_for_interaction`, terminal states.
2. Add event envelopes with correlation IDs (`run_id`, `tool_call_id`, `interaction_id`, timestamps).
3. Implement `InteractionRequest`/`InteractionResponse` schemas and validators.
4. Implement `InteractionManager` (register, wait, resolve once, timeout, cleanup).
5. Add external event ingress for interaction responses only.
6. Add logs and tests for interaction lifecycle and state transitions.

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

1. Add run inbox for `user_message_received` events with ordered sequence numbers.
2. Add checkpoint processing hooks for safe steering application.
3. Implement steering policy (`queue_until_checkpoint` default, optional `cancel_and_replan`).
4. Add explicit conversation append path for steering messages.
5. Add concurrency safeguards (ordering, idempotency, cancel race handling).
6. Add tests for streaming/tool/waiting states with steering events.
7. Emit `user_message_applied` and steering latency metrics.

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

- [ ] Define runner state machine transitions for interaction pause/resume.
- [x] Add event envelope fields (`run_id`, `tool_call_id`, `interaction_id`, timestamps).
- [x] Implement `InteractionRequest`/`InteractionResponse` schemas and validation.
- [x] Implement `InteractionManager` register/wait/resolve/timeout lifecycle.
- [x] Add external ingress for interaction responses.
- [x] Add tests for duplicate/unknown/expired responses.
- [ ] Add logs for interaction lifecycle and state transitions.
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

- [ ] Add run inbox for `user_message_received` with ordered sequence IDs.
- [ ] Add checkpoint hooks for control-plane event processing.
- [ ] Implement default steering policy `queue_until_checkpoint`.
- [ ] Optionally implement `cancel_and_replan` policy mode.
- [ ] Add explicit conversation append path with `source=user_steer` tags.
- [ ] Add ordering/idempotency/cancel-race safeguards.
- [ ] Add streaming/tool/wait-state steering tests.
- [ ] Emit `user_message_applied` and steering latency metrics.
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

Steering uses control plane events (`user_message_received`) processed at checkpoints.
The loop remains responsive without unsafe arbitrary interruption.

Steering flow:

1. User sends a new message during streaming/execution.
2. Runner enqueues event in run inbox with sequence number.
3. At next checkpoint, loop applies steering policy.
4. Runner injects user message into conversation state.
5. Loop continues with updated context.

### Detailed task breakdown

1. Inbound API and event model
- Add endpoint/method for `user_message_received` while run is active.
- Assign monotonic `event_seq` and timestamp.
- Validate payload size and content constraints.

2. Checkpoint processing
- Define checkpoint locations and ordering guarantees.
- Add handler to drain or partially drain inbox at checkpoint.
- Ensure interaction responses are prioritized over steering when blocked.

3. Steering policy
- Define policy modes:
  - `queue_until_checkpoint` (default)
  - `cancel_and_replan` (optional)
- Implement deterministic behavior for each mode.
- Expose mode via config/feature flag.

4. Conversation state mutation
- Add explicit append path for steering messages.
- Tag inserted messages as `source=user_steer` for traceability.
- Ensure no hidden rewriting of prior model/tool history.

5. Concurrency and correctness
- Handle multiple steering messages in order.
- Handle steering while waiting for interaction.
- Prevent race between cancel and steering apply.

6. Testing
- Integration tests for steering during streaming and tool phases.
- Property tests for ordering/idempotency.
- Failure tests for malformed/late events.

7. UX feedback
- Emit `user_message_applied` event so UI confirms steering took effect.
- Optionally show "applied at checkpoint" indicator.

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
