# AGENTS

## Build
- `cargo check`
- `cargo build`

## Test
- `cargo test`

## Lint and format
- `cargo fmt --check`
- `cargo clippy -- -D warnings`

## Architecture: LLM Domain Types

Use a provider-agnostic core module for LLM concepts and compose other modules from it.

The application core is the agent runtime: agent loop + domain types + traits.

- Agent loop orchestrates turns, tool calls, approvals, and termination.
- Domain types define canonical data structures used across the runtime.
- Traits define protocols for integration points (provider/model client, tools, events, persistence, approval policy).
- Adapters implement traits (for example, TUI implements event-handling traits).

1. Define domain-level types in a core module (for example: `Role`, `Message`, `ToolCall`).
   - These types represent the application's canonical LLM interaction model.
   - Keep this layer provider-agnostic and stable.
   - Keep provider-specific wire details out of core types.
2. Define core traits in the core runtime module.
   - Traits capture behavior contracts and side-effect boundaries.
   - Favor small, capability-oriented traits that are easy to test with fakes.
3. Make session persistence depend on core domain types.
   - Session events should compose core types rather than redefining parallel structures.
   - Add session-only metadata (`id`, timestamps, approval decisions, etc.) at the event layer.
4. Make provider implementations depend on core domain types.
   - Provider adapters should map provider-specific wire formats to/from the core types.
   - Avoid leaking provider-specific fields into the core module unless they are true cross-provider invariants.
5. Make UI layers depend on core traits.
   - TUI/CLI should implement core event/output traits rather than embedding runtime logic.
   - Keep UI state and rendering concerns out of the core agent loop.

Design intent: keep a single source of truth for LLM semantics, while letting persistence and provider adapters evolve independently.

## Debugging TUI

The TUI is only available in `chat` mode. The `run` command does not have a TUI.

Use `cargo run -- <command>` instead of `hh <command>` to run the latest built version.

### Using tmux for TUI Debugging

Capture TUI state using tmux:

```bash
# Start chat in a tmux session
tmux new-session -d -s hh-debug "cargo run -- chat"

# Capture current pane contents as text
tmux capture-pane -t hh-debug -p > debug-screen.txt

# Capture with ANSI colors preserved
tmux capture-pane -t hh-debug -p -e > debug-screen-ansi.txt

# Send keys to the session
tmux send-keys -t hh-debug "your input here" Enter

# Kill the session when done
tmux kill-session -t hh-debug
```

### Debugging Workflow for AI

1. Start chat in a tmux session:
   ```bash
   tmux new-session -d -s hh-debug "cargo run -- chat"
   ```

2. Interact with the session or send keys:
   ```bash
   tmux send-keys -t hh-debug "your test input" Enter
   ```

3. Capture the screen state:
   ```bash
   tmux capture-pane -t hh-debug -p > debug-output.txt
   ```

4. Read the captured output to understand what happened:
   ```bash
   cat debug-output.txt
   ```

5. Fix the issue based on what you observed.

6. Clean up when done:
   ```bash
   tmux kill-session -t hh-debug
   ```

## TUI UI Learnings

### Slash command autocomplete

- Anchor popup geometry to the input box geometry, not the parent chunk geometry.
  - Use the same left edge and width as the rendered input panel so borders line up.
- Keep autocomplete list styling flat (no extra border block) when matching the current TUI visual language.
- Avoid hardcoded spacing literals (for example, `"  "`) in row layout.
  - Define a padding variable (for example, `list_left_padding`) and derive both spacing and width calculations from it.
- When adding row padding, update description width math accordingly to prevent overflow/truncation regressions.
