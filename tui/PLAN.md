# TUI Rendering Refactor Plan

## Goals

1. Preserve current behavior exactly.
2. Make rendering code maintainable by:
   - centralizing layout configuration and offsets,
   - passing theme objects (not individual colors),
   - extracting readable widget renderers.
3. Remove unneeded code paths once replacements are covered by tests.

## Constraints and Invariants

- Keep all user-visible behavior unchanged:
  - message rendering and markdown behavior,
  - scrolling semantics (`up/down`, `pgup/pgdown`, mouse wheel, `home/end`),
  - auto-scroll behavior,
  - sidebar visibility threshold,
  - status line behavior,
  - tool call line behavior and symbols.
- Keep performance characteristics of existing caches:
  - item render cache invalidation by item signature,
  - markdown cache keyed by width + content,
  - no caching for pending tool calls.
- Prefer additive, reversible steps. Each phase should compile and pass tests before continuing.

## High-Level Design

### 1) Single Source of Truth for Layout

Introduce a derived `layoutState` computed once from terminal size and shared by both `View()` and `syncLayout()`.

`layoutState` should include all geometry used in rendering and input sizing, for example:

- `outerWidth`, `outerHeight`
- `innerWidth`, `innerHeight`
- `showSidebar`
- `mainWidth`, `sidebarWidth`
- `messageHeight`, `inputHeight`
- `inputBoxWidth`, `inputTextWidth`

Also centralize logically-equivalent offsets into named constants (instead of scattered `-2`, `-4`, etc.), for example:

- app padding contribution,
- input border/padding horizontal overhead,
- message indent width.

### 2) Widget View-Models (Derived, Read-Only)

Keep `model` as the sole mutable state owner. Build per-frame widget models at render time.

- `frameViewModel` is computed near the start of `View()`.
- Each widget gets only the state it needs.
- Widget render functions are pure string renderers (input -> string), no mutation.

Suggested widget models:

- `statusWidgetModel`
- `inputWidgetModel`
- `sidebarWidgetModel`
- `messageListWidgetModel`
- item widget models as needed (`userMessageWidgetModel`, etc.)

### 3) Theme Plumbing (No Per-Color Parameters)

Use one semantic theme object directly in widget renderers.

- Keep `tui.Theme` as the canonical theme source.
- Add a widget-facing theme view-model (if needed) inside `tui` to avoid passing individual colors.
- Do status/tool call rendering directly in widget renderer functions, not via extra `components.Render...` indirection.

### 4) Extract Widget Renderers With Clear Names

Break `View()` composition into named functions. Suggested names:

- `renderMainPane(...)`
- `renderMessagePane(...)`
- `renderInputPane(...)`
- `renderSidebarPane(...)`
- `renderRootFrame(...)`

Break item rendering dispatcher into explicit renderers:

- `renderUserMessageWidget(...)`
- `renderAssistantMessageWidget(...)`
- `renderThinkingWidget(...)`
- `renderToolCallWidget(...)`
- `renderErrorWidget(...)`

Keep `renderItemLines(...)` as dispatcher + caching wrapper.

Implementation style rule:

- Single-layer rendering for app widgets (`renderXWidget` does final rendering directly).
- No two-layer wrapper pattern like `renderXWidget -> components.RenderX` unless reuse is explicitly needed later.

### 5) Remove Unneeded Code

After new paths are in place and tests are updated, remove dead or duplicate helpers in `tui/app.go`:

- `formatSessionForViewport(...)`
- `formatSessionRaw()`
- `formatToolCallBody(...)` (app-level duplicate)
- `formatToolCallBodyRaw(...)`
- `formatPerfStats` if still unused

## Proposed File Structure

Keep package boundaries stable, but split by responsibility.

- `tui/app.go`
  - `model` type, constructor/init, `Update`, event handling, persistence.
- `tui/app_layout.go`
  - `layoutState`, `computeLayout(...)`, pane height calculation, layout constants/offsets.
- `tui/app_view_model.go`
  - frame/widget view-model definitions and builders.
- `tui/app_view.go`
  - `View()` orchestration and top-level pane composition.
- `tui/app_widgets.go`
  - status/input/sidebar/message-pane renderers (direct rendering).
- `tui/app_item_widgets.go`
  - item-type widget renderers used by `renderItemLines`, including tool call rendering.
- `tui/app_scroll.go`
  - list rendering and scroll offset logic.
- `tui/app_markdown_cache.go`
  - markdown renderer/cache and item render cache helpers.
- existing `tui/components/status_line.go`, `tui/components/tool_line.go`
  - migrate logic into `tui` widget renderers, then remove these files if no shared utility remains.

Note: If this split feels too large for one change, perform incrementally while keeping behavior/tests stable.

## Step-by-Step Execution Plan

### Phase 0: Baseline and Safety

1. Run `go test ./tui/...` to capture baseline.
2. Ensure refactor is done in small commits (or checkpoints) per phase.

### Phase 1: Layout Consolidation

1. Add `layoutState` and `computeLayout(width, height int)`.
2. Replace duplicated layout math in `View()` and `syncLayout()` with `layoutState` fields.
3. Replace repeated numeric offsets with named constants.
4. Keep rendered output unchanged.

Validation:

- `go test ./tui/...` passes.
- Manual smoke check: resize terminal and verify sidebar threshold + input sizing unchanged.

### Phase 2: Theme Object for Widgets

1. Define a single semantic theme input for widget renderers (from `tui.Theme`).
2. Update status/tool call paths to consume theme object, not individual color arguments.
3. Implement status/tool call rendering directly in widget renderers.
4. Keep behavior and symbols exactly the same.

Validation:

- `go test ./tui/...` passes.
- Visual check: status and tool call colors remain consistent.

### Phase 3: Widget Model + Widget Renderers

1. Add frame/widget view-model types and a builder (`buildFrameViewModel`).
2. Extract pane renderers from `View()` with readable names.
3. Extract item renderers from `renderItemLines(...)`.
4. Keep cache boundaries and normalization behavior identical.

Validation:

- `go test ./tui/...` passes.
- Manual scroll smoke check (keyboard + mouse).

### Phase 4: Remove Unneeded Code and Final Cleanup

1. Delete dead formatting helpers after confirming no references.
2. Update tests that depended on removed helpers to use active render path APIs.
3. Remove `tui/components/status_line.go` and `tui/components/tool_line.go` if fully migrated and unused.
4. Run formatting/lint if configured, then run tests again.

Validation:

- `go test ./tui/...` passes.
- No unused code warnings for removed helpers.

## Test Plan

Automated:

- Existing:
  - `tui/app_list_render_test.go`
  - `tui/app_markdown_test.go`
  - `tui/app_item_cache_test.go` (update to use active render path if needed)
- Add:
  - `tui/app_layout_test.go` for `computeLayout` invariants and edge sizes.
  - Optional targeted tests for status/tool renderers under busy/success/idle cases.

Manual checks:

1. Start app and submit prompt; confirm stream, spinner, stopwatch, and final success marker.
2. Scroll during stream and verify auto-scroll toggling behavior.
3. Resize window across sidebar threshold and verify stable layout.
4. Verify tool call lines and thinking block rendering.

## Acceptance Criteria

- Layout values are computed once per frame by `computeLayout` and reused.
- No scattered duplicate layout offsets for logically same concepts.
- Widget rendering functions accept theme objects, not color lists.
- Widget renderers are extracted with readable names.
- Status line and tool call rendering follow the same single-layer widget style.
- Unneeded/dead rendering helpers are removed.
- Behavior and tests remain green.

## Risk and Rollback Strategy

- Perform phased refactor; each phase should be independently reversible.
- Keep logic moves mechanical first (extract, then simplify).
- If any visual regression appears, compare frame output at fixed width/height and revert only the offending phase.
