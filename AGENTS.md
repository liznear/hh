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

The `--debug` and `replay` flows are no longer available.

Use tmux-based capture with `cargo run` instead.

### Single-Prompt Capture Mode

Run a single prompt in a dedicated tmux session and capture pane output:

```bash
# Start clean session
tmux kill-session -t hh-capture || true
tmux new-session -d -s hh-capture
tmux set-option -t hh-capture remain-on-exit on

# Execute prompt through local binary
tmux send-keys -t hh-capture 'cargo run -- run "what is 2+2?"' C-m

# Wait for completion, then capture output
sleep 5
tmux capture-pane -p -t hh-capture -S -300 > ./artifacts/tmux-run.txt
```

Use `cargo run -- ...` so tests and captures reflect the local workspace build, not a `hh` binary from `$PATH`.

### Interactive Capture Mode

Capture an interactive chat session with tmux:

```bash
tmux kill-session -t hh-chat || true
tmux new-session -d -s hh-chat
tmux set-option -t hh-chat remain-on-exit on
tmux send-keys -t hh-chat 'cargo run -- chat' C-m

# Attach to interact manually
tmux attach -t hh-chat

# After interaction, from another shell:
tmux capture-pane -p -t hh-chat -S -500 > ./artifacts/tmux-chat.txt
```

Store captures under a stable path (for example, `docs/artifacts/...`) when they are used as parity baselines.

### Debugging Workflow for AI

1. Run the problematic prompt via tmux and `cargo run`:
   ```bash
   tmux kill-session -t hh-case || true
   tmux new-session -d -s hh-case
   tmux set-option -t hh-case remain-on-exit on
   tmux send-keys -t hh-case 'cargo run -- run "your problematic prompt"' C-m
   ```

2. Capture pane output and inspect it:
   ```bash
   tmux capture-pane -p -t hh-case -S -400 > ./artifacts/hh-case.txt
   cat ./artifacts/hh-case.txt
   ```

3. Fix the issue based on what you observed in the captured terminal output.

## TUI UI Learnings

### Slash command autocomplete

- Anchor popup geometry to the input box geometry, not the parent chunk geometry.
  - Use the same left edge and width as the rendered input panel so borders line up.
- Keep autocomplete list styling flat (no extra border block) when matching the current TUI visual language.
- Avoid hardcoded spacing literals (for example, `"  "`) in row layout.
  - Define a padding variable (for example, `list_left_padding`) and derive both spacing and width calculations from it.
- When adding row padding, update description width math accordingly to prevent overflow/truncation regressions.
