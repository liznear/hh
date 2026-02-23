# AGENTS

## Build
- `cargo check`
- `cargo build`

## Test
- `cargo test`

## Lint and format
- `cargo fmt --check`
- `cargo clippy -- -D warnings`

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
