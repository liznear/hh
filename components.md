# Reusable Widgets Extraction Plan (`hh-widgets`)

## 1) High-Level Goal

Extract common TUI widgets from `hh` into a reusable crate (`hh-widgets`) that:

- fully supports current `hh` rendering and UX needs,
- has zero dependency on `hh` internals,
- stays simple to adopt in other `ratatui` applications,
- supports nested composition (for example, vertical scrollable areas with heterogeneous child widgets such as markdown and codediff),
- and does not increase `hh` integration complexity.


## 2) Core Principles

1. **`hh`-independent by construction**
   - `hh-widgets` must not import any `hh` module or type.
   - Public APIs accept generic render inputs (text, style/config structs, view models) only.

2. **Parity-first extraction**
   - Preserve current `hh` behavior and visual identity during migration.
   - Avoid feature redesign while extracting.

3. **Simple v1, extensible API**
   - Start with render-focused composition and state.
   - Favor additive API evolution (`#[non_exhaustive]`, options structs, builders) to avoid breaking users.

4. **Composable and nestable widgets**
   - Widgets must render correctly inside arbitrary parent `Rect`s.
   - `Scrollable` must support multiple child widgets of different types.

5. **Single adapter boundary in `hh`**
   - Keep all `hh`-specific mapping in one adapter layer.
   - Prevent glue-code sprawl across rendering modules.

6. **Reversible and testable phases**
   - Each phase must have clear acceptance criteria.
   - Keep compatibility shims until parity is proven.


## 2.1) Ownership and State Model Contract (Recommended)

Use a caller-owned canonical state model across all widgets.

1. **Canonical state is caller-owned**
   - `hh` (or any host app) owns durable widget state across frames (for example: scroll offset, selection, popup open/anchor).
   - `hh-widgets` must not store canonical interaction state internally.

2. **Render is pure and side-effect free**
   - Render APIs consume immutable model/options + caller state and write only to `ratatui` frame output.
   - No hidden mutation, no global/singleton render state.

3. **State transitions are explicit and testable**
   - Widgets may provide pure update/reducer helpers (for example: `scroll_by`, `apply_event`) that mutate caller-provided state only.
   - Helpers must be deterministic and unit-testable without terminal IO.

4. **Ephemeral caches are optional and recomputable**
   - Parsing/layout/measurement caches are allowed only as explicitly passed cache objects.
   - Cache contents are non-canonical; dropping cache must not change correctness, only performance.

5. **Determinism and failure behavior**
   - Same `(model, state, area, theme)` must produce the same rendered output.
   - Malformed markdown/diff input must degrade to deterministic fallback rendering with no panics.


## 2.2) v1 Scope and Explicit Non-Goals

### In Scope for v1

- `markdown` rendering widget (including fenced code blocks and tables with parity-focused defaults)
- `popup` primitives (anchored overlays, bounds clamping, clear/background policy)
- `scrollable` vertical container that can compose heterogeneous children
- `codediff` renderer for unified diff text with safe truncation behavior
- `theme`/style primitives that are generic and host-app configurable
- test harnesses and snapshots needed to preserve `hh` behavior parity

### Out of Scope for v1

- introducing a new runtime, event loop, or input framework
- redesigning `hh` visual language, spacing system, or layout architecture
- introducing async/background rendering pipelines
- building app-specific widgets tied to `hh` business semantics
- forcing a single global theme system across host applications


## 2.3) API Stability and Evolution Rules

To keep initial adoption simple while preserving long-term compatibility:

1. Public config/state structs should be `#[non_exhaustive]` where extension is likely.
2. New capabilities should be additive (new options fields, builders, trait impls) rather than breaking signatures.
3. Internal implementation details (parsers, caches, layout internals) are not API contracts unless explicitly documented.
4. Host-facing behavior changes that impact rendering parity should be introduced behind opt-in options first.
5. Minor releases may add fields/variants; major releases are required for removals or semantic breaking changes.


## 2.4) Dependency and Adapter Boundary Policy

### Crate Dependency Policy (`hh-widgets`)

- Allowed dependencies: generic Rust and terminal UI ecosystem crates (for example, `ratatui`, unicode/text measurement crates, parsing utilities).
- Forbidden dependencies: any `hh` crate/module, `hh` internal domain types, or imports from `src/app/**`.

### `hh` Integration Policy

- `hh` must integrate with `hh-widgets` only through a dedicated adapter namespace.
- Non-adapter `hh` modules must not import `hh-widgets` directly.
- Adapter is the only place where `hh` view/state/theme is mapped to generic widget models/options.

### Enforcement Strategy

- Add compile-time or lint-style checks that fail if forbidden imports appear.
- Keep adapter folder ownership explicit to prevent glue-code sprawl.


## 2.5) Terminal Text Measurement Contract

All widgets must follow a shared width/measurement contract:

1. Width calculations are based on displayed terminal cell width, not byte length.
2. Grapheme cluster boundaries are respected when slicing/wrapping text.
3. Wide characters and zero-width join behavior must be handled deterministically.
4. Tabs must be normalized via a documented tab-stop policy before layout.
5. Wrapping and truncation must be style-aware and deterministic for the same input/options.


## 2.6) Failure-Mode Contract

1. Rendering paths must be panic-free for malformed markdown or diff input.
2. Invalid input degrades to deterministic fallback text rendering.
3. Fallback behavior must preserve frame rendering continuity (no partial-frame aborts).
4. Performance guardrails (line/item limits, truncation) must bound worst-case work.
5. Errors that are not render-fatal should be surfaced as diagnostics/loggable metadata, not crashes.


## 2.7) Baseline Parity Checklist and Golden Artifacts

### Parity Checklist (must match pre-extraction behavior)

- markdown: paragraphs, soft/hard wraps, tables, fenced code blocks, inline emphasis, long-line handling
- popup: anchor positioning, edge clamping, clear/background handling, small-terminal behavior
- scrollable: offset math, viewport slicing, resize stability, mixed-child composition
- codediff: file headers, hunk headers, context/add/remove styling, truncation and malformed input fallback

### Golden Baseline Artifacts

Before extraction changes land:

- capture parity-critical snapshots and/or tmux-captured terminal outputs from `cargo run -- run ...`
- store artifacts in a stable, versioned test location
- reference artifact scenarios directly from parity tests
- require parity check pass before removing compatibility shims

Captured baseline set (2026-03-15):

- `docs/artifacts/phase0-baselines/tmux-simple.txt`
- `docs/artifacts/phase0-baselines/tmux-markdown.txt`
- `docs/artifacts/phase0-baselines/tmux-diff.txt`


## 3) Detailed Phases

## 3.0) Execution Status

- **Current phase:** Phase 1 (Crate Scaffolding and Public Surface Skeleton)
- **Completed phases:** Phase 0
- **Last completed milestone (2026-03-15):** Scope/contracts finalized and tmux-based golden baseline captures stored in `docs/artifacts/phase0-baselines/`.
- **Working rule:** Until parity is proven in later phases, keep compatibility shims and avoid behavior redesign.

---

## Phase 0 - Scope, Contracts, and Guardrails

### Goal

Define boundaries, compatibility expectations, and migration constraints before code movement.

### Fine-grained TODO Items

- [x] Confirm extraction scope for v1 (`markdown`, `popup`, `scrollable`, `codediff`).
- [x] Define explicit out-of-scope items for v1 (no new runtime/event framework; no redesign of `hh` theme/layout).
- [x] Document API stability rules (`#[non_exhaustive]` on config/state structs, additive options/builders, no hard-contract internals).
- [x] Define crate-level dependency policy (allowed: generic UI/render crates; forbidden: `hh` crate/module dependencies).
- [x] Define `hh` integration rule (all calls to `hh-widgets` go through a dedicated adapter folder).
- [x] Define API ownership model for v1 (which state is caller-owned vs widget-owned; render-only vs interactive state updates).
- [x] Define terminal text measurement contract (grapheme segmentation, wide characters, tabs, wrapping, and style-aware width accounting).
- [x] Define failure-mode contract (panic-free rendering; deterministic fallback for malformed markdown/diff input).
- [x] Create baseline parity checklist from current behavior (markdown tables/fences/wrap, popup placement, message scrolling/selection visuals, diff style/truncation).
- [x] Capture golden baseline artifacts before extraction (tmux-captured `cargo run -- run ...` outputs for parity-critical scenarios).

### Progress Notes

- 2026-03-15: Phase 0 documentation contracts were added in sections `2.2` through `2.7`.
- 2026-03-15: Golden baseline artifacts were captured via tmux using `cargo run -- run ...` and stored under `docs/artifacts/phase0-baselines/`.

### Completion Criteria

- Scope and out-of-scope are documented in this plan.
- API stability constraints are documented and agreed.
- Dependency and adapter rules are documented and enforceable.
- API ownership, text measurement, and failure-mode contracts are documented.
- A parity checklist exists and is used in later phases.
- Golden baseline artifacts exist and are referenced by parity checks.

### Phase Outcome

Phase 0 is complete as of 2026-03-15.

---

## Phase 1 - Crate Scaffolding and Public Surface Skeleton

### Goal

Create `hh-widgets` crate structure with a minimal, future-safe public API skeleton.

### Proposed Crate Layout (v1 Skeleton)

```text
hh-widgets/
  Cargo.toml
  src/
    lib.rs
    widget.rs
    theme.rs
    markdown.rs
    popup.rs
    scrollable.rs
    codediff.rs
```

Notes:
- Keep v1 flat and explicit; avoid premature nested module complexity.
- If a module grows, split internally later without breaking public re-exports.

### Proposed Public Surface Contracts (v1)

`widget` module should define crate-wide composition contracts:

- `WidgetNode` (enum or trait-object wrapper) for heterogeneous child composition.
- `Measure` capability to compute required height for a given width/options.
- `Render` capability that draws into caller-provided `Rect` + frame context.
- `RenderCtx` carrying theme/style references and optional ephemeral caches.

`scrollable` should consume heterogeneous child nodes via `WidgetNode` and provide:

- caller-owned `ScrollableState` (offset, viewport, optional anchoring state)
- pure state helpers (`scroll_by`, `scroll_to_end`, `clamp_offset`)
- deterministic viewport slicing from `(children, state, area.width, area.height)`

`markdown`, `popup`, and `codediff` should each expose:

- options structs (`MarkdownOptions`, `PopupOptions`, `CodeDiffOptions`)
- pure render entry points that accept caller-owned state/options
- optional explicit cache objects for performance-only memoization

### State Transition Contract (Interactive Widgets)

For any interactive widget state type in v1:

1. State mutation occurs only through explicit API calls on caller-owned state.
2. Equivalent input events from equivalent prior state produce equivalent next state.
3. `render` is idempotent with respect to state (no hidden updates).
4. Resize-aware clamping is explicit and test-covered (for example, offset clamped when viewport shrinks).
5. Helper APIs return enough metadata to support host-side policy (for example, whether offset changed).

### Dependency Guardrail Plan (Phase 1)

- Add a crate-level check that fails if `hh-widgets` imports `crate::app`, `crate::core`, or any `hh`-specific path.
- Add workspace CI check to enforce `hh-widgets` dependency independence.
- Keep adapter-only integration policy enforced in later Phase 6 (`src/app/widgets_adapter/`).

### Fine-grained TODO Items

- [x] Add `./hh-widgets/` as a workspace member.
- [x] Add module structure (`widget`, `markdown`, `popup`, `scrollable`, `codediff`, `theme`/style primitives).
- [x] Define core composition contracts for v1 (widget node type(s), sizing/measurement interface, render interface with explicit context/state).
- [x] Add public options/state types using non-breaking patterns.
- [x] Document state transition contract for interactive widgets (for example, scroll state update API, idempotent render behavior).
- [x] Add basic crate docs with a simple usage example.
- [x] Add guardrails/tests to ensure no `hh` dependency appears in crate imports.

### Phase 1 Immediate Implementation Order

1. Add workspace member and minimal crate scaffold.
2. Define public module tree and empty public surface (`lib.rs` + module exports).
3. Introduce core composition contracts and minimal options/state structs using additive patterns.
4. Add crate docs + compile-only usage example.
5. Add dependency-boundary guardrail test/check and run repository quality gates.

### Progress Notes

- 2026-03-15: Phase 1 API and layout design draft captured in this plan (contracts, module layout, and guardrail strategy) before code scaffolding.
- 2026-03-15: Implemented `hh-widgets` scaffold with workspace membership and module skeleton (`lib.rs`, `widget`, `theme`, `markdown`, `popup`, `scrollable`, `codediff`).
- 2026-03-15: Added initial additive public types (`#[non_exhaustive]` options/state models, `RenderCtx`, `Measure`/`Render`, `WidgetNode`, and `ScrollableState` pure helpers).
- 2026-03-15: Added crate-level docs/example and dependency guardrail test (`hh-widgets/tests/no_hh_dependency.rs`).
- 2026-03-15: Verified quality gates for scaffold stage: `cargo check`, `cargo test -p hh-widgets`, `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`.

### Next Action Queue (Phase 1)

1. Create `hh-widgets` crate and add it to workspace membership.
2. Add `lib.rs` and module files (`widget`, `theme`, `markdown`, `popup`, `scrollable`, `codediff`) with compile-safe placeholders.
3. Define initial public contracts/types (`WidgetNode`, `RenderCtx`, `ScrollableState`, options structs) behind additive, non-breaking shapes.
4. Add crate docs + one compile-only usage snippet.
5. Add a boundary guardrail check (import/dependency validation) and run standard quality gates.

### Completion Criteria

- `hh-widgets` compiles independently.
- Public module/API skeleton exists and is documented.
- Zero `hh` dependency in crate graph/imports.
- Base docs and compile tests pass.

### Phase Outcome

Phase 1 is complete as of 2026-03-15.

---

## Phase 2 - Markdown Widget Extraction

### Goal

Extract markdown rendering into `hh-widgets` while preserving current behavior and style defaults.

### Fine-grained TODO Items

- [x] Move markdown parsing/rendering logic into `hh-widgets::markdown`.
- [x] Define `MarkdownOptions` for rendering controls (wrapping, code block style, table style).
- [x] Keep code-fence highlighting and table rendering behavior compatible with current `hh` output.
- [x] Port or recreate current markdown-focused tests in `hh-widgets`.
- [x] Add a thin compatibility wrapper in `hh` that delegates markdown rendering to `hh-widgets`.

### Progress Notes

- 2026-03-15: Extracted markdown renderer implementation from `src/theme/markdown.rs` into `hh-widgets/src/markdown.rs`.
- 2026-03-15: Added `hh-widgets` markdown dependencies (`ratatui`, `syntect`, `syntect-tui`) and retained behavior-focused markdown test coverage in the new crate.
- 2026-03-15: Added `hh-widgets` as a root dependency and replaced `hh` markdown module with a thin compatibility re-export wrapper.
- 2026-03-15: Validation complete for extraction checkpoint via `cargo check`, `cargo test -p hh-widgets`, `cargo test`, `cargo fmt --check`, and `cargo clippy -- -D warnings`.

### Completion Criteria

- `hh` uses `hh-widgets` markdown path in main render flow.
- Existing markdown behavior in `hh` remains visually/functionally equivalent.
- Markdown unit tests pass in `hh-widgets`.
- `cargo check`, `cargo test`, `cargo fmt --check`, `cargo clippy -- -D warnings` pass.

### Phase Outcome

Phase 2 is complete as of 2026-03-15.

---

## Phase 3 - Scrollable Container with Nested Children

### Goal

Implement a vertical `Scrollable` widget that composes multiple heterogeneous child widgets.

### Fine-grained TODO Items

- [x] Implement scrollable state model (offset, viewport height, optional auto-follow/anchoring hooks).
- [x] Implement child composition model (multiple children with mixed types such as markdown, codediff, spacer).
- [x] Implement measurement pipeline (compute child heights for current width and maintain cumulative height index).
- [x] Implement virtualization (map offset to visible children and render only visible ranges).
- [x] Add nested-container safety checks (parent/child bounds correctness; no full-screen assumptions).
- [x] Add tests for mixed-children visibility, offset math, and resize behavior.
- [x] Add performance tests/benchmarks for long transcripts (for example, thousands of child blocks) with explicit render-time and allocation guardrails.

### Progress Notes

- 2026-03-15: Implemented `ScrollableState` with explicit caller-owned fields (`offset`, `viewport_height`, `auto_follow`) and deterministic pure helpers (`scroll_by`, `scroll_to_end`, `clamp_offset`).
- 2026-03-15: Implemented heterogeneous child measurement for `WidgetNode` (`Markdown`, `CodeDiff`, `Spacer`) and cumulative `ScrollLayout` indexing.
- 2026-03-15: Implemented viewport virtualization primitives (`visible_range`, `visible_children`) based on measured child geometry and scroll offset.
- 2026-03-15: Added scrollable tests for offset/auto-follow transitions, cumulative measurement invariants, visibility slicing, and viewport/max-offset resize math.
- 2026-03-15: Validation for this checkpoint passed via `cargo test -p hh-widgets`, `cargo check`, `cargo fmt --check`, and `cargo clippy --workspace -- -D warnings`.
- 2026-03-15: Added `hh-widgets/benches/scrollable_perf_probe.rs` with explicit guardrails and enforce mode (`--enforce`) for long mixed-child workloads.
- 2026-03-15: Benchmark probe validated at `children=4000`, `width=96`, `height=30`, `iterations=80` with `p95=1.704ms`, `max=2.183ms`, `p95_alloc=5177KB`, `max_alloc=5177KB` under configured guardrails.

### Completion Criteria

- Scrollable container renders mixed child lists correctly.
- Nested composition works inside arbitrary parent `Rect`s.
- Scroll math and viewport slicing are validated by tests.
- Behavior can be consumed by `hh` without additional runtime complexity.

### Phase Outcome

Phase 3 is complete as of 2026-03-15.

---

## Phase 4 - Popup Primitive Extraction

### Goal

Extract reusable popup primitives that support anchored overlays and embedding in arbitrary layouts.

### Fine-grained TODO Items

- [x] Create generic popup API (anchor strategy, bounds clamping, clear/background policy, content widget rendering).
- [x] Port clipboard notice and command-palette-like popup mechanics as generic capabilities.
- [x] Keep app-specific decisions (for example, palette item semantics) in `hh` adapter code.
- [x] Add geometry tests for popup placement near edges and small terminals.

### Progress Notes

- 2026-03-15: Expanded `hh-widgets::popup` with generic anchor-based geometry request types (`PopupRequest`, `Offset`) and deterministic placement API (`popup_from_request`, `clamp_popup`).
- 2026-03-15: Added popup geometry unit tests covering viewport clamping, edge anchoring, small-terminal fitting, and offset behavior.
- 2026-03-15: Integrated `hh` popup rendering with `hh-widgets` geometry helpers for clipboard notice and command palette placement while keeping palette content semantics local to `hh`.
- 2026-03-15: Validation complete via `cargo test -p hh-widgets`, `cargo check`, `cargo fmt --check`, and `cargo clippy --workspace -- -D warnings`.

### Completion Criteria

- Popup primitives are reusable without `hh` types.
- `hh` popup rendering uses crate primitives with parity.
- Geometry behavior is covered by tests.

### Phase Outcome

Phase 4 is complete as of 2026-03-15.

---

## Phase 5 - Codediff Widget

### Goal

Provide a reusable codediff renderer that accepts generic diff input and matches `hh` style expectations.

### Fine-grained TODO Items

- [x] Implement `codediff` widget API for unified diff text and optional metadata.
- [x] Add style options for added/removed/meta lines and truncation limits.
- [x] Implement parsing/rendering for file headers, hunk headers, and context/add/remove lines.
- [x] Mirror current `hh` truncation/safety behavior as configurable defaults.
- [x] Add tests for line classification and rendering edge cases.
- [x] Add malformed-input tests (invalid hunks, partial headers) with deterministic fallback rendering and no panics.
- [x] Add large-diff performance checks to validate truncation and rendering cost stays bounded.

### Progress Notes

- 2026-03-15: Expanded `hh-widgets::codediff` from placeholder types to a reusable unified-diff render pipeline (`render_unified_diff`) with typed output (`CodeDiffRender`, `CodeDiffLine`, `CodeDiffLineKind`).
- 2026-03-15: Added configurable truncation controls in `CodeDiffOptions` (`max_rendered_lines`, `max_rendered_chars`) and aligned defaults with existing `hh` safety behavior.
- 2026-03-15: Added line classification and malformed-input fallback coverage in `hh-widgets` tests (file headers, hunk headers, add/remove/context, partial/invalid forms).
- 2026-03-15: Integrated `hh` single-column diff rendering path with `hh-widgets::codediff` while preserving existing color/theme mapping in `hh`.
- 2026-03-15: Validation complete for this checkpoint via `cargo test -p hh-widgets`, `cargo test`, `cargo fmt --check`, and `cargo clippy --workspace -- -D warnings`.
- 2026-03-15: Added `hh-widgets/benches/codediff_perf_probe.rs` with enforceable performance/allocation guardrails for large synthetic diff workloads.
- 2026-03-15: Codediff benchmark probe validated (`files=120`, `hunks=3`, `lines=24`, `iterations=100`) with `p95=0.004ms`, `max=0.011ms`, `p95_alloc=9.8KB`, `max_alloc=9.8KB` under enforce mode.

### Completion Criteria

- Codediff widget renders expected diff semantics in `hh`.
- Codediff can be used as a child in scrollable composition.
- Tests validate parsing/rendering and truncation behavior.

### Phase Outcome

Phase 5 is complete as of 2026-03-15.

---

## Phase 6 - `hh` Adapter Integration and Simplification

### Goal

Integrate all extracted widgets through a single `hh` adapter layer and reduce local rendering duplication.

### Fine-grained TODO Items

- [x] Create adapter namespace (for example, `src/app/widgets_adapter/`).
- [x] Map `hh` state/view data to generic widget node trees.
- [x] Map `hh` theme tokens/layout to `hh-widgets` style options.
- [x] Replace direct render logic in `hh` modules with adapter invocations.
- [x] Keep compatibility shims while migrating call sites incrementally.
- [x] Add an enforceable boundary check so non-adapter `hh` modules do not import `hh-widgets` directly.
- [x] Remove obsolete duplicated render utilities after parity verification.

### Progress Notes

- 2026-03-15: Added adapter namespace at `src/app/widgets_adapter/` with focused modules: `markdown`, `codediff`, and `popup`.
- 2026-03-15: Routed markdown/codediff/popup usage in app rendering through adapter functions/re-exports, and removed the old `src/theme/markdown.rs` compatibility shim.
- 2026-03-15: Added enforceable boundary test `tests/widgets_adapter_boundary_tests.rs` to fail on direct `hh_widgets::` imports in non-adapter app modules.
- 2026-03-15: Validation for current Phase 6 checkpoint passed via `cargo test --test widgets_adapter_boundary_tests`, `cargo test`, `cargo fmt --check`, and `cargo clippy --workspace -- -D warnings`.
- 2026-03-15: Added `widgets_adapter::view_model` mapping from `ChatMessage` to generic `WidgetNode` collections (assistant/compaction/thinking/error -> markdown, successful edit/write tool result -> codediff).
- 2026-03-15: Added adapter mapping tests in `src/app/widgets_adapter/view_model.rs` and wired mapping call into message render path for incremental migration visibility.
- 2026-03-15: Added `widgets_adapter::theme::AdapterTheme` to centralize mapping from `UiLayout` + theme constants into adapter-owned widget option tokens (diff truncation limits, padding/indent metadata).
- 2026-03-15: Updated markdown/codediff/popup adapter entry points to accept/use `AdapterTheme`, so layout/style token mapping now happens in one adapter layer instead of scattered call sites.
- 2026-03-15: Added adapter theme mapping unit test (`maps_ui_layout_into_adapter_theme_tokens`) and revalidated all quality gates.
- 2026-03-15: Completed shim migration by deleting obsolete `parse_markdown_lines_unindented` duplicate helper and routing the thinking block through the shared adapter markdown entry point.
- 2026-03-15: At this checkpoint, compatibility shim posture is explicit: adapter layer remains as the only `hh-widgets` boundary; removed utilities are only those with no remaining call sites and parity-safe replacements.
- 2026-03-15: Integrated `hh-widgets` scrollable measurement/virtualization into runtime message rendering via `src/app/widgets_adapter/scrollable.rs` and `src/app/components/messages.rs`, so `ScrollLayout` now participates in live viewport slicing (not just tests/bench).

### Completion Criteria

- All widget usage in `hh` routes through the adapter boundary.
- Legacy duplicate rendering paths are removed or clearly deprecated.
- `hh` complexity is flat or reduced (no scattered glue growth).

### Phase Outcome

Phase 6 is complete as of 2026-03-15.

---

## Phase 7 - Hardening, Documentation, and Reuse Validation

### Goal

Prove stability and reusability for external projects while preserving `hh` behavior.

### Fine-grained TODO Items

- [x] Add crate-level docs and examples (standalone markdown widget, scrollable with mixed children, popup usage, codediff usage).
- [x] Add integration/snapshot tests for representative composed layouts.
- [x] Validate that widgets can render inside a non-`hh` ratatui component tree.
- [x] Run full repository checks and resolve regressions.
- [x] Document versioning and non-breaking extension policy.
- [x] Document SemVer and MSRV policy, plus publish strategy (workspace-only vs crates.io) for `hh-widgets`.
- [x] Remove compatibility shims only after parity + performance gates pass.

### Progress Notes

- 2026-03-15: Added crate-level usage docs and examples in `hh-widgets/README.md` covering standalone markdown rendering, mixed-child scrollable composition, popup geometry, and codediff rendering.
- 2026-03-15: Expanded crate rustdoc examples in `hh-widgets/src/lib.rs` with a composition scenario that combines markdown, codediff, and scrollable APIs.
- 2026-03-15: Added composed integration tests in `hh-widgets/tests/composed_layouts.rs` for deterministic composed layouts, codediff snapshot-shape stability (`MMMRAC`), and non-`hh` runtime usage.
- 2026-03-15: Validated Phase 7 checkpoint with `cargo test -p hh-widgets`, doctests, `cargo check`, `cargo fmt --check`, and `cargo clippy --workspace -- -D warnings`.
- 2026-03-15: Ran full repository verification (`cargo check`, `cargo build`, `cargo test`, `cargo fmt --check`, `cargo clippy -- -D warnings`) with all gates passing.
- 2026-03-15: Documented public API evolution policy and SemVer/MSRV/publish strategy in `hh-widgets/README.md`.
- 2026-03-15: Compatibility shims are now minimized to adapter-owned boundaries only, with obsolete duplicate helpers removed after parity/performance gates passed.

### Completion Criteria

- `hh-widgets` is demonstrably reusable outside `hh`.
- `hh` behavior remains aligned with baseline parity checklist.
- Documentation covers core extension points and usage patterns.
- All quality gates pass.

### Phase Outcome

Phase 7 is complete as of 2026-03-15.

---

## Release Readiness Checklist (Post-Phase)

- [x] No `hh` dependency in `hh-widgets` (imports and dependency tree).
- [x] Nested scrollable composition works with mixed children.
- [x] `hh` integrates through a single adapter boundary.
- [x] Public API follows additive, non-breaking extension patterns.
- [x] Behavior parity validated for markdown, popup, scrollable, and codediff.
- [x] Text measurement and malformed-input fallback behavior validated against contracts.
- [x] Performance guardrails validated for large scrollable and codediff workloads.
- [x] Adapter boundary enforcement prevents direct imports outside `hh` adapter modules.
- [x] SemVer/MSRV/publish policy documented and applied.
- [x] Full CI commands pass (`cargo check`, `cargo test`, `cargo fmt --check`, `cargo clippy -- -D warnings`).

## Verification Evidence (2026-03-15)

### Baseline and Parity Artifacts

- `docs/artifacts/phase0-baselines/tmux-simple.txt`
- `docs/artifacts/phase0-baselines/tmux-markdown.txt`
- `docs/artifacts/phase0-baselines/tmux-diff.txt`

### Benchmark/Guardrail Artifacts

- `hh-widgets/benches/scrollable_perf_probe.rs` (guardrail probe with `--enforce` mode)
- `hh-widgets/benches/codediff_perf_probe.rs` (guardrail probe with `--enforce` mode)

### Boundary and Composition Tests

- `tests/widgets_adapter_boundary_tests.rs` (adapter import boundary enforcement)
- `hh-widgets/tests/composed_layouts.rs` (composed deterministic behavior + snapshot-shape assertions)
- `hh-widgets/tests/no_hh_dependency.rs` (`hh-widgets` independence guardrail)

### Documentation Artifacts

- `hh-widgets/README.md` (examples, versioning policy, SemVer/MSRV/publish strategy)
- `hh-widgets/src/lib.rs` rustdoc examples (composition patterns)

### Final Quality Gate Command Set

- `cargo check`
- `cargo build`
- `cargo test`
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
