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

The TUI can be debugged in both interactive and single-prompt modes by dumping frames to a directory.

### Single-Prompt Debug Mode

Run a single prompt and capture screen dumps:

```bash
# Basic usage
hh run "list files in current directory" --debug ./debug

# With custom output directory
hh run "what is 2+2?" --debug ./my-debug
```

This creates numbered screen dump files (`screen-000.txt`, `screen-001.txt`, etc.) in the output directory.

### Interactive Debug Mode

Dump frames while running the interactive TUI:

```bash
hh chat --debug ./debug-session
```

Frames are written continuously while you interact with the TUI.

### Replay Debug Frames

View captured frames:

```bash
# Basic replay (100ms delay between frames)
hh replay ./my-debug

# Faster replay
hh replay ./my-debug --delay 50

# Loop continuously
hh replay ./my-debug --loop
```

When running in a terminal:
- Press `q` to quit
- Press `p` to pause/resume

### Debugging Workflow for AI

1. Run the problematic prompt in single-prompt debug mode:
   ```bash
   hh run "your problematic prompt" --debug ./debug
   ```

2. Read the screen dumps to understand what happened:
   ```bash
   cat ./debug/screen-000.txt
   cat ./debug/screen-final.txt
   ```

3. Or replay all frames to see the animation:
   ```bash
   hh replay ./debug --delay 200
   ```

4. Fix the issue based on what you observed in the screen dumps.
