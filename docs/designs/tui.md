# TUI Architecture Design

## Overview
The terminal user interface (TUI) of `hh` is built using the `ratatui` library. To manage the inherent complexity of terminal interfaces—such as concurrent background LLM tasks, keystroke parsing, complex layouts, and dynamic state—the UI layer is built on the **Elm Architecture** (or Model-View-Update pattern).

This design ensures strict separation of concerns, eliminating "God Object" anti-patterns and the need for awkward interior mutability (`RefCell`) when rendering cached views.

## Core Abstractions

### 1. The `Component` Trait
Every distinct visual region in the application (Input Box, Sidebar, Message List) is an isolated component.

```rust
pub trait Component {
    /// React to an application-wide message. 
    /// Optionally emit a new Action to be processed by the root App.
    fn update(&mut self, action: &AppAction) -> Option<AppAction> { None }
    
    /// Handle a normalized terminal event (key press, mouse click).
    /// Translate it into a semantic AppAction if it requires global context,
    /// or handle it internally (e.g., updating a cursor position).
    fn handle_event(&mut self, event: &InputEvent) -> Option<AppAction> { None }
    
    /// Draw the component's state to the terminal screen.
    fn render(&self, f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect);
}
```

### 2. Message Passing: `AppAction`
To satisfy Rust's borrow checker and enforce decoupling, components **never mutate global state or other components directly**. Instead, cross-component communication happens via the `AppAction` enum.

```rust
pub enum AppAction {
    // UI Lifecycle
    Quit,
    Redraw,
    
    // User Intent
    SubmitInput(String, Vec<MessageAttachment>),
    RunSlashCommand(SlashCommand, String),
    CancelExecution,
    
    // Agent Callbacks
    AgentEvent(TuiEvent),
    
    // Navigation
    ScrollMessages(i32),
    SelectSession(String),
}
```
*Flow*: The Input component detects an "Enter" key press -> Returns `AppAction::SubmitInput` -> The root `App` dispatches this to the `ExecutionManager` -> The `ExecutionManager` spawns a background task.

### 3. Normalizing Events: `InputEvent`
Raw terminal events from `crossterm` contain a lot of noise (such as key releases) and require deep pattern matching (e.g. for mouse actions). 

Before events ever reach a `Component`, the main application loop parses them into an `InputEvent`. This application-specific abstraction makes handling user input cleaner and less repetitive across components.

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

## Application Structure

The codebase is split into three primary layers according to MVU:

### Layer 1: The Visual Identity (`src/theme/`)
Pure functions and constants defining the look and feel. Independent of state.
* `colors.rs`: Constants like `ACCENT`, `TEXT_MUTED`.
* `markdown.rs`: Translating raw markdown strings into stylized `ratatui::Line` objects using `syntect`.

### Layer 2: The UI Views (`src/app/components/`)
Stateful chunks of the interface that implement `Component`.
* `sidebar.rs`: Holds its own fold state, scroll offsets, and cached layout lines.
* `messages.rs`: Manages text selection and viewport caching.
* `input.rs`: Handles cursor calculations and the pending question prompt overlay.

### Layer 3: The Controllers (`src/app/handlers/`)
Background logic that manipulates application-level data.
* `runner.rs`: Connects the `core::agent` to the UI via `mpsc` channels.
* `session.rs`: Handles loading, saving, and compacting DB histories.

### The Root Orchestrator (`src/app/state.rs` & `mod.rs`)
The `App` struct is the source of truth that binds the components together. It holds global read-only variables inside `SessionContext` (which is passed down to components during `render`), and it implements the `dispatch` function which broadcasts `AppAction`s to all components. 

The main loop simply ticks:
1. `terminal.draw()`
2. `tokio::select!` on User Inputs vs. Background Agent Events.
3. Pass inputs to focused components.
4. Route resulting actions via `app.dispatch()`.
