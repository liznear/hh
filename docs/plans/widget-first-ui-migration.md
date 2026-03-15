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

## Cross-Phase Invariants

- One global transcript scroll owner (`MessagesComponent`) for all renderer modes.
- Block measurement and render are deterministic for a fixed width + input data.
- No provider-specific wire fields leak into block contracts.
- Diff semantics and styling are sourced from `hh-widgets::codediff::CodeDiff` only after Phase 4.
- Migration remains additive and reversible until legacy removal in Phase 6.

## Execution Tracker

Use this as a lightweight status index in PR descriptions.

| Phase | Status | Evidence expected |
| --- | --- | --- |
| 0 | in progress | design note, inventory, baseline artifacts, baseline perf table |
| 1 | pending | block contract + layout tests + toggle parity snapshots |
| 2 | pending | assistant/user body block snapshots across widths |
| 3 | pending | tool state transition and truncation test matrix results |
| 4 | pending | diff path deletion + `CodeDiff` parity snapshots |
| 5 | pending | scroll/resize/append boundary test results |
| 6 | pending | legacy path removal diffs + docs updates |
| 7 | pending | baseline rerun comparison + CI green + rollout notes |

## File/Module Touch Map (planned)

- Existing hotspots:
  - `src/app/render.rs`
  - `src/app/components/messages.rs`
  - `src/app/components/viewport_cache.rs`
- Planned additive modules (names can be adjusted to repo conventions):
  - `src/app/components/messages_blocks.rs` (block contracts + constructors)
  - `src/app/components/messages_layout.rs` (measure, offsets, visible range)
  - `src/app/components/messages_render.rs` (widget render pass)
  - `src/app/components/messages_style.rs` (theme mapping for blocks)
- Widgets boundary:
  - `hh-widgets::codediff::CodeDiff` remains the diff rendering authority after Phase 4.

## Phase 0 - Baseline and Guardrails

### Design note draft: target renderer contract

- `MessagesComponent` owns one global transcript scroll offset and viewport clipping window.
- Renderable content is represented as ordered blocks (one block list per transcript).
- Each block is pure with respect to layout inputs: given width + message data, measurement and render output are deterministic.
- Block API shape:
  - `measured_height(width) -> u16`
  - `render(area, frame)` with no internal scrolling state
- Layout pass is the source of truth for:
  - cumulative block offsets
  - visible block range for current viewport + scroll offset
  - clamping behavior after resize and transcript mutation
- Blocks do not mutate global scroll state directly.
- Provider/tool semantics stay outside widgets; widgets consume normalized domain/view-model data.

### Inventory: current rendered UI sections in `src/app/render.rs`

The current line-first renderer composes transcript output in `build_message_lines_impl` and `render_message_line_item`, then delegates by `ChatMessage` variant.

- Transcript container/root composition:
  - `render_root_layout`
- Message body and status blocks:
  - `ChatMessage::Assistant` -> `parse_markdown_lines`
  - `ChatMessage::User` -> `render_user_message_block`
  - `ChatMessage::Thinking` -> `render_thinking_block`
  - `ChatMessage::CompactionPending`/`Compaction` -> `render_compaction_block`
  - `ChatMessage::Footer` -> `render_footer_block`
  - `ChatMessage::Error` -> inline error row rendering
- Tool call/result blocks:
  - `ChatMessage::ToolCall` -> `render_tool_call_message`
  - completed/pending/error details ->
    - `render_completed_tool_call`
    - `render_pending_tool_call`
    - `render_tool_error_detail`
- Diff rendering paths (currently duplicated semantics):
  - `render_edit_diff_block` (app-side side-by-side parser and renderer)
  - `render_edit_diff_block_single_column` (uses `CodeDiff::from_unified_diff`)
  - helper parsing/layout: `next_diff_row`, `render_side_by_side_diff_row`, `render_diff_cell`

### Baseline artifact capture plan (tmux)

Store captures under `docs/artifacts/widget-first-ui/phase0/` and keep file names stable so diffs are reviewable.

Captured in this branch:

- `docs/artifacts/widget-first-ui/phase0/markdown-heavy.txt`
- `docs/artifacts/widget-first-ui/phase0/tool-call-result.txt`
- `docs/artifacts/widget-first-ui/phase0/diff-unified-and-side-by-side.txt`
- `docs/artifacts/widget-first-ui/phase0/long-transcript-scroll.txt`

- `markdown-heavy.txt`
- `tool-call-result.txt`
- `diff-unified-and-side-by-side.txt`
- `long-transcript-scroll.txt`

Use `cargo run -- ...` from this workspace per `AGENTS.md`:

```bash
tmux kill-session -t hh-phase0 || true
tmux new-session -d -s hh-phase0
tmux set-option -t hh-phase0 remain-on-exit on
tmux send-keys -t hh-phase0 'cargo run -- run "<scenario prompt>"' C-m
sleep 5
tmux capture-pane -p -t hh-phase0 -S -400 > docs/artifacts/widget-first-ui/phase0/<scenario>.txt
```

### Performance baseline checklist (Phase 0)

- Record per-scenario render latency at two widths (80 and 120 columns).
- Record peak resident memory during long transcript replay.
- Record transcript length and tool output sizes used for each baseline.
- Store numbers in a small table in this document (or adjacent `phase0-baseline.md`) so Phase 7 can diff against it.

Baseline measurements (captured on this branch via `/usr/bin/time -l`):

| Scenario | Width | Real (s) | Max RSS (bytes) | Peak footprint (bytes) |
| --- | --- | ---: | ---: | ---: |
| simple prompt (`baseline-simple`) | default | 2.42 | 23,117,824 | 4,637,128 |
| markdown-heavy | 80 | 4.50 | 23,314,432 | 4,702,664 |
| markdown-heavy | 120 | 5.81 | 23,396,352 | 4,784,584 |
| tool call + result (`read src/app/mod.rs`) | 80 | 3.82 | 23,642,112 | 4,932,040 |
| tool call + result (`read src/app/mod.rs`) | 120 | 5.01 | 23,822,336 | 4,948,424 |
| diff (unified sample) | 80 | 3.11 | 23,134,208 | 4,653,512 |
| diff (unified sample) | 120 | 2.92 | 23,232,512 | 4,719,048 |

### Rollout toggle shape (proposed)

- Add one renderer mode toggle with explicit values (avoid bool ambiguity):
  - `legacy-lines`
  - `widget-blocks`
- Suggested env key: `HH_UI_RENDERER_MODE`.
- Suggested config key: `ui.renderer_mode`.
- Resolve precedence explicitly: CLI flag > env > config > default.
- Default remains `legacy-lines` until Phase 6.

### TODO

- [x] Write a short design note for target renderer contract:
  - container owns scroll state
  - blocks own rendering
  - no internal block scrolling
- [x] Inventory all currently rendered UI sections in `src/app/render.rs`.
- [ ] Capture golden tmux artifacts for representative scenarios:
- [x] Capture golden tmux artifacts for representative scenarios:
  - markdown-heavy transcript
  - tool call + tool result transcript
  - diff rendering (unified + side-by-side)
  - long transcript with scrolling
- [x] Capture performance baseline for representative sessions (render latency and memory churn where measurable).
- [x] Add rollout toggle (feature flag/config/env) for old vs new renderer path.
  - proposed keys documented in this plan (`HH_UI_RENDERER_MODE`, `ui.renderer_mode`)

### Completion criteria

- [x] Baseline artifacts are committed and reproducible.
- [x] Renderer scope inventory is complete and reviewed.
- [x] Old and new renderer paths can be toggled at runtime or build-time.

## Phase 1 - Block Abstraction Without Visual Change

### Proposed implementation shape

Use additive scaffolding so the legacy renderer remains the source of truth while block layout mechanics are validated.

- Add a new module for block contracts and layout metadata (for example `src/app/components/messages_blocks.rs`).
- Keep block data provider-agnostic and derived from `ChatMessage` + app state.
- Route render mode through a single mode enum at component boundary (legacy lines vs widget blocks).

Suggested core types:

```rust
enum TranscriptBlock {
    AssistantMarkdown(AssistantMarkdownBlock),
    UserBubble(UserBubbleBlock),
    Thinking(ThinkingBlock),
    Compaction(CompactionBlock),
    ToolCall(ToolCallBlock),
    Footer(FooterBlock),
    Error(ErrorBlock),
}

trait MeasurableBlock {
    fn measured_height(&self, width: u16) -> u16;
    fn render(&self, f: &mut Frame, area: Rect);
}

struct BlockLayoutRow {
    block_index: usize,
    start_y: u32,
    height: u16,
}
```

Suggested layout pass contract:

- Input: `&[TranscriptBlock]`, viewport width/height, global `scroll_offset`.
- Output:
  - ordered `Vec<BlockLayoutRow>` with cumulative offsets
  - visible range `(start_idx, end_idx)`
  - total transcript height for clamping and scrollbar math
- Invariants:
  - `start_y` is monotonic non-decreasing
  - `height >= 1` for all blocks
  - visible range exactly covers viewport intersection

### Phase 1 testing plan

- Unit tests for layout pass:
  - zero/one/many blocks
  - mixed heights
  - edge offsets (`0`, middle, max)
  - viewport smaller/larger than transcript
- Golden parity tests (toggle on/off) for representative transcripts.
- Scroll stability test when toggling renderer mode at same viewport size.

### TODO

- [x] Add a UI block abstraction (enum or trait object) representing renderable units.
- [x] Define block-level measurement API (`measured_height(width) -> u16`).
- [x] Build a shared layout pass:
  - measure block heights
  - compute cumulative offsets
  - derive visible block range from viewport + scroll offset
- [x] Add adapter blocks that delegate to existing line rendering for now.
- [ ] Keep output identical by rendering adapters through current logic.

### Completion criteria

- [x] New block pipeline runs end-to-end behind toggle.
- [ ] No intentional visual changes.
- [x] Existing rendering tests remain green.

### Progress notes (implementation started)

- Added renderer mode to runtime settings:
  - `src/config/settings.rs` (`ui.renderer_mode`, `UiRendererMode`)
  - `src/config/loader.rs` (`HH_UI_RENDERER_MODE` env override)
- Wired mode into app runtime state:
  - `src/app/state.rs` (`AppState::ui_renderer_mode`)
  - `src/app/mod.rs` (load mode from resolved settings)
- Added block/layout scaffolding modules:
  - `src/app/components/messages_blocks.rs`
  - `src/app/components/messages_layout.rs`
- Hooked `MessagesComponent` scroll slicing to mode switch:
  - `src/app/components/messages.rs`
  - `legacy-lines` keeps existing `hh_widgets::scrollable` path
  - `widget-blocks` uses new block layout pass to compute visible message range
- Added initial tests for layout and block mapping in new modules, plus config load coverage for `ui.renderer_mode`.

## Phase 2 - Message Body Blocks

### Proposed implementation shape

Migrate message body content first because it is the highest-volume transcript surface and least coupled to tool-state edge cases.

- Introduce dedicated body blocks with explicit content kinds:
  - markdown paragraph/list/quote
  - fenced code block
  - plain text fallback
- Keep indentation outside content parsing so wrapping is deterministic:
  - block computes content lines at logical width
  - container applies message indent at render time
- Preserve existing markdown conversion semantics initially by adapting through `markdown_to_lines_with_indent` where needed, then progressively replace adapters.
- Stop emitting migrated body sections into the global `Vec<Line>` path once parity is proven for that block kind.

Suggested block split for assistant text:

```rust
enum AssistantBodyBlock {
    Markdown(MarkdownBlock),
    CodeFence(CodeFenceBlock),
    PlainText(PlainTextBlock),
}
```

### Phase 2 parity checks

- Width-sensitive snapshots at minimum widths: 60, 80, 100, 120.
- Cases to lock:
  - nested list indentation
  - long unbroken token wrapping/truncation behavior
  - fenced code blocks with language labels
  - mixed markdown + plain text paragraphs
- Measurement consistency assertions:
  - `measured_height(width)` equals rendered row count for each body block
  - repeated measure calls at same width are stable (no hidden state)

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

### Proposed implementation shape

Tool transcript segments should be modeled as structured composite blocks so header/status/body can evolve without reintroducing global line coupling.

- Split tool rendering into explicit sub-blocks:
  - tool status/header row (`pending`, `success`, `error`)
  - optional metadata row (counts/duration/click hint)
  - output body block (plain/error/json/diff-ref)
- Keep truncation policy centralized and shared across pending/completed/error paths.
- Preserve existing semantics from:
  - `append_tool_result_count`
  - `task_pending_elapsed_secs` + `task_completed_label`
  - `extract_tool_error_text`
- Ensure tool state transitions are append-only at transcript level (avoid in-place height surprises when possible).

Suggested type shape:

```rust
struct ToolBlock {
    header: ToolHeaderBlock,
    body: Option<ToolBodyBlock>,
}

enum ToolBodyBlock {
    Text(TextBodyBlock),
    Error(ErrorBodyBlock),
    Diff(DiffBodyBlock),
}
```

### Phase 3 test matrix

- Pending -> completed transition at viewport top/middle/bottom.
- Error output extraction from:
  - raw text
  - JSON object with `error`/`message`
  - nested JSON arrays/objects
- Count summary parity for `list`/`glob`/`grep`.
- Long output truncation with deterministic suffix (`...`).
- Mixed transcript ordering:
  - assistant -> tool -> assistant
  - user -> tool pending -> user
  - multiple back-to-back tool calls

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

### Proposed implementation shape

Remove app-side unified-diff parsing from `src/app/render.rs` and make `CodeDiff` the single rendering authority for both layout modes.

- Replace `render_edit_diff_block` side-by-side parsing path (`next_diff_row`, `render_side_by_side_diff_row`, `render_diff_cell`) with a `CodeDiff`-backed block.
- Pass mode + width + theme-derived colors/config into `CodeDiff` once per block render.
- Keep truncation and line-limit policy in one layer only (preferably widget layer).
- Treat app layer as orchestrator:
  - parse tool output envelope (`path`, summary counts, raw diff)
  - render header row
  - delegate diff body fully to `CodeDiff`

### Phase 4 regression locks

- Verify no app-local diff parsing helpers remain referenced in production path.
- Snapshot both modes using same diff fixture set:
  - hunk headers
  - add/remove-only hunks
  - changed pairs
  - file header markers (`---`/`+++`)
- Add invariant tests:
  - measured height includes block padding consistently
  - token rendering does not duplicate adjacent segments (`pubpub` regression)
  - add/remove prefixes retain contrast in configured theme

### TODO

- [x] Replace all app-side diff line consumption with direct `CodeDiff` widget render.
- [x] Route layout mode and frame config from app theme/config into `CodeDiff`.
- [x] Remove duplicate app-side side-by-side diff parsing/render code.
- [x] Keep `CodeDiff` as single source of truth for diff semantics and styling.
- [x] Add/keep regression tests for:
  - hidden `---/+++` rows in side-by-side
  - measured height with padding
  - no duplicated tokens (`pubpub`)
  - add/remove prefix background contrast

### Completion criteria

- [x] App no longer re-implements diff rendering logic.
- [x] All diff rendering goes through `hh-widgets::codediff::CodeDiff`.
- [x] Diff snapshots/parity checks pass.

### Progress notes (in progress)

- Removed app-local side-by-side diff parser/render helpers from `src/app/render.rs`.
- `render_edit_diff_block` now renders diff rows only through `CodeDiff`:
  - chooses `CodeDiffLayout::{SideBySide, Unified}` from width constraints
  - maps app theme colors into `CodeDiffStyles`
  - disables panel padding via `CodeDiffFrame` and applies transcript indent in app layer
- App no longer uses `CodeDiff::rendered_lines()`/`CodeDiffLineKind` for diff rendering path.
- Added regressions in `src/app/render.rs` test module for:
  - side-by-side hidden file header rows (`---`/`+++`)
  - token duplication guard (`pubpub`)
  - unified fallback on narrow widths

## Phase 5 - Container-Level Scrolling Finalization

### Proposed implementation shape

Finalize one global scroll model that depends only on block layout metadata, not on legacy line vectors.

- Scroll state fields at container level:
  - `scroll_offset_rows` (u32)
  - `viewport_height_rows` (u16)
  - `content_height_rows` (u32)
- Recompute `content_height_rows` from block layout each render pass.
- Clamp function is the single authority:
  - `max_offset = content_height_rows.saturating_sub(viewport_height_rows as u32)`
  - `scroll_offset_rows = min(scroll_offset_rows, max_offset)`
- Render pass draws only blocks that intersect viewport.
- Keep keyboard commands mapped to row-level deltas so behavior remains stable across mixed block heights.

Suggested behavior rules:

- **Resize:** clamp scroll offset immediately after new layout is measured.
- **Append while at bottom:** remain pinned to bottom.
- **Append while away from bottom:** preserve top-visible content anchor (no jump).
- **Mode toggle:** preserve equivalent visual position by row offset translation.

### Phase 5 test matrix

- Deterministic scroll math tests:
  - exact-fit content
  - content shorter than viewport
  - content much longer than viewport
- Resize tests:
  - shrink height while mid-scroll
  - expand height near bottom
- Append tests:
  - append when offset == max (stick to bottom)
  - append when offset < max (do not auto-jump)
- Boundary tests with mixed block heights:
  - viewport begins inside tall block
  - viewport ends inside tall block
  - one-line blocks around viewport edge

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

### Proposed implementation shape

Perform removal in narrow, reviewable steps after widget path has been defaulted and validated.

- Removal sequence:
  1. Switch default renderer mode to `widget-blocks`.
  2. Keep legacy path compile-time present for one release window with explicit fallback toggle.
  3. Remove legacy builders and adapters after parity signoff.
  4. Remove fallback toggle and dead config/env branches.
- Keep module boundaries explicit after cleanup:
  - `messages_layout` (measurement + offsets + visible range)
  - `messages_render` (frame drawing from layout metadata)
  - `messages_style` (theme mapping)
- Delete app-local diff parsing helpers once Phase 4 is fully live.

### Phase 6 code deletion checklist

- Remove/replace line-first entry points used only by legacy path.
- Remove temporary adapter blocks that proxy into legacy line functions.
- Remove unused helper functions/constants discovered by compiler and clippy.
- Update architecture docs and inline module docs to match the final block-first model.

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

### Validation protocol

Re-run all Phase 0 baselines and treat changes as regressions unless explicitly justified and documented.

- Artifact parity:
  - regenerate the same tmux capture files
  - compare against committed baselines
  - annotate any intentional differences
- Performance parity:
  - rerun same scenarios, widths, and transcript fixtures
  - compare render latency and memory against Phase 0 table
  - document deltas and rationale if neutral-or-better is not met
- Quality gates (must pass):
  - `cargo test`
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`

### Rollout and fallback playbook

- Rollout stages:
  1. internal default off (opt-in toggle)
  2. internal default on with fallback available
  3. remove fallback after stability window
- Fallback trigger examples:
  - reproducible scroll jitter on resize
  - broken diff readability in common cases
  - sustained performance regression above agreed threshold
- Fallback action:
  - switch renderer mode back to `legacy-lines`
  - capture failing scenario artifact
  - open regression issue with artifact + viewport size + transcript fixture

### Evidence checklist for completion

- Link to updated artifacts directory and baseline comparison notes.
- Link to performance table with before/after values.
- Link to CI run proving quality gates passed.
- Link to rollout notes including fallback criteria and owner.

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

### Validation status (current branch)

- `cargo test`: passing
- `cargo clippy -- -D warnings`: passing
- `cargo fmt --check`: currently fails due to pre-existing formatting diffs in `hh-widgets/*` files not modified by this migration work

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
