# DESIGN

This document explains the runtime architecture of `hh`, how major modules interact, and the rules we use when adding new code.

## 1) System Design

### High-level flow

1. CLI entry (`hh chat`, `hh run`) builds `Settings` and chooses mode.
2. Chat layer (`src/cli/chat.rs` + `src/cli/chat/*`) constructs an `AgentCore` with:
   - provider adapter
   - tool registry
   - approval policy matcher
   - session store
   - event sink (TUI or noop)
3. `AgentCore` (`src/core/agent/mod.rs`) runs turn-by-turn:
   - loads/replays session history
   - sends provider request
   - streams thinking/assistant deltas to events
   - handles tool calls + approval decisions
   - persists events and updates runtime state
4. UI (`src/cli/tui/*`) renders state and user interactions.

### Main components

- `src/core/types.rs`
  - Canonical domain model: `Message`, `ToolCall`, `ToolSchema`, `TodoItem`, `QuestionPrompt`, provider request/response types.
  - Provider-agnostic and persistence-friendly.

- `src/core/traits.rs`
  - Integration contracts: `Provider`, `ToolExecutor`, `ApprovalPolicy`, `SessionSink`, `SessionReader`.
  - The agent loop depends on traits, not concrete adapters.

- `src/core/agent/mod.rs`
  - Orchestrates the agent runtime loop.
  - Handles replay, step progression, approval requests, blocking/non-blocking tool execution, and event emission.
  - Defines `RunnerOutputObserver`, the adapter contract used by UI/render layers.

- `src/provider/openai_compatible.rs`
  - Provider adapter between wire format and core types.
  - Includes fallback request strategies (now iterative attempts) for compatibility.

- `src/tool/*`
  - Tool implementations (`read`, `write`, `bash`, `todo_write`, `task`, etc.).
  - `ToolRegistry` composes tools and exposes executor behavior.

- `src/permission/*`
  - Approval policy and matching logic (`PermissionMatcher`, rule matching).
  - Uses capability-based policy lookup from `Settings`.

- `src/session/*`
  - Append-only session event persistence and replay.
  - Stores canonical runtime history and approval outcomes.

- `src/cli/chat/*`
  - Mode-specific orchestration for interactive chat and single prompt.
  - Split by concern: `input`, `commands`, `session`, `subagent`, `agent_run`.

- `src/cli/tui/*`
  - App state (`app.rs`) and rendering surface (`ui/*.rs`).
  - UI split by concern: `theme`, `messages`, `input`, `sidebar`, `overlays`.

### Interaction pattern (data boundaries)

- Core loop talks only through traits.
- Providers/tools/session implementations map to/from core types.
- TUI consumes event stream + app state; it does not embed runtime loop logic.
- Session events compose core types and add lifecycle metadata.

## 2) Design Principles and Module Rules

### A. Ownership and placement

- Put cross-provider LLM semantics in `core/types.rs`.
- Put orchestration logic in `core/agent/*`.
- Put integration boundaries in `core/traits.rs`.
- Put wire/protocol specifics in adapters (`provider/*`, `tool/*`, `session/*`, `cli/*`).
- Put view-only formatting/rendering in `cli/tui/ui/*`.

### B. Dependency direction

- `core` should not depend on concrete provider/tool/UI internals.
- Adapters may depend on `core`.
- UI depends on core traits/events and app view state, never the internal loop algorithm.

### C. Domain model consistency

- Prefer one canonical type per concept (`TodoItem`, `ToolSchema`, status enums).
- Avoid duplicate enums/structs in tool or UI modules unless they are purely view models.
- If conversion is required, centralize conversion in one place and keep names explicit.

### D. Persistence and compatibility

- Session events are append-only historical truth.
- Preserve serialization compatibility for existing logs.
- Add new fields additively and default them safely.

### E. Approval and policy

- Capability-to-policy resolution must be centralized (single helper in `Settings`).
- Approval request presentation may vary by mode (TUI/non-TUI), but decision semantics must remain identical.
- Persist approval choices only where defined by policy (for example, bash local rules).

### F. Runtime safety

- Prefer structured errors over panics in runtime paths.
- Keep fallback logic explicit and inspectable (strategy lists over deep nested branching).
- Non-blocking tools must not break event ordering guarantees.

### G. UI decomposition

- Keep `ui/*` files concern-focused:
  - `messages`: history rendering + selection visuals
  - `input`: input panel, cursor layout, processing indicator
  - `sidebar`: context/todo/modified files presentation
  - `overlays`: command palette, clipboard and transient overlays
  - `theme`: constants/layout tokens
- UI modules should avoid hidden coupling to runtime internals.

### H. Change workflow

- Prefer behavior-preserving extraction before logic changes.
- Validate each meaningful step with:
  - `cargo check`
  - `cargo test`
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
- Update `refactor_plan.md` (or equivalent) as changes land.

## Practical guidance for future features

- New provider: implement `Provider`, map wire objects to core types, avoid leaking wire-only fields into core.
- New tool: implement `Tool`, register in `ToolRegistry`, define `ToolSchema` + capability, ensure approval behavior is explicit.
- New approval behavior: update centralized capability mapping and keep session persistence semantics stable.
- New UI behavior: add/adjust in the relevant `ui/*` concern module; avoid growing `ui.rs` orchestration wrappers.
