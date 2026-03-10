# TUI Refactoring Plan

Status: Completed (all phases and verification items checked).

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
