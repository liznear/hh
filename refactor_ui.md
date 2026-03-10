# TUI Refactoring Plan

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
│   ├── mod.rs           # entrypoint: run_interactive_chat()
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
│   │   └── commands.rs  # Slash command definitions
│   │
│   └── handlers/        # CONTROLLERS (No UI logic, just State manipulation)
│       ├── mod.rs
│       ├── runner.rs    # (from chat/agent_run.rs) Spawns LLMs
│       ├── session.rs   # (from chat/session.rs) Loads DB histories
│       ├── subagent.rs  # (from chat/subagent.rs)
│       └── actions.rs   # (from chat/commands.rs) Handles UI intent triggers
```

## 3. Detailed Phases & Todo Items

### Phase 1: Establish the Global Themes (`src/theme/`)
*   **Goal**: Decouple pure visual styling from the application state.
*   **Description**: Move styling constants, markdown rendering, and tool output formatting to a top-level module so that they can be used independently of the interactive TUI (e.g. for standard CLI stdout).
*   **Principle**: Pure functions taking raw data and returning `ratatui::Line` or string widgets. No dependencies on `App` state.
*   **Todos**:
    - [ ] Create `src/theme/mod.rs`.
    - [ ] Move `src/cli/tui/ui/theme.rs` to `src/theme/colors.rs`.
    - [ ] Move `src/cli/tui/markdown.rs` to `src/theme/markdown.rs`.
    - [ ] Move `src/cli/tui/tool_presentation.rs` and `tool_render.rs` into `src/theme/`.
    - [ ] Update `lib.rs` and existing imports to point to `crate::theme::*`.

### Phase 2: Establish the App Framework (`src/app/`)
*   **Goal**: Create the foundation for the new UI orchestrator.
*   **Description**: Define the `Component` trait, the `AppAction` enum, and the parsed `InputEvent` abstractions to establish the boundaries of the Elm architecture.
*   **Principle**: Message passing over direct mutation. Components never directly mutate the root state; they emit `AppAction`s.
*   **Todos**:
    - [ ] Create `src/app/core.rs` containing `trait Component` and `enum AppAction`.
    - [ ] Create `src/app/events.rs`. Migrate `InputEvent` parsing from `chat/input.rs` and `TuiEvent` from `tui/event.rs`.
    - [ ] Create `src/app/terminal.rs` by migrating crossterm setup logic from `tui/terminal.rs`.
    - [ ] Create `src/app/state.rs` defining the `SessionContext` and the skeletal `App` struct.

### Phase 3: Implement the Components (`src/app/components/`)
*   **Goal**: Split the massive `ChatApp` struct into focused UI elements implementing the `Component` trait.
*   **Description**: Each component will own its local state, implement `render()`, translate keystrokes in `handle_event()`, and react to messages in `update()`.
*   **Principle**: Components should hold their own cache (eliminating `RefCell`) and only require `SessionContext` for read-only global state during rendering.
*   **Todos**:
    - [ ] Create `sidebar.rs`: Migrate the todo list, file tree, subagent state, and rendering logic.
    - [ ] Create `messages.rs`: Migrate the chat history vector, scroll state, text selection logic, and complex diff/markdown rendering.
    - [ ] Create `input.rs`: Migrate the user text string, cursor position, pending questions, slash commands, and keyboard input logic.
    - [ ] Create `popups.rs`: Migrate overlays like Command Palette, Session Picker, and Clipboard Notice.
    - [ ] Migrate helper modules like `viewport_cache.rs` and `commands.rs`.

### Phase 4: Implement the Handlers (`src/app/handlers/`)
*   **Goal**: Isolate execution and business logic from the UI folder.
*   **Description**: Move DB loads, subagent spawning, and LLM task generation into dedicated controllers.
*   **Principle**: Handlers do not touch UI components directly. They only emit `AppAction`s or receive them from the `App` orchestrator.
*   **Todos**:
    - [ ] Move `src/cli/chat/agent_run.rs` to `src/app/handlers/runner.rs`.
    - [ ] Move `src/cli/chat/session.rs` to `src/app/handlers/session.rs`.
    - [ ] Move `src/cli/chat/subagent.rs` to `src/app/handlers/subagent.rs`.
    - [ ] Move action handlers (`src/cli/chat/commands.rs`) to `src/app/handlers/actions.rs`.

### Phase 5: Wire the Orchestrator (`src/app/mod.rs` & `state.rs`)
*   **Goal**: Re-write the main application loop using message passing.
*   **Description**: Tie the UI components and handlers together inside `run_interactive_chat_loop`.
*   **Principle**: Centralized dispatch. All state changes that span multiple components are routed through `app.dispatch(action)`.
*   **Todos**:
    - [ ] Implement `App::dispatch(&mut self, action: AppAction)` to broadcast actions to components and trigger handlers.
    - [ ] Re-write `run_interactive_chat_loop()` to call `terminal.draw` and `tokio::select!` for `InputEvent` and `TuiEvent` channels.
    - [ ] Hook up component `handle_event` methods within the main loop to generate `AppAction`s.

### Phase 6: Cleanup
*   **Goal**: Remove legacy code and ensure typing/formatting.
*   **Description**: Complete the transition by severing ties to the old structure.
*   **Principle**: Leave no dead code behind.
*   **Todos**:
    - [ ] Update `src/cli/mod.rs` to point to `crate::app::run_interactive_chat` instead of `crate::cli::chat::run_chat`.
    - [ ] Delete `src/cli/tui/` and `src/cli/chat/` directories entirely.
    - [ ] Run `cargo check`, `cargo clippy`, and `cargo test` to ensure stability and correctness.