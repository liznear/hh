## Context
You want two additions in this Rust CLI/TUI codebase:
1. Todo item management with both a tool and right-column display.
2. A file edit tool with a GitHub-style diff view in chat.

Decision for this iteration:
- Use `edit(path, old_string, new_string, replace_all?)` as the primary edit primitive.
- Defer `apply_patch`/hunk-style editing to a later iteration.

This repo already has good extension points:
- Tool trait + registry dispatch: `src/tool/mod.rs`, `src/tool/registry.rs`
- Workspace-aware file tooling pattern: `src/tool/fs.rs`
- Tool start-line presentation mapping: `src/cli/tui/tool_presentation.rs`
- TUI state/event flow: `src/cli/tui/app.rs`, `src/cli/tui/event.rs`
- Message and sidebar rendering: `src/cli/tui/ui.rs`

## Key constraints discovered in current code
1. `ToolEnd` currently carries only a preview string (truncated in agent loop), not full output.
   - `src/core/agent/mod.rs` uses `preview(...)` before `on_tool_end`.
   - `src/cli/tui/event.rs` and `src/core/traits.rs` mirror this preview-only contract.
   - Result: TUI cannot reliably parse full JSON diff payloads from `ToolEnd` today.
2. New tool names are denied unless permission matcher/config are updated.
   - `src/permission/matcher.rs` defaults unknown tools to deny.
3. Workspace path checks in `src/tool/fs.rs` are lightweight and should not be copied unchanged for security-sensitive edit operations.

## Updated implementation strategy (in dependency order)

### Phase 0: Event contract + safety prerequisites

#### 0.1 Make tool-end payload usable for structured UI
Update the agent event contract to optionally pass full sanitized output for selected tooling/UI paths.

Recommended minimal shape:
- Keep existing preview behavior for console UX.
- Extend event path with full output field (or a typed structured payload) for TUI state updates.

Files:
- `src/core/traits.rs` (extend `on_tool_end` signature or add a new callback)
- `src/core/agent/mod.rs` (send preview + full sanitized output)
- `src/cli/tui/event.rs` (`TuiEvent::ToolEnd` includes full output)
- `src/cli/render.rs` (keep using preview for terminal line output)

#### 0.2 Wire permissions/config for new tools
Add explicit permission routing for new tools.

Files:
- `src/config/settings.rs` (add permission fields for `todo_write` and `edit`)
- `src/permission/matcher.rs` (map both tool names)

Note: Keep defaults conservative (ask/deny aligned with existing write risk posture).

#### 0.3 Harden workspace boundary helper for edit tool
Implement a reusable strict path resolver that:
- resolves relative paths under workspace
- rejects traversal outside workspace
- handles canonicalization/symlink escape checks safely

Prefer placing this helper in `src/tool/fs.rs` (shared) or a small shared tool utility module.

---

### Phase 1: Add todo tool with typed state

#### 1.1 Tool shape
Create `src/tool/todo.rs` with a `todo_write` tool implementing `Tool`.

Use a canonical full-state API first:
- required: `todos: [{ content, status, priority }]`

Todo item schema:
- `content: String`
- `status: "pending" | "in_progress" | "completed" | "cancelled"`
- `priority: "high" | "medium" | "low"`

Return JSON output with:
- `todos`
- `counts` (`total`, `completed`, `in_progress`, etc.)

Why this shape:
- Matches common agent todo workflows (Codex/Claude/Crush style)
- Avoids fragile index-based mutation semantics
- Keeps UI rendering deterministic

#### 1.2 Registry wiring
Update:
- `src/tool/mod.rs` (export module)
- `src/tool/registry.rs` (register `todo_write` with FS tool set)

#### 1.3 TUI state integration
Update `src/cli/tui/app.rs`:
- Replace `todo_items: Vec<String>` with typed todo items (e.g., `Vec<TodoItemView>`).
- On `ToolEnd` for `todo_write` success, parse full JSON output and update todo state.
- Keep existing `extract_todos` only as fallback behavior when no tool-driven state is available.

#### 1.4 Sidebar rendering
Update `src/cli/tui/ui.rs`:
- Render todo rows with status markers:
  - pending/in_progress: `[ ] item`
  - completed: `[x] item` with muted style
  - cancelled: `[-] item` muted
- Add compact progress line under TODO header, for example `3/7 done`.
- Preserve truncation behavior (`...`) under height limits.

---

### Phase 2: Add edit tool + diff-focused rendering

#### 2.1 Tool design
Create `src/tool/edit.rs` with tool name `edit` (keep name consistent everywhere).

Chosen contract for v1:
- exact replacement API (`old_string` -> `new_string`) with optional `replace_all`
- explicit error on missing/ambiguous matches
- emit structured diff JSON for UI rendering

Deferred for later:
- patch/hunk input format (for example Begin Patch / unified patch envelopes)
- multi-file atomic patch application

Arguments:
- `path: String`
- `old_string: String`
- `new_string: String`
- `replace_all?: bool`

Execution flow:
1. Resolve and validate path with strict workspace boundary helper.
2. Read file text.
3. Apply replacement:
   - error if `old_string` missing
   - error on non-unique match when `replace_all=false`
4. Write file.
5. Generate unified diff from before/after.
6. Return JSON:
   - `path`
   - `applied`
   - `summary` (`added_lines`, `removed_lines`)
   - `diff`

#### 2.2 Diff generation
Add dependency in `Cargo.toml`:
- `similar`

Produce compact unified diff with standard prefixes (` `, `-`, `+`).

#### 2.3 Registry and presentation wiring
Update:
- `src/tool/mod.rs`
- `src/tool/registry.rs`
- `src/cli/tui/tool_presentation.rs` with start label `Edit <path>`

#### 2.4 Chat rendering for completed edit calls
Update `src/cli/tui/ui.rs` for `ChatMessage::ToolCall` when completed and `name == "edit"`:
- Parse structured output JSON.
- Render a diff block with:
  - header: path + `+A -R`
  - added lines in green-tinted style
  - removed lines in red-tinted style
  - context in muted style
- Add guardrails:
  - max rendered diff lines/chars
  - show `diff truncated` marker when exceeded
- Fallback to existing compact one-line tool rendering when parse fails or payload too large.

---

## Files to modify
- `Cargo.toml`
- `src/core/traits.rs`
- `src/core/agent/mod.rs`
- `src/cli/tui/event.rs`
- `src/cli/render.rs`
- `src/config/settings.rs`
- `src/permission/matcher.rs`
- `src/tool/mod.rs`
- `src/tool/registry.rs`
- `src/tool/fs.rs` (shared strict path helper)
- `src/tool/todo.rs` (new)
- `src/tool/edit.rs` (new)
- `src/cli/tui/app.rs`
- `src/cli/tui/tool_presentation.rs`
- `src/cli/tui/ui.rs`

## Testing and verification plan

### Unit tests
1. `tests/tool_tests.rs` additions:
   - `todo_write_set_updates_list`
   - `todo_write_rejects_invalid_args`
   - `edit_applies_single_replacement`
   - `edit_replace_all`
   - `edit_errors_when_old_string_missing`
   - `edit_errors_on_non_unique_match_when_replace_all_false`
   - `edit_respects_workspace_boundary`
   - `edit_rejects_parent_traversal`
   - `edit_rejects_symlink_escape` (if platform setup allows)

2. Event/TUI state tests:
   - `ToolEnd` carries full output to TUI state path
   - todo state updates from `todo_write` JSON output

3. `src/cli/tui/ui_tests.rs` additions:
   - sidebar todo rendering shows status markers and progress
   - completed/cancelled todo styles are muted
   - edit tool success renders diff header (`+/-` counts)
   - diff added/removed lines are present with style assertions
   - oversized diff falls back or truncates with explicit marker

4. Permission/config tests:
   - matcher routes `todo_write` and `edit` as configured
   - settings deserialize defaults with new permission keys

### End-to-end verification
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- manual run:
  - invoke `todo_write` and confirm sidebar updates immediately
  - invoke `edit` and confirm diff rendering in chat area
  - verify fallback path with malformed/oversized output

## Notes
- Keep changes additive and reversible.
- Do not remove existing compact tool rendering; use it as fallback.
- Keep todo state ephemeral in-memory unless a persistence requirement is added later.
- Prefer explicit, inspectable data flow over heuristic parsing.
- Replace-based edit is intentionally selected first for implementation simplicity, deterministic failures, and easier testability.
- If later needed, add a second tool (`apply_patch`) rather than overloading `edit`; keep both contracts explicit.
