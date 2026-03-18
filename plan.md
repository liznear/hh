I want to extract the runner (in @src/core) as a separate crate hh-coding-agent.

This hh-coding-agent crate should cover:

1. The interaction with the agent loop (hh-agent).
2. The tool execution and all tools.
3. Approval workflow.
4. Event channel for input and output. This is how outer layer interacts with this crate.

This hh-coding-agent crate should not have:

1. Session transcript storage
2. Any UI related

Basically, this crate should provides all functionality a coding agent should provide, except the UI for end uesr.

If someone wants to implement their own coding agent, they can just use this crate.

---

## Detailed Extraction Plan

### Current Architecture

```
hh-cli (main crate)
├── src/core/           # Agent runner, types, traits ← EXTRACT
├── src/tool/           # All tool implementations     ← EXTRACT
├── src/permission/     # Permission matching          ← EXTRACT
├── src/safety/         # Output sanitization          ← EXTRACT
├── src/config/         # Settings (partial)           ← EXTRACT (types only)
├── src/session/        # Session storage              ← KEEP IN hh-cli
├── src/provider/       # Provider implementations     ← KEEP IN hh-cli
├── src/app/            # TUI                          ← KEEP IN hh-cli
├── src/cli/            # CLI commands                 ← KEEP IN hh-cli
└── src/agent/          # Agent config/registry        ← KEEP IN hh-cli

hh-agent (existing crate)
├── AgentLoop, Provider trait, basic types (Message, ToolResult, etc.)
```

### Target Architecture

```
hh-agent (existing - minimal agent loop)
├── AgentLoop, Provider trait
├── Basic types: Message, ToolResult, ToolSchema, Role, etc.

hh-coding-agent (NEW - coding agent functionality)
├── AgentCore, AgentRunner
├── All tools (bash, fs, edit, web, todo, question, task, skill)
├── Permission/approval system
├── RunnerInput/RunnerOutput (event channel)
├── Traits: ToolExecutor, ApprovalPolicy, SessionSink, SessionReader
├── Domain types: TodoItem, QuestionPrompt, ApprovalRequest, etc.
├── System prompts
└── Settings types (for tools/permissions only)

hh-cli (existing - UI + persistence)
├── SessionStore implementation
├── Provider implementations (OpenAI compatible)
├── TUI (app/, components/)
├── CLI commands
├── Config loading (from files)
└── Agent registry
```

### Modules to Extract to hh-coding-agent

#### 1. Core Agent (`src/core/`)
- `core/agent/mod.rs` → `AgentCore`, `RunnerOutputObserver`
- `core/agent/runner.rs` → `AgentRunner` (main execution logic)
- `core/agent/output_channel.rs` → `RunnerOutputChannel`
- `core/agent/subagent_manager.rs` → `SubagentManager`
- `core/agent/types.rs` → `RunnerInput`, `RunnerOutput`, `RunnerState`, `StatePatch`
- `core/traits.rs` → `ToolExecutor`, `ApprovalPolicy`, `ApprovalDecision`, `ApprovalChoice`, `ApprovalRequest`, `SessionSink`, `SessionReader`, `QueuedUserMessage`
- `core/types.rs` → `QuestionPrompt`, `TodoItem`, `TodoStatus`, `TodoPriority`, `SubAgentCall`, `SubAgentResult`
- `core/system_prompt.rs` → System prompt functions
- `core/prompts/*.md` → Prompt templates

#### 2. Tools (`src/tool/`)
- `tool/mod.rs` → `Tool` trait, `ToolExecution`, `ToolResult` (re-export from hh-agent)
- `tool/schema.rs` → `ToolSchema`
- `tool/bash.rs` → `BashTool`
- `tool/fs.rs` → `FsRead`, `FsWrite`, `FsList`, `FsGlob`, `FsGrep`, `FileAccessController`
- `tool/edit.rs` → `EditTool`
- `tool/diff.rs` → Diff utilities
- `tool/web.rs` → `WebFetchTool`, `WebSearchTool`
- `tool/todo.rs` → `TodoReadTool`, `TodoWriteTool`
- `tool/question.rs` → `QuestionTool`
- `tool/task.rs` → `TaskTool`
- `tool/skill.rs` → `SkillTool`
- `tool/registry.rs` → `ToolRegistry`

#### 3. Permission (`src/permission/`)
- `permission/mod.rs` → `PermissionMatcher`
- `permission/matcher.rs` → `PermissionMatcher` (implements `ApprovalPolicy`)
- `permission/rules.rs` → `PermissionRule`, `RuleContext`
- `permission/policy.rs` → `Decision` enum

#### 4. Safety (`src/safety/`)
- `safety/mod.rs` → `sanitize_tool_output()`

#### 5. Config Types (partial, from `src/config/`)
- `config/settings.rs` → Extract only:
  - `ToolSettings`
  - `PermissionSettings`
  - `AgentSettings` (partial - max_steps, sub_agent settings)
  - Helper functions for default values
- Keep in hh-cli:
  - `Settings` struct (aggregate)
  - `ModelSettings`, `ProviderConfig`, `ModelMetadata`
  - `SessionSettings`
  - Config loading functions (`loader.rs`)

### Modules to Keep in hh-cli

1. **Session Storage** (`src/session/`)
   - `SessionStore` implementation
   - `SessionEvent`, `SessionMetadata` types
   - These depend on file I/O and are persistence concerns

2. **Provider Implementations** (`src/provider/`)
   - `OpenAICompatibleProvider`
   - These are concrete implementations of the `Provider` trait

3. **UI** (`src/app/`)
   - All TUI components
   - Event handling

4. **CLI** (`src/cli/`)
   - Command parsing
   - Run/chat modes

5. **Config Loading** (`src/config/loader.rs`)
   - File-based config loading
   - Path resolution

6. **Agent Registry** (`src/agent/`)
   - Agent configuration loading
   - Registry management

### Key Design Decisions

#### 1. Session Traits vs Implementation
- **In hh-coding-agent:** `SessionSink`, `SessionReader` traits
- **In hh-cli:** `SessionStore` implementation
- **Rationale:** Users of hh-coding-agent can implement their own persistence strategy

#### 2. Settings Split
- **In hh-coding-agent:** Types needed for tool/permission configuration
  - `ToolSettings`, `PermissionSettings`, `AgentSettings` (partial)
- **In hh-cli:** Full `Settings` struct, model/provider config, loading logic
- **Rationale:** Config loading is infrastructure; only the data types are needed by the agent

#### 3. SubagentManager
- **In hh-coding-agent:** `SubagentManager` (made generic over session persistence)
- **Rationale:** Sub-agent orchestration is core agent functionality
- **Change needed:** Make `SubagentManager` accept a generic session sink instead of `SessionStore`

#### 4. ToolRegistry
- **In hh-coding-agent:** `ToolRegistry` with its context types
- **Rationale:** Tool registration and execution is core functionality

### Dependencies for hh-coding-agent

```toml
[package]
name = "hh-coding-agent"
version = "0.1.0"
edition = "2024"

[dependencies]
hh-agent = { path = "../hh-agent" }
anyhow = "1.0"
async-trait = "0.1"
futures = "0.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.46", features = ["macros", "rt", "sync", "time", "process", "io-util"] }
glob = "0.3"
regex = "1"  # if needed for grep tool
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
uuid = { version = "1.11", features = ["v4", "v7", "serde"] }
base64 = "0.22"
urlencoding = "2.1"

[dev-dependencies]
tempfile = "3.15"
```

### Implementation Steps

#### Phase 1: Create Crate Structure
1. Create `hh-coding-agent/` directory
2. Create `Cargo.toml` with dependencies
3. Create `src/lib.rs` with module declarations
4. Update root `Cargo.toml` to add workspace member

#### Phase 2: Extract Core Types and Traits
1. Move `src/core/types.rs` → `hh-coding-agent/src/core/types.rs`
2. Move `src/core/traits.rs` → `hh-coding-agent/src/core/traits.rs`
3. Update imports, remove session-specific types from traits if needed

#### Phase 3: Extract Agent Runner
1. Move `src/core/agent/types.rs` → `hh-coding-agent/src/core/agent/types.rs`
2. Move `src/core/agent/runner.rs` → `hh-coding-agent/src/core/agent/runner.rs`
3. Move `src/core/agent/mod.rs` → `hh-coding-agent/src/core/agent/mod.rs`
4. Move `src/core/agent/output_channel.rs`
5. Move `src/core/agent/subagent_manager.rs` (may need refactoring for generic session)

#### Phase 4: Extract Tools
1. Move `src/tool/mod.rs` and all tool files
2. Move `src/tool/registry.rs`
3. Update `ToolRegistry` to work with extracted settings types

#### Phase 5: Extract Permission System
1. Move `src/permission/` directory
2. Update `PermissionMatcher` to work with extracted settings types

#### Phase 6: Extract Safety
1. Move `src/safety/mod.rs`

#### Phase 7: Extract System Prompts
1. Move `src/core/system_prompt.rs`
2. Move `src/core/prompts/*.md`

#### Phase 8: Extract Settings Types
1. Extract `ToolSettings`, `PermissionSettings`, `AgentSettings` from `src/config/settings.rs`
2. Create `hh-coding-agent/src/config/settings.rs` with just these types
3. Add conversion/adapter if needed

#### Phase 9: Update hh-cli
1. Add `hh-coding-agent` dependency to `hh-cli/Cargo.toml`
2. Update all imports in hh-cli to use types from hh-coding-agent
3. Remove moved files from hh-cli
4. Implement `SessionSink`/`SessionReader` for `SessionStore` (adapter pattern)

#### Phase 10: Tests and Validation
1. Move relevant tests to hh-coding-agent
2. Ensure all tests pass
3. Run `cargo clippy -- -D warnings`
4. Run `cargo fmt --check`

### File Movement Summary

```
FROM hh-cli/src/                    TO hh-coding-agent/src/
─────────────────────────────────────────────────────────────────
core/mod.rs                    →   core/mod.rs
core/types.rs                  →   core/types.rs
core/traits.rs                 →   core/traits.rs
core/system_prompt.rs          →   core/system_prompt.rs
core/prompts/*.md              →   core/prompts/*.md
core/agent/mod.rs              →   core/agent/mod.rs
core/agent/runner.rs           →   core/agent/runner.rs
core/agent/types.rs            →   core/agent/types.rs
core/agent/output_channel.rs   →   core/agent/output_channel.rs
core/agent/subagent_manager.rs →   core/agent/subagent_manager.rs
tool/*                         →   tool/*
permission/*                   →   permission/*
safety/*                       →   safety/*
(partial) config/settings.rs   →   config/settings.rs
```

### Public API of hh-coding-agent

```rust
// Core agent
pub use core::{AgentCore, RunnerOutputObserver};
pub use core::agent::{RunnerInput, RunnerOutput, RunnerState, StatePatch};

// Traits
pub use core::traits::{
    ToolExecutor, ApprovalPolicy, ApprovalDecision, ApprovalChoice,
    ApprovalRequest, SessionSink, SessionReader, QueuedUserMessage,
};

// Domain types
pub use core::types::{
    QuestionPrompt, QuestionOption, QuestionAnswer, QuestionAnswers,
    TodoItem, TodoStatus, TodoPriority,
    SubAgentCall, SubAgentResult,
};

// Tools
pub use tool::{Tool, ToolExecution, ToolRegistry, ToolSchema};
pub use tool::registry::ToolRegistryContext;

// Permission
pub use permission::{PermissionMatcher, Decision as PermissionDecision};

// Safety
pub use safety::sanitize_tool_output;

// Config (types only)
pub use config::{ToolSettings, PermissionSettings, AgentSettings};

// System prompts
pub use core::system_prompt::{
    default_system_prompt, build_system_prompt, plan_system_prompt,
    explorer_system_prompt, general_system_prompt,
};
```
