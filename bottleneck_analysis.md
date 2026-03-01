# hh TUI Performance Bottleneck Analysis

## Context

Observed symptom: when chat history becomes large, `hh` becomes noticeably laggy, including delayed or unresponsive typing.

This analysis focuses on the interactive TUI path and identifies likely bottlenecks, impact, and implementation-ready remediation ideas.

---

## High-Confidence Bottlenecks

### 1) Constant full redraw loop (even when idle)

- **Location:** `src/cli/chat.rs:782`, `src/cli/chat.rs:790`, `src/cli/chat.rs:88`
- **What happens:**
  - Main loop calls `tui_guard.get().draw(|f| tui::render_app(f, app))?;` every iteration.
  - Input polling timeout is `16ms`, effectively driving frequent redraw attempts.
  - Redraw is not gated by a `needs_redraw` or equivalent render-dirty condition.
- **Why it hurts:**
  - CPU is consumed rendering the full screen continuously, regardless of state changes.
  - Cost scales with message history size because render path still walks/copies large structures.
- **User-visible effect:**
  - Keystroke handling competes with render work; typing feels laggy under large history.

---

### 2) Per-frame clone of full message line buffer

- **Location:** `src/cli/tui/ui.rs:434-445`
- **What happens:**
  - `lines = app.get_lines(wrap_width)` returns cached lines.
  - Then render path clones all lines via `let mut rendered_lines = lines.to_vec();`
  - Selection highlighting is applied to this clone.
- **Why it hurts:**
  - Large `Vec<Line<'static>>` clone each frame is expensive with long history.
  - Clone happens even when no text selection exists.
- **User-visible effect:**
  - Baseline redraw cost increases sharply with history length.

---

### 3) Sidebar recomputation every frame, including O(messages) scans + JSON parsing

- **Location:** `src/cli/tui/ui.rs:315-333`, `src/cli/tui/ui.rs:383`, `src/cli/tui/ui.rs:1810`, `src/cli/tui/ui.rs:1846`
- **What happens:**
  - `render_sidebar` calls `build_sidebar_lines` on every draw.
  - `build_sidebar_lines` calls:
    - `app.context_usage()` (`src/cli/tui/app.rs:855`) — scans messages when provider token count absent.
    - `collect_modified_files(&app.messages)` — iterates all messages.
    - `parse_modified_file_summary(output)` — `serde_json::from_str` repeatedly for tool outputs.
- **Why it hurts:**
  - Repeated full-history traversal and parse work during rendering.
- **User-visible effect:**
  - Extra frame-time overhead independent of active typing/streaming state.

---

### 4) Full message-line rebuild on dirty state with large history

- **Location:** `src/cli/tui/app.rs:825-835`, `src/cli/tui/ui.rs:550-653`
- **What happens:**
  - Cached lines rebuild when `needs_rebuild` or width changes.
  - Rebuild processes all messages and markdown formatting/wrapping.
- **Why it hurts:**
  - Dirty events are frequent during assistant/tool streaming (`src/cli/tui/app.rs:449`, tool events).
  - Rebuild cost grows with total chat history size.
- **User-visible effect:**
  - During active responses, interactivity degrades as history grows.

---

### 5) Event processing fairness under load (single event per select wake)

- **Location:** `src/cli/chat.rs:855-861`
- **What happens:**
  - One event from `event_rx.recv()` is handled per `select!` wake.
  - No explicit burst-drain loop for queued app events.
- **Why it hurts:**
  - If many assistant/tool events queue quickly, input handling can lag behind.
- **User-visible effect:**
  - Perceived delayed typing and stale UI updates during heavy streaming/tool output.

---

## Secondary/Contributing Costs

### Input rendering helpers do repeated string/char work

- **Location:** `src/cli/tui/ui.rs:1420+` (`wrap_input_lines`, `cursor_visual_position`, etc.)
- **Notes:**
  - These are mostly bounded by input size and not the primary history-related bottleneck.
  - Can still contribute when multiline input is large.

### Markdown and code highlight rendering can be expensive on rebuild

- **Location:** `src/cli/tui/markdown.rs`
- **Notes:**
  - This cost is mostly paid on message-line rebuilds.
  - Not the root cause alone, but amplified by frequent dirty rebuilds.

---

## Prioritized Remediation Plan

### P0 (highest impact, lowest risk)

1. **Dirty-driven rendering**
   - Add a `needs_redraw`/render invalidation flag.
   - Draw only when:
     - app state changed, or
     - terminal resize/focus/refresh event, or
     - periodic UI element truly requires ticking.
   - Keep periodic tick minimal and avoid unconditional full draw.

2. **Selection fast-path**
   - In `render_messages`, if no active selection:
     - render cached lines directly without cloning.
   - Only clone/apply highlight when selection exists.

3. **Sidebar data caching**
   - Cache computed sidebar model in app state:
     - context usage (or last-known token count),
     - modified file summaries.
   - Recompute only on relevant events:
     - message append/update,
     - tool completion outputs,
     - model/context changes.

### P1 (high impact, moderate complexity)

4. **Coalesce assistant deltas**
   - Buffer multiple small `AssistantDelta` updates and flush at fixed cadence (e.g., every 30-60ms) or chunk threshold.
   - Reduces `mark_dirty` + rebuild frequency during streaming.

5. **Drain event bursts**
   - After receiving one `event_rx` item, opportunistically drain remaining queued events (`try_recv`) up to a cap.
   - Apply all updates before deciding render.

### P2 (optional optimizations)

6. **Incremental line-cache updates**
   - For append-only cases, avoid rebuilding entire `cached_lines`.
   - Append or patch affected tail lines only.
   - More complex; defer until P0/P1 measured.

7. **Memoize parsed tool output for modified-files sidebar**
   - Parse once at tool-end and store normalized summary in message/event state.

---

## Suggested Implementation Steps (handoff-ready)

1. Introduce render invalidation state in chat loop/app state.
2. Gate `draw` on invalidation/tick conditions.
3. Add selection fast-path in `render_messages`.
4. Add sidebar cache struct to `ChatApp` and update hooks in `handle_event`.
5. Batch-drain `event_rx` (bounded).
6. Optional: introduce assistant-delta coalescer.
7. Add lightweight instrumentation (frame time + rebuild counters).
8. Validate with synthetic large-history session.

---

## Validation Plan

- **Functional:**
  - No regression in scrolling, selection, command palette, sidebar correctness.
- **Performance metrics to collect:**
  - Mean/p95 frame render duration.
  - Count of full message-line rebuilds per second.
  - Input-to-visual latency under large history.
- **Test scenario:**
  - Session with thousands of messages and long assistant/tool outputs.
  - Compare before/after while idle, typing, and streaming.

---

## Risk Notes

- Dirty-driven rendering can miss redraws if invalidation hooks are incomplete.
  - Mitigation: centralize invalidation paths and keep a low-frequency fallback tick initially.
- Event batching can starve input if unbounded.
  - Mitigation: fixed drain cap per loop iteration.
- Delta coalescing changes perceived streaming smoothness.
  - Mitigation: short flush interval with adaptive behavior.

---

## File/Code Reference Index

- Main loop/render cadence:
  - `src/cli/chat.rs:782`
  - `src/cli/chat.rs:790`
  - `src/cli/chat.rs:88`
- App line cache:
  - `src/cli/tui/app.rs:825`
- Message rendering clone path:
  - `src/cli/tui/ui.rs:423`
  - `src/cli/tui/ui.rs:442`
- Sidebar recompute path:
  - `src/cli/tui/ui.rs:315`
  - `src/cli/tui/ui.rs:333`
  - `src/cli/tui/ui.rs:383`
  - `src/cli/tui/ui.rs:1810`
  - `src/cli/tui/ui.rs:1846`
- Context estimate path:
  - `src/cli/tui/app.rs:855`
- Event handling path:
  - `src/cli/chat.rs:855`

---

## Benchmark Baseline Added

A Criterion benchmark target was added to establish a repeatable baseline for the hot paths most related to history-size lag:

- `benches/tui_baseline.rs`
  - `tui/build_message_lines`
  - `tui/context_usage`
  - `tui/get_lines_rebuild`
- `Cargo.toml`
  - adds `criterion` under `[dev-dependencies]`
  - adds `[[bench]]` target named `tui_baseline`

Run with:

```bash
cargo bench --bench tui_baseline
```

Note: this benchmark currently exercises compute-heavy rendering/model paths without terminal I/O, which is appropriate for stable regression tracking in CI.

---

## TL;DR

Primary issue is not one isolated hotspot; it is the combination of:
- unconditional frequent full redraw,
- per-frame cloning of large rendered message buffers,
- sidebar full-history rescans/parsing every frame,
- frequent full-cache rebuilds while streaming.

Addressing P0 items should materially improve responsiveness for long sessions, especially typing latency.
