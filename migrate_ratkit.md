# Ratkit Migration Plan for `hh` TUI

## 1) High-Level Goal

Migrate the `hh` interactive TUI runtime from the current custom `crossterm + ratatui` event loop to a `ratkit`-coordinated runtime in a staged, reversible, and testable way, while preserving current behavior, performance, and operator trust.

This migration should:

- Preserve the existing user experience and key interaction semantics.
- Reduce runtime/event-loop complexity and improve long-term maintainability.
- Keep rendering quality and current visual identity intact unless intentionally changed.
- Avoid risky big-bang rewrites by using incremental, parity-first phases.


## 2) Principles

1. **Correctness before convenience**
   - Behavior parity with current TUI is required before cleanup or optimization.

2. **Single source of truth**
   - Remove dual ownership patterns in UI state to avoid split-brain behavior.

3. **Reversible migration**
   - Every phase must have a rollback path (feature flags, runtime switch, additive changes).

4. **Explicit boundaries**
   - Keep strict boundaries between input normalization, state transitions, side effects, and rendering.

5. **Parity-first, then refactor**
   - Establish equivalence first; only then simplify internals.

6. **Instrument before changing hot paths**
   - Measure behavior and performance before and after each major phase.

7. **No hidden behavior shifts**
   - Any intentional UX changes must be explicitly documented and tested.


## 3) Detailed Phases

---

## Phase 0 - Baseline, Scope, and Safety Nets

### Goal

Establish a trustworthy baseline for behavior and performance before introducing ratkit.

### TODO Items

- Define migration scope (runtime/event loop only vs widget adoption).
- Capture representative interaction scenarios (chat, slash commands, questions, subagent session navigation, scrolling, selection, cancel flows).
- Capture debug-frame snapshots using existing debug/replay flow for representative scenarios.
- Record current performance baselines using `tui_perf_probe` (median, p95, max).
- Create a migration risk register with severity and owner.
- Define a no-regression checklist used in every later phase.

### Phase Principles

- Do not change behavior in this phase.
- Make acceptance measurable, not subjective.

### Acceptance Criteria

- Baseline scenarios are documented and reproducible.
- Performance baseline numbers are recorded and versioned.
- A no-regression checklist exists and is agreed.
- Migration scope and out-of-scope items are explicitly documented.

---

## Phase 1 - Architecture Alignment (without ratkit)

### Goal

Reduce known migration friction in the current code by tightening boundaries and eliminating avoidable duplication.

### TODO Items

- Audit and unify `InputEvent` normalization paths so one canonical path exists.
- Define explicit contracts for:
  - input normalization,
  - action dispatch,
  - side-effect handlers,
  - rendering.
- Update/align `docs/designs/tui.md` with actual architecture and intended target shape.
- Identify and mark legacy coupling points (especially where `legacy_chat_app` is required).
- Add lightweight invariants/assertions for state consistency where feasible.

### Phase Principles

- Prefer additive adapters over broad rewrites.
- Avoid moving business logic while standardizing boundaries.

### Acceptance Criteria

- Exactly one canonical input normalization path is used by the interactive loop.
- Architecture document reflects current truth plus target migration shape.
- Legacy coupling points are explicitly tracked with owner and planned removal phase.

---

## Phase 2 - Runtime Abstraction Layer

### Goal

Introduce a runtime abstraction around terminal lifecycle, event polling, and frame drawing so the loop backend is swappable.

### TODO Items

- Define a `UiRuntime` interface (or equivalent) with explicit methods for:
  - initialization/shutdown,
  - event retrieval,
  - redraw signaling,
  - frame drawing,
  - resize handling.
- Implement the abstraction using the current runtime first (no ratkit yet).
- Route the interactive loop through this abstraction.
- Preserve all existing key/mouse/paste/resize semantics.
- Add parity tests around event translation and redraw timing assumptions.

### Phase Principles

- Keep this phase behavior-preserving.
- No visual redesign and no new UX semantics.

### Acceptance Criteria

- Interactive chat runs entirely through the abstraction with no behavior regressions.
- All baseline scenarios from Phase 0 pass.
- Measured performance is within agreed tolerance of baseline.

---

## Phase 3 - State Unification (Remove Dual State Authority)

### Goal

Eliminate split ownership between `AppState` and legacy chat state, moving to one authoritative state graph.

### TODO Items

- Inventory all fields currently duplicated or mirrored.
- Define canonical ownership for each state domain (messages, session metadata, processing flags, selection, input, sidebar state).
- Migrate read/write paths incrementally so only one owner remains per domain.
- Introduce compatibility adapters where needed to keep rendering stable during transition.
- Remove dual-write logic and stale shadow fields once parity is proven.

### Phase Principles

- One domain, one owner.
- Migrate by domain, not by file count.
- Preserve external behavior while reducing internal ambiguity.

### Acceptance Criteria

- No UI domain has dual-write ownership.
- State transitions are deterministic and traceable from actions.
- Baseline scenarios pass, including subagent and pending-question flows.

---

## Phase 4 - Ratkit Runtime Spike (Non-default Path)

### Goal

Integrate ratkit as an alternate runtime path while retaining existing rendering and components.

### TODO Items

- Add `ratkit` dependency with minimal features (core runtime only unless needed).
- Implement `CoordinatorApp` adapter that maps ratkit events to existing action/input pipeline.
- Reuse existing render functions inside ratkit draw callback.
- Add runtime selection toggle (config/env/flag) between legacy loop and ratkit loop.
- Validate startup/shutdown correctness (raw mode, alt screen, mouse, bracketed paste behavior).

### Phase Principles

- Limit scope to runtime orchestration first.
- Keep rendering and UI component semantics unchanged.
- Keep rollback immediate via runtime toggle.

### Acceptance Criteria

- Ratkit runtime path starts and runs baseline scenarios successfully.
- Legacy runtime path still works unchanged.
- No critical parity regressions on input, redraw, or shutdown behavior.

---

## Phase 5 - Parity Hardening and Differential Verification

### Goal

Prove ratkit path parity, identify deltas, and close gaps before promoting ratkit to default.

### TODO Items

- Run side-by-side baseline scenarios for both runtimes.
- Compare debug-frame outputs for deterministic scenarios.
- Validate edge cases:
  - repeated resize events,
  - high-frequency stream updates,
  - large transcript scroll/selection,
  - interrupt/cancel paths,
  - pending question custom input mode.
- Fix behavior differences with explicit tests and notes.
- Re-run performance probe and compare against baseline.

### Phase Principles

- Differential testing over anecdotal confidence.
- Any accepted divergence must be intentional and documented.

### Acceptance Criteria

- All no-regression checklist scenarios pass under ratkit.
- Any remaining differences are explicitly approved and documented.
- Performance remains within accepted budget or has documented tradeoff and mitigation.

---

## Phase 6 - Optional Ratkit Primitive Adoption (Selective)

### Goal

Adopt ratkit-provided widgets/primitives only where they materially reduce complexity or maintenance cost.

### TODO Items

- Evaluate each candidate primitive/component with cost-benefit criteria:
  - net complexity reduction,
  - feature parity,
  - styling control,
  - testability,
  - dependency/upgrade risk.
- Prioritize low-risk candidates (e.g., dialogs/toasts/split layout helpers) before high-risk core panes.
- Maintain visual consistency with current theme and interaction model.
- Keep replacements isolated and reversible.

### Phase Principles

- Do not adopt primitives for novelty.
- Local replacement only when net maintenance benefit is clear.

### Acceptance Criteria

- Each adopted primitive has a documented rationale and rollback path.
- No UX regressions introduced by primitive swaps.
- Styling and behavior stay aligned with `hh` identity.

---

## Phase 7 - Ratkit Default, Legacy Deprecation, and Cleanup

### Goal

Make ratkit runtime the default path, then remove obsolete legacy runtime code safely.

### TODO Items

- Flip default runtime to ratkit after parity sign-off.
- Keep temporary fallback switch for one stabilization window.
- Monitor issues/regressions and patch quickly.
- Remove legacy runtime code paths after stabilization criteria are met.
- Simplify architecture docs and code comments to reflect final state.
- Ensure CI includes checks that enforce the new runtime path.

### Phase Principles

- Deprecate first, delete second.
- Cleanup only after operational confidence.

### Acceptance Criteria

- Ratkit is default and stable for agreed window.
- Legacy path removal does not reduce test coverage or observability.
- Documentation and architecture diagrams match implementation.

---

## Phase 8 - Post-Migration Validation and Maintenance Policy

### Goal

Lock in long-term safety and maintainability after migration completes.

### TODO Items

- Define upgrade policy for `ratkit`, `ratatui`, and `crossterm`.
- Add regression suites for critical interaction contracts.
- Keep perf probes in CI or scheduled checks.
- Add troubleshooting guide for runtime/event-loop issues.
- Record lessons learned and future refactor opportunities.

### Phase Principles

- Migration is not done until maintenance is routine.
- Preserve operational confidence with continuous verification.

### Acceptance Criteria

- Version upgrade playbook exists and is tested.
- Regression/perf checks run automatically and are actionable.
- Team has documented operational runbooks for TUI runtime incidents.


## Cross-Phase Exit Gates

No phase advances unless:

- Baseline scenario checklist passes.
- Performance is within accepted budget (or exception is documented and approved).
- Rollback path exists and is verified.
- Documentation is updated for any architectural change.


## Suggested Tracking Template (per phase)

Use this structure in issue/PR tracking:

- `Phase`: <N>
- `Status`: planned | in_progress | blocked | done
- `Owner`: <name>
- `Risks`: <top risks>
- `Mitigations`: <actions>
- `Acceptance Evidence`: <tests, traces, frame captures, perf output>
- `Rollback Plan`: <how to revert safely>
