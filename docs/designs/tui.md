# TUI Architecture Design

## Overview
The terminal user interface (TUI) of `hh` is built using the `ratatui` library. To manage the inherent complexity of terminal interfaces-such as concurrent background LLM tasks, keystroke parsing, complex layouts, and dynamic state-the UI layer is built on the **Elm Architecture** (Model-View-Update).

This design enforces strict separation of concerns, eliminating "God Object" anti-patterns and avoiding awkward interior mutability (`RefCell`) in rendering paths.

## Core Abstractions

### 1. The `Component` Trait
Every distinct visual region in the application (input box, sidebar, message list) is an isolated component.

```rust
pub trait Component {
    /// React to an application-wide action.
    /// Optionally emit a follow-up action for cross-component behavior.
    fn update(&mut self, action: &AppAction) -> Option<AppAction> { None }

    /// Handle normalized terminal input.
    /// Return an AppAction when global coordination is needed.
    fn handle_event(&mut self, event: &InputEvent) -> Option<AppAction> { None }

    /// Draw the component state from a read-only render snapshot.
    fn render(
        &self,
        f: &mut ratatui::Frame<'_>,
        area: ratatui::layout::Rect,
        state: &SessionContext,
    );
}
```

**Guideline**:
- Local-only effects stay local (`update`/`handle_event` mutates component state and returns `None`).
- Return `Some(AppAction)` only when other components, handlers, or root state must react.

### 2. Message Passing: `AppAction`
Components never mutate global state or other components directly. Cross-component communication and orchestration are done via `AppAction`.

```rust
pub enum AppAction {
    // UI lifecycle
    Quit,
    Redraw,

    // User intent
    SubmitInput(String, Vec<MessageAttachment>),
    RunSlashCommand(SlashCommand, String),
    CancelExecution,

    // Agent callbacks (boundary event wrapped for dispatch)
    AgentEvent(TuiEvent),

    // Navigation + layout
    ScrollMessages(i32),
    ScrollSidebar(i32),
    SelectSession(String),

    // Runtime/session coordination
    QueueUserMessage { .. },
    OpenSubagentSession { .. },
    ResumeSessionLoaded { .. },
    SetSessionIdentity { .. },
}
```

### 3. Normalizing Input: `InputEvent`
Raw `crossterm` input includes noise (for example key release events). The main loop first normalizes raw input into `InputEvent` before routing it to components.

```rust
pub enum InputEvent {
    KeyPress(crossterm::event::KeyEvent),
    Paste(String),
    ScrollUp { x: u16, y: u16 },
    ScrollDown { x: u16, y: u16 },
    Click { x: u16, y: u16 },
    Resize,
}
```

### 4. Render Context vs Mutable App State
- `SessionContext`: read-only snapshot passed to components during `render`.
- `AppState` (owned by root `App`): mutable source of truth used by reducers/orchestrator and handlers.

Components can mutate only their own local state.

## Event Pipeline (End-to-End)

### A) Runner output -> UI
1. TUI-side adapter implements `RunnerOutputObserver`.
2. Observer callbacks convert runner outputs into boundary events (`TuiEvent`) and send them over channel.
3. Main loop receives `TuiEvent` and wraps it as `AppAction::AgentEvent(TuiEvent)`.
4. `app.dispatch(...)` processes the action (root reducer -> handlers -> component `update()`).

### B) User input -> UI / runner
1. Terminal event is normalized to `InputEvent`.
2. Focused component receives `handle_event(&InputEvent)`.
3. Component may emit semantic `AppAction` (for example `SubmitInput`, `ScrollMessages`).
4. `app.dispatch(...)` processes the action.
5. If runner interaction is needed, handler/orchestrator maps action to `RunnerInput` and sends it to runner.

**Important boundary rule**: components do not send `RunnerInput` directly.

## Dispatch Contract and Safety
- Dispatch execution is queue-based (`VecDeque<AppAction>`), not recursive.
- There are two entrypoints over one queue engine:
  - `dispatch(...)` for pure UI/reducer/component action flow.
  - `dispatch_with_runtime(...)` for actions that require runtime handler context (settings/cwd/event sender).
- For each dequeued action:
  1. Apply root reducer/orchestrator state changes.
  2. Run handlers (side effects, runtime, DB, subprocesses).
  3. Broadcast to component `update()`.
- Follow-up actions are appended to the queue.
- Enforce `MAX_ACTIONS_PER_TICK` as a convergence guard.
- On overflow: log an error, enqueue a visible user-facing error state/event, and drop remaining queued actions for that tick.

Implementation note: task-handle installation (`SetAgentTask`) is applied in queue plumbing before reducer matching because the action payload owns non-cloneable runtime handles.

## Application Structure

The codebase is split into three layers:

### Layer 1: Visual Identity (`src/theme/`)
Pure functions and constants defining look-and-feel, independent of app state.
- `colors.rs`: constants like `ACCENT`, `TEXT_MUTED`
- `markdown.rs`: markdown -> styled `ratatui::Line`

### Layer 2: UI Views (`src/app/components/`)
Stateful interface chunks implementing `Component`.
- `sidebar.rs`: fold state, scroll offsets, local caches
- `messages.rs`: text selection, viewport caching
- `input.rs`: cursor behavior, pending question overlays

### Layer 3: Handlers (`src/app/handlers/`)
Controllers for side effects and domain operations.
- `runner.rs`: runtime integration and channel plumbing
- `session.rs`: loading/saving/compacting session history

Handler boundaries:
- Handlers receive explicit inputs from `App`.
- Handlers do not mutate UI state directly and do not read component internals.
- Handlers emit outcomes as actions/events; `App::dispatch` maps outcomes to state/UI updates.

### Root Orchestrator (`src/app/state.rs` and `mod.rs`)
`App` binds components + handlers and owns queue dispatch.

Main loop shape:
1. `terminal.draw()`
2. `tokio::select!` on normalized user input vs boundary runner events
3. Route to components or wrap as actions
4. `app.dispatch(...)` or `app.dispatch_with_runtime(...)` depending on side-effect needs
