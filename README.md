# Happy Harness

Version: 0.1.0

Happy Harness (hh) is a terminal-based agentic coding harness. It provides a robust, extensible framework for building AI-powered coding agents that operate through a rich terminal user interface (TUI).

## Features

- **Terminal-Based TUI**: Rich, interactive terminal interface built with ratatui with syntax highlighting
- **Agent Runtime**: Core loop orchestrating turns, tool calls, approvals, and termination
- **Provider-Agnostic Architecture**: Clean separation between LLM concepts and provider implementations
- **Extensible Tools**: 9 integrated tools including file operations, bash, web access, and todo management
- **Permission System**: Fine-grained per-tool permission control (allow/ask/deny)
- **Session Persistence**: Full session history with support for multiple sessions and resume
- **Configuration**: TOML-based project and global configuration
- **Debug Mode**: Frame-by-frame TUI debugging for development and troubleshooting

## Quick Start

```bash
# Build the project
cargo build --release

# Initialize project configuration
hh config init

# List available tools
hh tools

# Run a single prompt
hh run "list files in current directory"

# Start interactive chat
hh chat

# Debug a prompt (captures screen frames)
hh run "your prompt" --debug ./debug

# Replay debug frames
hh replay ./debug

# Show current configuration
hh config show
```

## Available Tools

- `read` - Read file contents
- `write` - Write UTF-8 text to file
- `edit` - Edit a file by replacing an exact string
- `list` - List directory entries
- `glob` - Glob files matching a pattern
- `grep` - Search regex in files recursively
- `bash` - Run shell commands
- `web_search` - Search the web
- `web_fetch` - Fetch content from URLs
- `todo_write` - Manage canonical todo list state

## Configuration

Happy Harness uses TOML configuration files to manage settings:

### Initialize Configuration

```bash
hh config init  # Creates .hh/config.toml in current directory
```

### Configuration Structure

- `provider` - LLM provider settings (base_url, model, api_key_env)
- `agent` - Agent behavior (max_steps, token_budget, system_prompt)
- `tools` - Tool enablement flags (fs, bash, web)
- `permission` - Per-tool permission policy (allow/ask/deny)
- `session` - Session storage root directory

### Environment Variables

The default configuration uses `OPENAI_API_KEY` environment variable. Set it before running:

```bash
export OPENAI_API_KEY="your-key-here"
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
  - `src/core/` - Core runtime, types, traits, and agent loop
  - `src/config/` - Configuration loading and settings
  - `src/provider/` - LLM provider adapters (OpenAI-compatible)
  - `src/tool/` - Tool implementations and registry
  - `src/cli/` - Command-line interface and TUI
  - `src/permission/` - Permission policy system
  - `src/session/` - Session persistence and management
  - `src/safety/` - Safety and validation utilities
- `tests/` - Integration and unit tests
- `AGENTS.md` - Detailed architecture and debugging guide
- `Cargo.toml` - Project dependencies and metadata

## License

[Add your license here]
