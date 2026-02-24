## Context
You want two additions in this Rust CLI/TUI codebase:
1. **Todo item management** with both a tool for managing todo items and a right-column display.
2. A new **file edit tool** with UI rendering of edits in a GitHub code-review style diff.

Current architecture already has reusable patterns:
- Tool trait + registry dispatch: `src/tool/mod.rs`, `src/tool/registry.rs`
- File-system tool implementation pattern: `src/tool/fs.rs`
- Tool start-line presentation mapping: `src/cli/tui/tool_presentation.rs`
- TUI state and message model: `src/cli/tui/app.rs`
- Main rendering and sidebar rendering: `src/cli/tui/ui.rs`
- Existing tests to mirror style: `tests/tool_tests.rs`, `src/cli/tui/ui_tests.rs`

## Recommended implementation approach

### 1) Add a Todo management tool (and hook it into sidebar state)

#### 1.1 Tool design
Implement a new `todo_write` tool in `src/tool/todo.rs` following the existing `Tool` trait pattern (`schema()` + `execute()`), similar to `FsWrite` in `src/tool/fs.rs`.

Use a JSON command-style API in args:
- `action: "set"` with full `items` array (canonical source of truth)
- `action: "add"` with one item
- `action: "toggle"` with item index
- `action: "remove"` with item index
- `action: "clear"`

Todo item shape should support future UI state cleanly:
- `text: String`
- `done: bool`

Return structured JSON output string containing updated todo list and summary counts so TUI can parse it.

#### 1.2 Registry wiring
Update `src/tool/mod.rs` and `src/tool/registry.rs`:
- export new module
- register `todo_write` when FS tools are enabled (same settings gate pattern used now)

#### 1.3 TUI integration
Update `src/cli/tui/app.rs`:
- Replace `todo_items: Vec<String>` with typed todo items (`Vec<TodoItemView>` or shared type)
- Remove heuristic input parsing (`extract_todos`) as the primary source of todo state
- On `ToolEnd` for `todo_write`, parse tool output JSON and update `todo_items`

This reuses existing `TuiEvent::ToolStart/ToolEnd` flow in `src/cli/tui/event.rs` and avoids adding a new event type.

#### 1.4 Sidebar rendering
Update `src/cli/tui/ui.rs`:
- Replace string-only list rendering in `append_sidebar_list` with todo-aware rendering:
  - pending: `[ ] item`
  - done: `[x] item` (muted style)
- Add compact progress line under TODO header (e.g., `3/7 done`)
- Keep existing truncation behavior (`...`) for limited vertical space

---

### 2) Add file edit tool + GitHub-like diff rendering

#### 2.1 Tool design
Create `src/tool/edit.rs` with a new `edit` tool (or `file_edit`; choose one name and keep it consistent across registry + presentation + tests).

Implement operation model compatible with existing exact-replacement workflow:
- required: `path`, `old_string`, `new_string`
- optional: `replace_all: bool`

Execution flow:
1. Validate path in workspace (reuse workspace-boundary approach from `FsWrite` / `to_workspace_target` pattern in `src/tool/fs.rs`).
2. Read current file.
3. Apply replacement with explicit failure when `old_string` is not found (and when non-unique with `replace_all=false` if required).
4. Write file.
5. Generate unified diff from before/after text.
6. Return JSON output with:
   - `path`
   - `applied` (bool)
   - `summary` (`added_lines`, `removed_lines`)
   - `diff` (unified diff text)

#### 2.2 Diff generation
Add dependency in `Cargo.toml`: `similar` (for robust text diff generation).

Generate a compact unified diff suitable for terminal rendering, with line prefixes:
- context: ` `
- removals: `-`
- additions: `+`

#### 2.3 Tool presentation
Update `src/cli/tui/tool_presentation.rs`:
- add presentation entry for the new edit tool
- start label format like: `Edit <path>`

#### 2.4 Message rendering with diff view
Update `src/cli/tui/ui.rs` rendering for `ChatMessage::ToolCall` when completed and tool is edit tool:
- parse `output` as JSON
- if JSON contains `diff`, render a GitHub-like block:
  - header line: file path + `+A -R` counts
  - each diff line styled:
    - `+` green-ish foreground/background
    - `-` red-ish foreground/background
    - context muted/neutral
- fallback gracefully to current compact line rendering when output is invalid / too large

No new `ChatMessage` variant is required; this keeps changes small and leverages current tool-call pipeline.

---

## Files to modify
- `Cargo.toml`
- `src/tool/mod.rs`
- `src/tool/registry.rs`
- `src/tool/fs.rs` (optional small helper reuse extraction only if needed)
- `src/tool/todo.rs` (new)
- `src/tool/edit.rs` (new)
- `src/cli/tui/app.rs`
- `src/cli/tui/tool_presentation.rs`
- `src/cli/tui/ui.rs`

## Reuse points from existing code
- Tool contract and result shape: `src/tool/mod.rs`
- Registration helper pattern: `src/tool/registry.rs`
- Workspace path safety checks: `src/tool/fs.rs`
- Tool event lifecycle into TUI: `src/core/agent/mod.rs` + `src/cli/tui/event.rs`
- Sidebar list behavior/truncation patterns: `src/cli/tui/ui.rs`
- UI test style helpers (`line_text`, span assertions): `src/cli/tui/ui_tests.rs`

## Testing and verification plan

### Unit tests
1. `tests/tool_tests.rs` additions:
   - `todo_write_set_updates_list`
   - `todo_write_toggle_and_remove`
   - `todo_write_rejects_invalid_args`
   - `edit_applies_single_replacement`
   - `edit_replace_all`
   - `edit_errors_when_old_string_missing`
   - `edit_respects_workspace_boundary`

2. `src/cli/tui/ui_tests.rs` additions:
   - sidebar todo rendering shows checkboxes and progress
   - completed todo uses muted style
   - edit tool success renders diff header (`+/-` counts)
   - added/removed diff lines are present and colored via style assertions

### End-to-end verification
- `cargo fmt --all`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- Run app manually and verify:
  - invoke `todo_write` actions and confirm right-column TODO updates immediately
  - invoke `edit` tool and confirm diff-like rendering appears in conversation area

## Notes / constraints
- Keep scope focused: no persistence layer unless already present elsewhere.
- Keep backward compatibility for current rendering paths by falling back to compact tool lines when structured parsing fails.
- Enforce workspace boundary checks in edit tool to match existing security behavior.