# Happy Harness

Version: 0.1.3

Happy Harness (`hh`) is a terminal-based coding agent runtime with a TUI, provider-agnostic core domain types, and a configurable tool/permission system.

## Current Status

`hh` is usable for day-to-day coding workflows and is still actively evolving.

Implemented today:
- Core agent loop, tool execution, approvals, and termination
- Interactive chat (`hh chat`) and one-shot mode (`hh run`)
- Config loading with layered overrides
- Workspace-scoped session persistence and history compaction
- OpenAI-compatible provider adapter
- Built-in primary and subagent profiles (`build`, `plan`, `explorer`, `general`)
- Subagent orchestration via the `task` protocol when task context is available

## Features

- **Terminal TUI**: Interactive chat experience built with `ratatui`.
- **Provider-Agnostic Core**: Canonical LLM domain types and runtime traits shared across providers.
- **Tool Runtime**: File, shell, web, skill, question, and todo tools with schema-driven invocation.
- **Permission Policy**: Per-capability allow/ask/deny policy with override support.
- **Session Persistence**: Workspace-aware session storage and resume/compact workflows.
- **Agent Profiles**: Built-in agents plus Markdown-defined custom agents.
- **Image Input**: Clipboard and file-path image attachments in chat input.

## Quick Start

```bash
# Build
cargo build --release

# Initialize project config
hh config init

# Show resolved config
hh config show

# List available tools and agents
hh tools
hh agents

# One-shot prompt
hh run "list files in current directory"

# Interactive chat
hh chat
```

## CLI Commands

- `hh chat [--max-turns <n>] [--agent <name>]`
- `hh run <prompt> [--max-turns <n>] [--agent <name>]`
- `hh tools`
- `hh agents`
- `hh config init`
- `hh config show`

## Slash Commands (Chat)

- `/new` - start a new session
- `/model` - list or switch models (`/model <provider-id/model-id>`)
- `/resume` - resume a prior session
- `/compact` - compact/summarize conversation history
- `/quit` - exit chat

## Available Tools

Default tool names from `hh tools` with default settings:

- `bash`
- `edit`
- `glob`
- `grep`
- `list`
- `question`
- `read`
- `skill`
- `todo_read`
- `todo_write`
- `web_fetch`
- `web_search`
- `write`

Notes:
- `task` is registered only when runtime task context is available.
- File tools are workspace-scoped by a file access controller.

## Agents

Built-in agents:
- `build` (primary)
- `plan` (primary)
- `explorer` (subagent, read-only)
- `general` (subagent, broader execution permissions)

Custom agents are discovered from:
- `./.agents/agents/*.md`
- `./.claude/agents/*.md`
- `~/.agents/agents/*.md`
- `~/.claude/agents/*.md`

## Configuration

`hh` uses JSON configuration files and merges them in order (lowest to highest precedence):

1. `~/.config/hh/config.json`
2. ancestor `.claude/settings.json`
3. ancestor `.hh/config.json`
4. ancestor `.claude/settings.local.json`
5. ancestor `.hh/config.local.json`

Environment overrides:
- `HH_MODEL`
- `HH_BASE_URL`
- `HH_API_KEY_ENV`
- `HH_SYSTEM_PROMPT`

Default provider API key variable:
- `OPENAI_API_KEY`

## Architecture

- `src/core/` - agent loop, domain types, traits, prompts
- `src/provider/` - provider adapters (OpenAI-compatible mapping)
- `src/tool/` - tool implementations and registry
- `src/session/` - session persistence and compaction
- `src/permission/` - policy and capability matching
- `src/config/` - settings model, loading, and overrides
- `src/cli/` - CLI commands and TUI runtime
- `src/agent/` - agent profiles and discovery

## Development

```bash
cargo check
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

See `AGENTS.md` for architecture constraints and project workflows.

## Session Storage

By default, sessions are stored under:

`~/.local/state/hh/sessions/<workspace-path>/`

## License

MIT
