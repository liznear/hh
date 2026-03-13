# TUI Refactoring Plan

Status: Completed.
Last reviewed: 2026-03-13.

## Current Alignment Audit (vs `docs/designs/tui.md`)

### What is aligned
- Target module layout is mostly in place (`src/theme`, `src/app/components`, `src/app/handlers`).
- Legacy `src/cli/tui` and `src/cli/chat` directories are removed.
- `App::dispatch` is queue-based with an overflow guard (`MAX_ACTIONS_PER_TICK`).
- Normalized terminal-input abstraction exists in `src/app/events.rs`.
- Exactly one `InputEvent` enum remains under `src/app` (`src/app/events.rs`).
- `AppState` now holds canonical runtime state (`messages`, `agent_task`, `is_processing`, etc.).
- Input handlers (`src/app/input.rs`) operate on `AppState` instead of `ChatApp`.
- `AppAction::AgentEvent` is reduced through `AppState::handle_agent_event`.
- Session replay loading is now centralized in `src/app/handlers/session.rs` and reused by input/tick paths.
- Submit and subagent-open input flows now emit intent actions (`AppAction::SubmitInput`, `AppAction::OpenSubagentSession`) instead of invoking handlers directly in `input.rs`.
- Queued submit flow now emits `AppAction::QueueUserMessage` and is executed at app runtime boundary instead of directly in `input.rs`.
- Runtime-aware queue dispatch is now unified through a shared internal queue (`dispatch_internal`), preserving reducer -> handlers -> components ordering for intent actions.
- Subagent session-open now reduces through `AppAction::SubagentSessionLoaded` instead of mutating state directly in runtime handler path.
- Command palette rendering entrypoint is now owned by `PopupComponent` (root delegates to component method).
- Message list, sidebar, and input panel render entrypoints now delegate through component methods (`MessagesComponent`, `SidebarComponent`, `InputComponent`).
- Processing indicator rendering entrypoint now delegates through `InputComponent`.
- Subagent back-indicator rendering entrypoint now delegates through `InputComponent`.
- Session-picker state transition from `/resume` now reduces via `AppAction::ShowSessionPicker` instead of direct handler mutation.
- Slash-command input cleanup now emits `AppAction::RemoveMessageAt` and is reduced centrally.
- Session replay selection now emits `AppAction::ResumeSessionLoaded` and is reduced centrally instead of mutating `AppState` in session handler.
- Runner now emits `AppAction::SetSessionIdentity` for initial session assignment instead of mutating session id/title directly.
- Root rendering now invokes popup `Component::render` directly from root layout composition (no separate post-render component pass).
- Slash/session handler helpers now use read-only `&AppState` where mutation is no longer needed.
- Removed legacy `actions` out-param plumbing from submit/runner/session handlers; handler outputs now flow only via returned `Vec<AppAction>`.
- Runner task registration now emits `AppAction::SetAgentTask`; dispatch applies task install through the runtime action queue.
- Runner pre-run cancellation now emits `AppAction::CancelAgentTask` instead of direct handler mutation.
- Submit/session/runner chat handlers now operate as action producers (state writes are reduced centrally for these flows).
- Active subagent-session replay refresh now routes through runtime intent (`AppAction::RefreshActiveSubagentSession`) and session handler outcomes.
- Subagent session-open replay loading now routes through `handlers/session.rs` helper outcomes instead of inline runtime loading.
- Periodic tick state refresh (`on_periodic_tick`) is now reduced centrally through `AppAction::PeriodicTick`.

### Remaining intentional deviations
1. **Legacy runtime bridge removed**
   - `legacy_chat_app` is no longer present in `AppState`.
   - Reducers and runtime dispatch now update only canonical `AppState` fields.
   - Exit criteria for removing bridge references is met (`rg "legacy_chat_app" src/app` returns no matches).

2. **Handler boundary intentional deviation**
   - Dispatch still has context-dependent entrypoints (`dispatch` vs `dispatch_with_runtime`), even though they share one internal queue.
   - Task-lifecycle side effects (`CancelAgentTask`/`SetAgentTask`) are handled in runtime dispatch plumbing rather than the reducer match because `SetAgentTask` carries non-cloneable task handles.

3. **Render contract largely aligned**
   - Root render path composes component entrypoints for message/sidebar/input/popup rendering.
   - Shared layout helpers moved to `src/app/components/layout.rs`; root focuses on composition + high-level branching.

4. **Interior mutability moved out of component cache paths**
   - Component cache paths now use explicit mutable ownership.
   - Remaining `RefCell` usage is in non-component state (`AppState`) and is outside component render-cache ownership.

## 1. High-Level Goal
Transform the monolithic `ChatApp` into a decoupled, Elm-style Model-View-Update (MVU) architecture with strict boundaries between state, rendering, and business logic. This will eliminate "God Object" anti-patterns, remove the need for `RefCell` caching workarounds in rendering, and ensure the UI layer is scalable, testable, and maintainable.

## 2. High-Level Folder Structure
```text
src/
├── cli/                 # STRICTLY CLI BOOTSTRAPPING
│   ├── mod.rs           # clap args parsing
│   ├── render.rs        # stdout streaming (non-interactive)
│   └── agent_init.rs    # Initializing settings from disk
│
├── theme/               # GLOBAL VISUAL IDENTITY
│   ├── mod.rs           # Exports colors and math
│   ├── colors.rs        # (from tui/ui/theme.rs)
│   ├── markdown.rs      # (from tui/markdown.rs)
│   ├── tool_presentation.rs # (from tui/tool_presentation.rs)
│   └── tool_render.rs   # (from tui/tool_render.rs)
│
├── app/                 # THE INTERACTIVE APPLICATION
│   ├── mod.rs           # entrypoint: run_interactive_chat() (owns loop lifecycle)
│   ├── core.rs          # Component trait, AppAction enum
│   ├── state.rs         # The App orchestrator & SessionContext
│   ├── events.rs        # (from tui/event.rs) Agent -> UI channels & InputEvent
│   ├── terminal.rs      # (from tui/terminal.rs) Crossterm setup/teardown
│   │
│   ├── components/      # UI VIEWS (Implement Component Trait)
│   │   ├── mod.rs
│   │   ├── input.rs     # User input box & key handling
│   │   ├── messages.rs  # Message list
│   │   ├── sidebar.rs   # File tree, subagents, todos
│   │   ├── popups.rs    # Command palettes, session pickers
│   │   ├── viewport_cache.rs # Message scrolling helper
│   │   └── commands.rs  # Slash command parsing/metadata (UI-facing only)
│   │
│   └── handlers/        # CONTROLLERS (No UI logic, just State manipulation)
│       ├── mod.rs
│       ├── runner.rs    # (from chat/agent_run.rs) Spawns LLMs
│       ├── session.rs   # (from chat/session.rs) Loads DB histories
│       ├── subagent.rs  # (from chat/subagent.rs)
│       └── actions.rs   # (from chat/commands.rs) Handles UI intent triggers
```

## 3. Architecture Contracts (Must Hold During Migration)

### 3.1 `SessionContext` vs `AppState`
- `SessionContext` is a **read-only render snapshot** shared with components during `render()`.
- `SessionContext` contains cross-component data needed for drawing (for example: active session id, model label, execution status, feature flags, readonly derived summaries).
- `AppState` (owned by the root `App`) is the **mutable source of truth** for orchestration and handler interaction.
- Components can mutate only their own local state; they cannot directly mutate `AppState` or other components.

### 3.2 Why `Component::update` May Return `AppAction`
`update()` may emit a follow-up action when a component must react to an app-wide event with a new semantic intent.

Examples:
- `InputComponent` receives `AgentEvent(TuiEvent::AssistantDone)` and updates local input focus state.
- `SidebarComponent` receives `SelectSession` and emits `RequestSessionMetadataRefresh`.
- `PopupComponent` receives `SubmitInput` and emits `ClosePopup`.

Rule: return `None` unless a cross-component effect is required.

### 3.3 Dispatch and Recursion Safety
- `App::dispatch` processes actions through an explicit FIFO queue (`VecDeque<AppAction>`), not recursive calls.
- Each dequeued action is applied to:
  1) root reducer / orchestrator,
  2) handlers,
  3) components via `update()`.
- Any emitted follow-up actions are appended to the queue.
- Add a per-tick action budget guard (for example `MAX_ACTIONS_PER_TICK`). If exceeded: log an error, enqueue a visible user-facing error state/event, and drop remaining queued actions for that tick.
- Components and handlers must not emit the exact same action they just consumed unless they also change payload/state to guarantee convergence.

### 3.4 Handler Boundary Contract
Handlers are pure controllers around side effects and domain operations.

- Handlers receive explicit inputs from `App` (command + required state snapshot or ids).
- Handlers do not read component internals and do not mutate UI state directly.
- Handlers return domain outcomes as `AppAction` (or send `TuiEvent` which is converted to `AppAction` at the boundary).
- Side effects (DB, subprocesses, agent runtime, subagents) stay inside handlers.
- Mapping from handler outcome -> UI state changes always happens in `App::dispatch`.

### 3.5 Cache Ownership and Invalidation
- All render/perf caches are component-local.
- No shared mutable cache objects across components.
- Invalidation is action-driven and component-defined.
- Each component documents:
  - cache key inputs,
  - invalidating actions,
  - fallback behavior when cache is stale/missing.

## 4. Detailed Phases & Todo Items

### Phase 1: Establish the Global Themes (`src/theme/`)
*   **Goal**: Decouple pure visual styling from the application state.
*   **Description**: Move styling constants, markdown rendering, and tool output formatting to a top-level module so that they can be used independently of the interactive TUI (e.g. for standard CLI stdout).
*   **Principle**: Pure functions taking raw data and returning `ratatui::Line` or string widgets. No dependencies on `App` state.
*   **Todos**:
    - [x] Create `src/theme/mod.rs`.
    - [x] Move `src/cli/tui/ui/theme.rs` to `src/theme/colors.rs`.
    - [x] Move `src/cli/tui/markdown.rs` to `src/theme/markdown.rs`.
    - [x] Move `src/cli/tui/tool_presentation.rs` and `tool_render.rs` into `src/theme/`.
    - [x] Update `lib.rs` and existing imports to point to `crate::theme::*`.
    - [x] Verification: run `cargo check`.

### Phase 2: Establish the App Framework (`src/app/`)
*   **Goal**: Create the foundation for the new UI orchestrator.
*   **Description**: Define the `Component` trait, the `AppAction` enum, and the parsed `InputEvent` abstractions to establish the boundaries of the Elm architecture.
*   **Principle**: Message passing over direct mutation. Components never directly mutate the root state; they emit `AppAction`s.
*   **Todos**:
    - [x] Create `src/app/core.rs` containing `trait Component` and `enum AppAction`.
    - [x] Create `src/app/events.rs`. Migrate `InputEvent` parsing from `chat/input.rs` and `TuiEvent` from `tui/event.rs`.
    - [x] Create `src/app/terminal.rs` by migrating crossterm setup logic from `tui/terminal.rs`.
    - [x] Create `src/app/state.rs` defining the `SessionContext` and the skeletal `App` struct.
    - [x] Verification: run `cargo check` and focused tests for input normalization.

### Phase 3: Implement the Components (`src/app/components/`)
*   **Goal**: Split the massive `ChatApp` struct into focused UI elements implementing the `Component` trait.
*   **Description**: Each component will own its local state, implement `render()`, translate keystrokes in `handle_event()`, and react to messages in `update()`.
*   **Principle**: Components should hold their own cache (eliminating `RefCell`) and only require `SessionContext` for read-only global state during rendering.
*   **Todos**:
    - [x] Create `sidebar.rs`: Migrate the todo list, file tree, subagent state, and rendering logic.
    - [x] Create `messages.rs`: Migrate the chat history vector, scroll state, text selection logic, and complex diff/markdown rendering.
    - [x] Create `input.rs`: Migrate the user text string, cursor position, pending questions, slash commands, and keyboard input logic.
    - [x] Create `popups.rs`: Migrate overlays like Command Palette, Session Picker, and Clipboard Notice.
    - [x] Migrate `viewport_cache.rs` as a component utility (no side effects).
    - [x] Move slash-command parsing/metadata to `components/commands.rs`; move command execution side effects to `handlers/actions.rs`.
    - [x] Verification: run `cargo check` and targeted component update/render tests.

### Phase 4: Implement the Handlers (`src/app/handlers/`)
*   **Goal**: Isolate execution and business logic from the UI folder.
*   **Description**: Move DB loads, subagent spawning, and LLM task generation into dedicated controllers.
*   **Principle**: Handlers do not touch UI components directly. They receive explicit inputs from `App` and emit outcomes as `AppAction`s (or boundary events converted to actions).
*   **Todos**:
    - [x] Move `src/cli/chat/agent_run.rs` to `src/app/handlers/runner.rs`.
    - [x] Move `src/cli/chat/session.rs` to `src/app/handlers/session.rs`.
    - [x] Move `src/cli/chat/subagent.rs` to `src/app/handlers/subagent.rs`.
    - [x] Move action handlers (`src/cli/chat/commands.rs`) to `src/app/handlers/actions.rs`.
    - [x] Verification: run `cargo check` and focused handler tests (runner/session/subagent paths).

### Phase 5: Wire the Orchestrator (`src/app/mod.rs` & `state.rs`)
*   **Goal**: Re-write the main application loop using message passing.
*   **Description**: Tie components and handlers together inside internal `run_interactive_chat_loop`, invoked by public `run_interactive_chat`.
*   **Principle**: Centralized dispatch. All state changes that span multiple components are routed through `app.dispatch(action)`.
*   **Todos**:
    - [x] Implement queue-based `App::dispatch` (`VecDeque<AppAction>`) to process reducer -> handlers -> components without recursion.
    - [x] Re-write `run_interactive_chat_loop()` to call `terminal.draw` and `tokio::select!` for `InputEvent` and `TuiEvent` channels.
    - [x] Hook up component `handle_event` methods within the main loop to generate `AppAction`s.
    - [x] Verification: run `cargo check` and dispatch integration tests (including overflow guard behavior).

### Phase 6: Migration Safety Bridge
*   **Goal**: Make rollout reversible and low-risk while new and old paths coexist briefly.
*   **Description**: Add temporary compatibility seams so behavior can be validated before deleting legacy modules.
*   **Principle**: Prefer additive migration, then deletion.
*   **Todos**:
    - [x] Add temporary re-exports/shims so existing call sites keep compiling while modules move.
    - [x] Introduce a narrow interactive entrypoint bridge (`run_chat` -> `app::run_interactive_chat`) and migrate call sites incrementally.
    - [x] Keep old and new paths behind a short-lived feature/config toggle only if needed for fallback.
    - [x] Remove shims immediately after all call sites are switched.

### Phase 7: Cleanup
*   **Goal**: Remove legacy code and ensure typing/formatting.
*   **Description**: Complete the transition by severing ties to the old structure.
*   **Principle**: Leave no dead code behind.
*   **Todos**:
    - [x] Update `src/cli/mod.rs` to point to `crate::app::run_interactive_chat` instead of `crate::cli::chat::run_chat`.
    - [x] Delete `src/cli/tui/` and `src/cli/chat/` directories entirely.
    - [x] Run `cargo check`, `cargo clippy`, and `cargo test` to ensure stability and correctness.

### Phase 8: Remove `legacy_chat_app` as Runtime Source of Truth
*   **Goal**: Make `AppState` the only mutable orchestration state.
*   **Description**: Migrate remaining `ChatApp`-owned state and behavior into `AppState` + component-local state, then delete the bridge field.
*   **Principle**: No parallel state containers for the same UI/runtime concepts.
*   **Todos**:
    - [x] Inventory all `legacy_chat_app` reads/writes in `src/app/mod.rs` and `src/app/state.rs`.
    - [x] Move session/run epoch, message transcript, processing flags, and selection state into canonical `AppState` fields.
    - [x] Replace `legacy_chat_app.handle_event(...)` reducer path with explicit `AppAction` reducers.
    - [x] Complete `AppAction` reducers implementation (`AgentEvent` logic on `AppState`).
    - [x] Update rendering to use `AppState` fields instead of `legacy_chat_app`.
    - [x] Remove `legacy_chat_app` from `AppState` once all call sites are migrated.
    - [x] Exit criteria: `rg "legacy_chat_app" src/app` returns no production call sites (allow tests only if explicitly justified).
    - [x] Verification: run `cargo check` and targeted reducer tests.

### Phase 9: Unify Input Normalization Pipeline
*   **Goal**: Use one `InputEvent` model and one terminal normalization path.
*   **Description**: Consolidate overlapping input modules so the loop consumes only `app::events::InputEvent`.
*   **Principle**: Single boundary type for terminal input.
*   **Todos**:
    - [x] Declare `app::events::InputEvent` as canonical and migrate `AppAction::Input` to use it.
    - [x] Remove duplicate enum definitions from `src/app/input.rs` after migration.
    - [x] Route `run_interactive_chat_loop` to the canonical `read_input_batch` path.
    - [x] Migrate key/paste/mouse handlers to consume canonical events without legacy adapters.
    - [x] Delete obsolete input translation utilities.
    - [x] Exit criteria: exactly one `enum InputEvent` remains under `src/app`.
    - [x] Verification: run `cargo test` focused on input normalization and key handling.

### Phase 10: Enforce Handler and Dispatch Boundaries
*   **Goal**: Ensure side effects are orchestrated through `App::dispatch` contracts.
*   **Description**: Remove direct runtime/command side effects from input plumbing and route intent through `AppAction` -> handlers -> outcome actions.
*   **Principle**: Components/input translators emit intent; handlers perform side effects.
*   **Todos**:
    - [x] Define missing intent actions for submit, scroll, session ops, and cancellation (`SubmitInput`, `QueueUserMessage`, `OpenSubagentSession`, `ShowSessionPicker`, `ResumeSessionLoaded`, `RemoveMessageAt`, `SetSessionIdentity`, `CancelExecution`, `CancelAgentTask`, `SetAgentTask`, `ScrollMessages`, `ScrollSidebar`).
    - [x] Remove direct side-effect calls from `src/app/mod.rs` input loop (`handle_key_event`, `apply_paste`, `handle_area_scroll`, `handle_mouse_click`, `handle_mouse_drag`, `handle_mouse_release`) by routing through dispatch + handlers.
    - [x] Move remaining direct side-effect calls out of `src/app/input.rs` into handler entrypoints (session replay loading moved to `handlers/session.rs`; submit/queued-submit/subagent-open handling moved behind intent actions).
    - [x] Ensure handler outputs are represented as `AppAction`/`TuiEvent` and reduced centrally for submit/session/runner chat flows (subagent session-open completion via `AppAction::SubagentSessionLoaded`; session replay via `AppAction::ResumeSessionLoaded`; `/resume` picker transition via `AppAction::ShowSessionPicker`; slash-input cleanup via `AppAction::RemoveMessageAt`; runner initial session assignment via `AppAction::SetSessionIdentity`; runner task lifecycle via `AppAction::CancelAgentTask`/`AppAction::SetAgentTask`; legacy out-param action plumbing removed).
    - [x] Add regression tests for dispatch ordering (reducer -> handlers -> components).
    - [x] Exit criteria: input plumbing emits intent only; no direct runner/session mutation outside reducers/handlers.
    - [x] Verification: run `cargo check` and dispatch integration tests.

### Phase 11: Align Rendering with Component + `SessionContext` Contract
*   **Goal**: Make rendering component-owned and snapshot-driven.
*   **Description**: Narrow render inputs to read-only `SessionContext` and reduce monolithic `render.rs` responsibilities.
*   **Principle**: Components render from local state + read-only context; root orchestrates composition only.
*   **Todos**:
    - [x] Change `Component::render` to take `&SessionContext` (or equivalent render snapshot), not full `&AppState`.
    - [x] Move message/sidebar/input/popup rendering ownership behind component `render` implementations (Command palette/message/sidebar/input panel/processing indicator/subagent-back entrypoints delegate through component methods; sidebar clipping/composition helper lives in `SidebarComponent`; shared layout-rect computation plus root/main/subagent chunk builders live in `components/layout.rs`; input hit-testing/scroll paths use component layout/sidebar helpers directly; popup overlay render is invoked via `Component::render`).
    - [x] Keep layout composition at root, but remove direct `ChatApp`-centric rendering dependencies.
    - [x] Remove legacy `crate::app::render::render_app(...)` path after component rendering owns layout/content.
    - [x] Add tests for component render behavior where practical (`app::components::layout::tests` cover layout-rect render composition behavior).
    - [x] Exit criteria: root render path composes components only; no direct `ChatApp` rendering dependency.
    - [x] Verification: run `cargo check` and targeted render tests.

### Phase 12: Remove Interior-Mutability Cache Workarounds
*   **Goal**: Eliminate `RefCell`-based render caches where feasible, or explicitly justify retained uses.
*   **Description**: Replace interior mutability with explicit mutable cache ownership in component state/update paths.
*   **Principle**: Prefer explicit mutability and deterministic invalidation over hidden mutable state.
*   **Todos**:
    - [x] Refactor sidebar cache (`sidebar.rs`) to avoid `RefCell`/`Cell` for routine render updates.
    - [x] Refactor message viewport cache (`viewport_cache.rs`) to explicit mutable ownership via component update/render flow.
    - [x] Remove stale cache fields from `chat_state.rs` after ownership migration.
    - [x] Document cache keys + invalidation triggers per component in code comments/docstrings.
    - [x] Exit criteria: no `RefCell|Cell` in component cache paths unless accompanied by explicit in-code justification.
    - [x] Verification: run `cargo check`, `cargo clippy -- -D warnings`, and `cargo test`.

### Phase 13: Final Architecture Conformance Pass
*   **Goal**: Reconcile implementation with `docs/designs/tui.md` and close this plan.
*   **Description**: Perform a final audit and mark completion only when all contracts are satisfied.
*   **Principle**: Documentation status must match verified code reality.
*   **Todos**:
    - [x] Re-audit all architecture contracts in section 3 against code.
    - [x] Update `docs/designs/tui.md` and this plan for any intentional deviations.
    - [x] Run full verification suite: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `cargo check`.
    - [x] Mark status as completed only after passing checks and audit.
