# Refactor Plan

## Objectives
- Reduce complexity and coupling in core runtime and CLI/TUI without changing user-facing behavior.
- Eliminate duplicated domain modeling (`todo`, subagent status, capability policy mapping).
- Improve maintainability of large files (`cli/chat.rs`, `cli/tui/ui.rs`) through module boundaries.
- Preserve correctness via small, reversible phases with explicit validation gates.

## Guardrails
- Prefer additive, reversible changes.
- Keep all public tool names and wire formats stable unless explicitly noted.
- No behavior changes to approval policy defaults.
- Validate each phase with:
  - `cargo check`
  - `cargo test`
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`

## Baseline (Current Hotspots)
- `src/cli/chat.rs` (~3319 LOC): event loop + runtime orchestration + command handling + session logic.
- `src/cli/tui/ui.rs` (~2618 LOC): all rendering paths in one file.
- `src/cli/tui/app.rs` (~1700 LOC): rich app state with protocol conversion helpers.
- `src/core/agent/mod.rs` (~605 LOC): main loop + approval + todo snapshot logic.
- `src/provider/openai_compatible.rs` (~680 LOC): provider mapping + fallback tree.

---

## Phase 0 - Safety Harness and Mapping (Low risk)

### Scope
- Establish module ownership map and invariants before moving code.
- Add targeted tests around behavior likely to regress during extraction.

### Changes
- Add/expand tests for:
  - Approval choice parsing and persistence behavior.
  - Tool result -> todo state updates.
  - Subagent status mapping (`manager` -> session -> TUI view).
- Document invariants in module-level docs where missing.

### Files
- `src/cli/chat.rs`
- `src/core/agent/state.rs`
- `src/core/agent/subagent_manager.rs`
- `src/cli/tui/app.rs`

### Blast radius
- Minimal runtime impact; mostly tests and docs.

### Exit criteria
- Existing behavior covered by regression tests.
- Full build/test/lint green.

---

## Phase 1 - Extract `cli/chat` into Focused Modules (Medium risk, highest payoff)

### Scope
- Decompose `src/cli/chat.rs` into coherent units while keeping function signatures and behavior stable.

### Proposed modules
- `src/cli/chat/mod.rs` (public entrypoints)
- `src/cli/chat/input.rs` (keyboard/mouse/clipboard/selection)
- `src/cli/chat/commands.rs` (slash command dispatch)
- `src/cli/chat/agent_run.rs` (agent execution and loop wiring)
- `src/cli/chat/session.rs` (session title generation/resume/compaction helpers)
- `src/cli/chat/subagent.rs` (subagent manager init + mapping)

### Changes
- Move code only first; avoid logic changes in extraction commit(s).
- Keep `run_chat`, `run_single_prompt`, `run_single_prompt_with_events` signatures stable.
- Keep event types and `TuiEventSender` usage unchanged.

### Blast radius
- High local churn in CLI runtime but limited external API impact.

### Exit criteria
- No behavior change in manual smoke tests:
  - `hh chat`
  - `hh run "hello"`
  - slash commands `/new`, `/model`, `/resume`, `/compact`, `/quit`
- Full build/test/lint green.

---

## Phase 2 - Split TUI Rendering Surface (Medium risk)

### Scope
- Decompose `src/cli/tui/ui.rs` into render submodules to reduce cognitive load.

### Proposed modules
- `src/cli/tui/ui/mod.rs`
- `src/cli/tui/ui/messages.rs`
- `src/cli/tui/ui/sidebar.rs`
- `src/cli/tui/ui/input.rs`
- `src/cli/tui/ui/overlays.rs` (command palette, clipboard notice, question prompt)
- `src/cli/tui/ui/theme.rs` (constants and layout tokens)

### Changes
- Move constants and helper functions by concern.
- Keep layout and visual behavior equivalent.
- Keep existing `ui_tests` passing without snapshot churn unless intentional.

### Blast radius
- Medium; rendering code path only.

### Exit criteria
- No regressions in TUI unit tests.
- Manual visual smoke check for chat flow, command palette, and sidebar.

---

## Phase 3 - Unify Domain Types and Conversion Boundaries (Medium/High risk)

### Scope
- Remove duplicated status/priority enums and centralize conversion points.

### Changes
- Replace duplicated tool-local todo enums with `core` todo types.
  - `src/tool/todo.rs` should deserialize/serialize `crate::core::TodoItem` directly.
- Keep TUI-specific view models where needed, but move conversions into dedicated conversion module.
- Define a single status mapping path for subagent status.

### Target files
- `src/core/types.rs`
- `src/tool/todo.rs`
- `src/cli/tui/app.rs`
- `src/core/agent/subagent_manager.rs`
- `src/session/types.rs`

### Blast radius
- Medium/high; affects persistence, tool payloads, and UI adapters.

### Exit criteria
- Serialization compatibility preserved for existing session event logs.
- No user-visible status label regressions.

---

## Phase 4 - Core Boundary Cleanup (`core` independence) (High design value)

### Scope
- Remove `core` dependency on `tool` module types for better architecture alignment.

### Changes
- Move `ToolSchema` to `core` (or new neutral `domain` module used by both core and tools).
- Update references:
  - `ProviderRequest.tools`
  - `ToolExecutor::schemas`
  - `PermissionMatcher` inputs
- Keep runtime behavior unchanged.

### Blast radius
- Broad compile-time churn, low expected runtime behavior change.

### Exit criteria
- `core` compiles without importing from `tool::*`.
- All modules compile and tests pass.

---

## Phase 5 - Policy and Approval Simplification (Medium risk)

### Scope
- Remove duplicated capability-policy mappings and approval flow duplication.

### Changes
- Centralize capability policy key resolution in one helper shared by:
  - settings apply-agent overrides
  - permission matcher lookup
- Introduce a small approval presentation adapter used by both TUI and non-TUI paths.
- Keep persistence of `AllowAlways`/`Deny` rules behavior identical.

### Blast radius
- Medium; impacts approval and permission behavior.

### Exit criteria
- Approval prompts and decisions match pre-refactor behavior.
- Rule persistence unchanged for bash approvals.

---

## Phase 6 - Provider and Tool Runtime Quality Pass (Low/Medium risk)

### Scope
- Simplify internals without changing interfaces.

### Changes
- Refactor OpenAI-compatible fallback logic into iterative strategy list to replace nested branching.
- Standardize tool arg parsing:
  - migrate manual `serde_json::Value` extraction to typed args where appropriate.
- Remove hard panic in tool registry initialization; return structured error instead.

### Blast radius
- Medium; provider and tool execution path.

### Exit criteria
- Same provider request/retry behavior for successful and fallback cases.
- No panics in normal initialization paths.

---

## Suggested Commit Plan
- Commit 1: Phase 0 tests/invariants.
- Commit 2-4: Phase 1 extraction in slices (`input`, `commands`, `agent_run/session`).
- Commit 5-6: Phase 2 rendering extraction.
- Commit 7: Phase 3 todo/status unification.
- Commit 8: Phase 4 core boundary update.
- Commit 9: Phase 5 policy/approval consolidation.
- Commit 10: Phase 6 provider/tool cleanup.

Each commit should pass full validation commands before moving to the next.

## Rollback Strategy
- Keep each phase in separate commits to allow `git revert <commit>` per phase.
- Avoid schema-breaking persistence changes until Phase 3 is fully tested.
- If a phase introduces behavior drift, revert that phase and continue with independent phases.
