# Widget-First UI Migration Plan

This plan migrates the UI from line-composition-first rendering to widget-composition-first rendering, while preserving a single global scroll model at the container level.

## Goals

- Make UI blocks render as first-class widgets.
- Keep scroll ownership at the transcript/container layer.
- Remove duplicate rendering logic (especially for diff rendering).
- Preserve correctness, inspectability, and visual parity during migration.

## Non-goals

- Per-widget internal scrolling.
- Broad visual redesign during migration.
- Unrelated behavior changes outside rendering/scroll architecture.

## Phase 0 - Baseline and Guardrails

### TODO

- [ ] Write a short design note for target renderer contract:
  - container owns scroll state
  - blocks own rendering
  - no internal block scrolling
- [ ] Inventory all currently rendered UI sections in `src/app/render.rs`.
- [ ] Capture golden tmux artifacts for representative scenarios:
  - markdown-heavy transcript
  - tool call + tool result transcript
  - diff rendering (unified + side-by-side)
  - long transcript with scrolling
- [ ] Capture performance baseline for representative sessions (render latency and memory churn where measurable).
- [ ] Add rollout toggle (feature flag/config/env) for old vs new renderer path.

### Completion criteria

- [ ] Baseline artifacts are committed and reproducible.
- [ ] Renderer scope inventory is complete and reviewed.
- [ ] Old and new renderer paths can be toggled at runtime or build-time.

## Phase 1 - Block Abstraction Without Visual Change

### TODO

- [ ] Add a UI block abstraction (enum or trait object) representing renderable units.
- [ ] Define block-level measurement API (`measured_height(width) -> u16`).
- [ ] Build a shared layout pass:
  - measure block heights
  - compute cumulative offsets
  - derive visible block range from viewport + scroll offset
- [ ] Add adapter blocks that delegate to existing line rendering for now.
- [ ] Keep output identical by rendering adapters through current logic.

### Completion criteria

- [ ] New block pipeline runs end-to-end behind toggle.
- [ ] No intentional visual changes.
- [ ] Existing rendering tests remain green.

## Phase 2 - Message Body Blocks

### TODO

- [ ] Migrate markdown content to direct widget rendering.
- [ ] Migrate plain text/code snippets to dedicated blocks/widgets.
- [ ] Stop flattening migrated message-body sections into one global line vector.
- [ ] Preserve wrapping and indent behavior.
- [ ] Add tests for width-sensitive wrapping and measured height consistency.

### Completion criteria

- [ ] Message body sections render via widget blocks.
- [ ] Wrapping/indent parity is maintained.
- [ ] Snapshot parity for representative transcripts.

## Phase 3 - Tool Call and Tool Result Blocks

### TODO

- [ ] Create blocks/widgets for tool headers and status rows.
- [ ] Create blocks/widgets for tool output body sections.
- [ ] Preserve count summaries, truncation behavior, and status styling.
- [ ] Validate mixed transcripts (assistant/user/tool interleaving).
- [ ] Add tests for expanded/collapsed tool states and long outputs.

### Completion criteria

- [ ] Tool sections render through block widgets.
- [ ] Behavior parity for truncation/formatting is preserved.
- [ ] Scroll behavior remains stable in tool-heavy transcripts.

## Phase 4 - Diff Rendering Through CodeDiff Widget Only

### TODO

- [ ] Replace all app-side diff line consumption with direct `CodeDiff` widget render.
- [ ] Route layout mode and frame config from app theme/config into `CodeDiff`.
- [ ] Remove duplicate app-side side-by-side diff parsing/render code.
- [ ] Keep `CodeDiff` as single source of truth for diff semantics and styling.
- [ ] Add/keep regression tests for:
  - hidden `---/+++` rows in side-by-side
  - measured height with padding
  - no duplicated tokens (`pubpub`)
  - add/remove prefix background contrast

### Completion criteria

- [ ] App no longer re-implements diff rendering logic.
- [ ] All diff rendering goes through `hh-widgets::codediff::CodeDiff`.
- [ ] Diff snapshots/parity checks pass.

## Phase 5 - Container-Level Scrolling Finalization

### TODO

- [ ] Make scroll math depend only on block layout metadata.
- [ ] Render only visible blocks (viewport clipping).
- [ ] Clamp offsets safely on resize and content mutation.
- [ ] Keep keyboard navigation semantics unchanged.
- [ ] Add tests for:
  - resize while mid-scroll
  - appended messages at bottom vs away from bottom
  - mixed block heights around viewport boundaries

### Completion criteria

- [ ] Single global scrolling model is stable.
- [ ] No jump/jitter regressions on resize/appends.
- [ ] Keyboard navigation parity is maintained.

## Phase 6 - Legacy Path Removal

### TODO

- [ ] Remove legacy line-first renderer code paths.
- [ ] Remove migration adapters and toggle after rollout.
- [ ] Simplify render module structure into:
  - layout pass
  - render pass
  - style/theme mapping
- [ ] Update architecture docs to reflect final model.

### Completion criteria

- [ ] Exactly one production renderer path remains.
- [ ] Dead code and temporary compatibility layers are removed.
- [ ] Docs match implementation.

## Phase 7 - Validation and Hardening

### TODO

- [ ] Re-run baseline tmux captures from Phase 0 and compare.
- [ ] Re-run performance checks and document deltas.
- [ ] Run full quality gates:
  - `cargo test`
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
- [ ] Fix regressions and update baselines if changes are intentional.
- [ ] Prepare rollout notes and fallback strategy.

### Completion criteria

- [ ] No critical visual or behavior regressions.
- [ ] Performance is neutral or improved (or justified with rationale).
- [ ] CI checks are green.

## Suggested PR Breakdown

1. Phase 0-1 scaffolding PRs (no visual change)
2. Message/tool block migration PRs (Phase 2-3)
3. Diff direct-widget migration PR (Phase 4)
4. Scroll finalization PR (Phase 5)
5. Legacy cleanup + hardening PRs (Phase 6-7)

## Risk Notes

- Highest-risk area is scroll behavior under resize and dynamic content updates.
- Keep migrations additive and reversible until Phase 6.
- Preserve artifact-based parity checks throughout rollout.
