# Happy Harness

Version: 0.1.0

Happy Harness (`hh`) is a terminal-based coding agent runtime with a TUI, provider-agnostic core domain types, and a configurable tool/permission system.

## Current Status

`hh` is usable for day-to-day coding workflows and still actively evolving.

- Core agent loop, tool execution, approvals, and termination are implemented.
- TUI chat, one-shot `run`, and frame replay debugging are implemented.
- Config loading, per-workspace sessions, and history compaction are implemented.
- OpenAI-compatible provider support is implemented.
- Multi-agent scaffolding and the `task` sub-agent protocol exist; the `task` tool is exposed when runtime task context is available.

## Features

- **Terminal TUI**: Interactive UI built with `ratatui`, markdown rendering, syntax highlighting, and debug frame dumping.
- **Provider-Agnostic Core**: Canonical LLM domain types and runtime traits shared across providers.
- **Tool Runtime**: File, shell, web, skill, question, and todo tools with schema-driven invocation.
- **Permission Policy**: Per-tool allow/ask/deny policy with capability overrides.
- **Session Persistence**: Workspace-aware storage and resume/compact workflows.
- **Agent Profiles**: Built-in agents (`build`, `plan`, `explorer`, `general`) plus custom Markdown-defined agents.
- **Image Input**: Clipboard and file-path image attachments for multimodal prompts.

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

# Debug/replay UI frames
hh run "debug this prompt" --debug ./debug
hh replay ./debug
```

## CLI Commands

- `hh chat [--debug <dir>] [--max-turns <n>] [--agent <name>]`
- `hh run <prompt> [--debug <dir>] [--max-turns <n>] [--agent <name>]`
- `hh replay <dir> [--delay <ms>] [--loop]`
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

Current default tool list from `hh tools`:

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

The `task` tool is runtime-context dependent and may not appear in `hh tools` outside task-enabled execution paths.

## Configuration

`hh` uses JSON config files:

- Global: `~/.config/hh/config.json`
- Claude project: `.claude/settings.json`
- Project: `.hh/config.json`
- Claude local project override: `.claude/settings.local.json`
- Local project override: `.hh/config.local.json`

Precedence (lowest -> highest): global, `.claude/settings.json`, `.hh/config.json`, `.claude/settings.local.json`, `.hh/config.local.json`.

### Key Settings

- `models.default` - selected model reference (`provider-id/model-id`)
- `providers` - provider definitions (`base_url`, `api_key_env`, model metadata)
- `agent` - runtime limits and behavior (`max_steps`, sub-agent settings, optional `system_prompt`)
- `tools` - enable/disable tool groups (`fs`, `bash`, `web`)
- `permissions` - per-tool policies (`allow`/`ask`/`deny`)
- `session.root` - session storage root
- `agents` - per-agent overrides (for example model selection)

### Environment Overrides

- `OPENAI_API_KEY` (default provider auth)
- `HH_MODEL` (override selected model)
- `HH_BASE_URL` (override selected provider base URL)
- `HH_API_KEY_ENV` (override selected provider API-key env var name)
- `HH_SYSTEM_PROMPT` (override runtime system prompt)

## Architecture

The project keeps LLM semantics in a provider-agnostic core and composes runtime behavior through traits.

- `src/core/` - runtime loop, domain types, traits, prompts
- `src/provider/` - provider adapters (OpenAI-compatible wire mapping)
- `src/tool/` - tool implementations and registry
- `src/session/` - session persistence and compaction
- `src/permission/` - policy and capability matching
- `src/config/` - settings model + loader/overrides
- `src/cli/` - commands, chat runtime, TUI/replay
- `src/agent/` - agent profile definitions and discovery

## Development

```bash
cargo check
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

Use `hh run ... --debug <dir>` or `hh chat --debug <dir>` to capture screen frames, then `hh replay <dir>` to inspect UI behavior.

See `AGENTS.md` for architecture constraints and debugging workflow details.

## Session Storage

Sessions are stored under `~/.local/state/hh/sessions/<workspace-path>/` and scoped by workspace.

## License

License not specified yet.
