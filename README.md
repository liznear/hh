# Happy Harness

Happy Harness (hh) is a terminal-based agentic coding harness. It provides a robust, extensible framework for building AI-powered coding agents that operate through a rich terminal user interface (TUI).

## Features

- **Terminal-Based TUI**: Rich, interactive terminal interface built with ratatui
- **Agent Runtime**: Core loop orchestrating turns, tool calls, approvals, and termination
- **Provider-Agnostic Architecture**: Clean separation between LLM concepts and provider implementations
- **Extensible Tools**: Composable tool system for file operations, web access, and more
- **Session Persistence**: Full session history and state management
- **Debug Mode**: Frame-by-frame TUI debugging for development and troubleshooting

## Quick Start

```bash
# Build the project
cargo build --release

# Run a single prompt
hh run "list files in current directory"

# Start interactive chat
hh chat

# Debug a prompt (captures screen frames)
hh run "your prompt" --debug ./debug

# Replay debug frames
hh replay ./debug
```

## Architecture

The project follows a layered architecture with provider-agnostic core types:

- **Core Domain Types**: Canonical data structures for LLM interactions (`Role`, `Message`, `ToolCall`)
- **Agent Loop**: Orchestrates conversation flow, tool execution, and termination
- **Core Traits**: Protocols for integration points (provider/model client, tools, events, persistence, approval policy)
- **Adapters**: Implement traits for specific providers (OpenAI, Anthropic, etc.) and interfaces (TUI, CLI)

Key design principles:
- Single source of truth for LLM semantics
- Provider-specific details isolated in adapters
- UI state separated from core runtime logic
- Small, testable traits with clear contracts

## Development

### Build and Test

```bash
cargo check
cargo build
cargo test
```

### Code Quality

```bash
cargo fmt --check
cargo clippy -- -D warnings
```

### Debugging the TUI

The TUI supports debug mode for development:

```bash
# Capture frames from a single prompt
hh run "test prompt" --debug ./debug

# Capture frames from interactive session
hh chat --debug ./debug-session

# Replay captured frames
hh replay ./debug --delay 100
```

See `AGENTS.md` for detailed architecture documentation and debugging workflows.

## Project Structure

- `src/` - Core Rust source code
- `tests/` - Integration and unit tests
- `AGENTS.md` - Detailed architecture and debugging guide
- `Cargo.toml` - Project dependencies and metadata

## License

[Add your license here]
