# Happy Harness

Version: 0.1.0

Happy Harness (hh) is a terminal-based agentic coding harness. It provides a robust, extensible framework for building AI-powered coding agents that operate through a rich terminal user interface (TUI).

## Features

- **Terminal-Based TUI**: Rich, interactive terminal interface built with ratatui with syntax highlighting and markdown rendering
- **Agent Runtime**: Core loop orchestrating turns, tool calls, approvals, and termination with trait-based architecture
- **Provider-Agnostic Architecture**: Clean separation between LLM concepts and provider implementations (OpenAI-compatible API)
- **Extensible Tools**: 10 integrated tools including file operations, bash, web access, diff visualization, and todo management
- **Typed Tool Output**: Structured tool results with content-type metadata for intelligent rendering
- **Permission System**: Fine-grained per-tool permission control (allow/ask/deny) with capability-based policy
- **Session Persistence**: Full session history with workspace-based storage, compaction, and resume support
- **Configuration**: TOML-based project and global configuration
- **Image Support**: Paste images from clipboard or file paths for multimodal interactions
- **Debug Mode**: Frame-by-frame TUI debugging for development and troubleshooting
- **Sub-Agent Protocol**: Scaffolding for nested agent execution with depth limits

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

## TUI Slash Commands

When running `hh chat`, the following slash commands are available:

- `/new` - Start a new session
- `/model` - List models or switch to `/model <provider-id/model-id>`
- `/resume` - Resume a previous session from a list
- `/compact` - Summarize conversation history to save context
- `/quit` - Exit the application

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
- `todo_read` - Read canonical todo list state
- `todo_write` - Manage canonical todo list state
- `diff` - Compute and display unified diffs between text

## Configuration

Happy Harness uses JSON configuration files to manage settings:

### Initialize Configuration

```bash
hh config init  # Creates .hh/config.json in current directory
```

### Configuration Structure

- `models.default` - Global default model reference (`provider-id/model-id`)
- `providers` - Provider registry (per-provider display name, base_url, api_key_env, and `models` map keyed by HH model id)
- `agent` - Agent behavior (max_steps, sub_agent_max_depth, system_prompt)
- `tools` - Tool enablement flags (fs, bash, web)
- `permission` - Per-tool permission policy (allow/ask/deny)
- `session` - Session storage root directory

### Environment Variables

The default configuration uses `OPENAI_API_KEY` environment variable. Set it before running:

```bash
export OPENAI_API_KEY="your-key-here"
```

## Architecture

The project follows a layered architecture with provider-agnostic core types and trait-based boundaries:

### Core Design Principles

- **Provider-Agnostic Core**: Canonical data structures for LLM interactions (`Role`, `Message`, `ToolCall`)
- **Trait-Based Boundaries**: `Provider`, `ToolExecutor`, `ApprovalPolicy`, `SessionSink`, `SessionReader`
- **Event-Driven UI**: TUI and CLI render by observing `AgentEvents`, not by embedding runtime logic
- **Typed Tool Output**: `ToolResult` includes structured metadata (summary, content_type, payload) for intelligent rendering
- **Session Per Workspace**: Sessions are organized by workspace path for automatic project context

### Module Structure

- **`src/core/`** - Core runtime, types, traits, and agent loop
  - `AgentLoop` - Generic orchestration over trait bounds
  - Domain types (`Message`, `ToolCall`, `SubAgentCall`, etc.)
  - Core traits (`Provider`, `ToolExecutor`, `ApprovalPolicy`, `SessionSink`, `SessionReader`)
- **`src/config/`** - Configuration loading and settings
- **`src/provider/`** - LLM provider adapters (OpenAI-compatible)
- **`src/tool/`** - Tool implementations and registry
- **`src/cli/`** - Command-line interface and TUI
- **`src/permission/`** - Permission policy system with capability-based matching
- **`src/session/`** - Session persistence, compaction, and workspace-aware storage
- **`src/safety/`** - Safety and validation utilities

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

### Nix Development Environment

```bash
nix develop  # Enter development shell with Rust toolchain
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

### Running Specific Tests

```bash
cargo test agent_loop
cargo test tool
cargo test session
```

See `AGENTS.md` for detailed architecture documentation and debugging workflows.

## Session Management

Sessions are automatically organized by workspace path:

- Each workspace (directory) gets its own session collection
- Use `/new` to start a fresh session in the current workspace
- Use `/resume` to browse and resume previous sessions
- Use `/compact` to summarize conversation history and save tokens

Sessions persist across runs in `~/.local/state/hh/sessions/<workspace-path>/`.

## Image Support

The TUI supports image attachments for multimodal interactions:

- Paste images from clipboard (Ctrl+V or Cmd+V)
- Drag and drop image file paths into the input
- Supported formats: PNG, JPEG, GIF, WebP, BMP, TIFF, HEIC/HEIF, AVIF

Images are base64-encoded and sent to the provider if supported.

## License

[Add your license here]
