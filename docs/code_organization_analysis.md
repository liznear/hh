# Code Organization Analysis

## Current Structure: By Technical Layer

```
app.go                    (295 lines) - Core model, Update switch
app_state.go              (298 lines) - State types, agent event handling
app_update_input.go       (111 lines) - Input handling
app_update_stream.go      ( 46 lines) - Stream handlers
app_update_run.go         ( 56 lines) - Run lifecycle
app_update_dialog.go      ( 22 lines) - Dialog routing
app_update_scroll.go      ( 27 lines) - Scroll handlers
app_update_branches.go    ( 13 lines) - Window resize
app_update_helpers.go     ( 86 lines) - Run helpers
app_view.go               (321 lines) - View composition
app_widgets.go            (637 lines) - Widget renderers
app_scroll.go             (465 lines) - Scroll logic + list rendering
app_layout.go             ( 77 lines) - Layout computation
app_shell.go              ( 73 lines) - Shell mode
app_stream.go             ( 86 lines) - Stream commands
app_markdown_cache.go     (148 lines) - Markdown rendering
```

**Total**: 17 files, ~3,261 lines (excluding tests)

## Problems with Current Structure

1. **Navigation Friction**: Following a feature requires jumping across multiple files
   - Example: Understanding "agent run" requires looking at:
     - `app_update_input.go` (Enter key → `beginAgentRun`)
     - `app_update_run.go` (run lifecycle)
     - `app_update_stream.go` (stream handlers)
     - `app_state.go` (event handling)
     - `app_update_helpers.go` (finalization)

2. **Many Small Files**: 5 files under 100 lines create more cognitive overhead than value
   - `app_update_branches.go` (13 lines)
   - `app_update_dialog.go` (22 lines)
   - `app_update_scroll.go` (27 lines)

3. **Artificial Boundaries**: Split by "update handler" vs "helper" vs "state mutation"
   - `finalizeRun` is in "helpers" but it's core run lifecycle
   - `handleAgentEvent` is in "state" but it's event processing

## Crush's Approach: Giant File with Short Switch Cases

Looking at the Crush implementation (reference codebase for this architecture):

### Structure
- **`model/ui.go`**: 3,531 lines, 64 methods on the model
- **`chat/`**: Domain-specific rendering logic (agent, bash, file, etc.)

### Key Insights

1. **Update switch cases are SHORT** (1-5 lines each)
   ```go
   case tea.KeyPressMsg:
       if cmd := m.handleKeyPressMsg(msg); cmd != nil {
           cmds = append(cmds, cmd)
       }
   case tea.WindowSizeMsg:
       m.width, m.height = msg.Width, msg.Height
       m.updateLayoutAndSize()
   ```

2. **Handlers stay in the same file** (or move to domain packages)
   - `handleKeyPressMsg`, `handlePasteMsg`, `handleDialogMsg` are all in `ui.go`
   - Complex domain logic (like chat rendering) goes to `chat/` package
   - No artificial split by "update handler" vs "state mutation" vs "helper"

3. **Navigation is easy**
   - Everything about the main model is in ONE place (`ui.go`)
   - Switch cases are short enough to scan quickly
   - Handler functions are just below in the same file
   - Complex domains (chat) have their own package

### Why This Works

- **3,500 lines is manageable** with good editor support (outline, search)
- **Short switch cases** make the flow obvious
- **Handlers in same file** eliminate jumping
- **Domain packages** only for complex, reusable logic

## Proposed Approach: Follow Crush's Pattern

### Option A: Single Giant File (Recommended - like Crush)

```
app.go                    - EVERYTHING (~3,000-3,500 lines)
                            - Model + state types
                            - Update with giant switch
                            - All handlers (input, agent, scroll, etc.)
                            - All state mutations
                            - Helper functions

app_view.go               - All rendering (~1,500 lines)
                            - View composition
                            - All widget renderers
                            - Layout + markdown

app_scroll.go             - Scroll domain (keep separate, ~500 lines)
                            - Complex scroll logic
                            - List rendering
```

**Benefits**:
- Follow proven Crush architecture
- Single source of truth for all model behavior
- Short switch cases make flow obvious
- No jumping between files for handlers
- Editor outline/navigation handles size

**Trade-offs**:
- Large file (but Crush proves it works)
- Requires good editor support

### Option B: Domain-Based (if single file feels too large)

```
app.go                    - Model, state, Update switch (~500 lines)
app_handlers.go           - All update handlers (~800 lines)
app_state.go              - State mutations (~500 lines)
app_view.go               - All rendering (~1,500 lines)
app_scroll.go             - Scroll domain (~500 lines)
```

**Benefits**:
- Smaller files if that's your preference
- Still eliminates most jumping
- Groups related functions

**Trade-offs**:
- More files than Option A
- Some jumping between handlers and state mutations

## Recommendation: Option A (Single Giant File)

### File Breakdown (Option A: Single Giant File)

#### `app.go` (~3,000-3,500 lines)
**Everything about the model:**
- Model type definition
- State types (domainState, uiState, runtimeState)
- Constructor functions
- `Update` method with giant switch (short cases, routes to handlers)
- `Init` and utility methods
- **All update handlers:**
  - Keyboard: `handleKeyPressMsg`, `handleEnterKey`
  - Mouse: `handleMouseWheelMsg`
  - Dialogs: `handleDialogKeyPress`
  - Window: `handleWindowSizeMsg`
  - Stream: `handleStreamBatchMsg`, `handleAgentEventMsg`, etc.
  - Shell: `handleShellCommandDoneMsg`
- **All agent logic:**
  - Run lifecycle: `beginRun`, `beginAgentRun`, `beginShellRun`, `finalizeRun`
  - Event processing: `handleAgentEvent` + all event type handlers
  - State mutations: `appendThinkingDelta`, `appendMessageDelta`, `addToolCall`, `completeToolCall`
- **Commands:**
  - `startAgentStreamCmd`, `waitForStreamCmd`
  - Shell commands
- **Message types:** All message type definitions

#### `app_view.go` (~1,500 lines)
**All rendering:**
- Main view: `View`, `buildFrameViewModel`, `renderFrame`
- Layout: `computeLayout`, `layoutState`
- Pane renderers: `renderMessagePane`, `renderInputPane`, `renderMainPane`, etc.
- Widget renderers: all `render*Widget` functions
- Markdown/cache: rendering and caching logic

#### `app_scroll.go` (~500 lines)
**Scroll domain (keep separate):**
- Scroll logic: `handleScrollKey`, `scrollListBy`, `isListAtBottom`
- List rendering: `renderMessageList`, `renderItemLines`
- Performance tracking
- (Already well-organized, keep as-is)

### Navigation Examples

**Understanding "agent run" flow:**
1. Open `app.go`
2. Search for "Enter key" in Update switch → see `handleKeyPressMsg` call
3. Jump to `handleKeyPressMsg` (same file) → see `beginAgentRun` call
4. Jump to `beginAgentRun` (same file) → see all agent logic
5. **Total: 1 file** (vs 5+ currently)

**Understanding "scroll behavior":**
1. Open `app.go`
2. Search for "scroll" in Update switch → see handler calls
3. Jump to `app_scroll.go` for detailed scroll logic
4. **Total: 2 files** (same as current)

**Understanding "rendering":**
1. Open `app_view.go`
2. All rendering logic is in one place
3. **Total: 1 file** (vs 3+ currently)

## Implementation Strategy (Option A: Single Giant File)

### Phase 1: Consolidate all model logic into `app.go`
1. Move all handlers from `app_update_*.go` files into `app.go`
2. Move agent event handling from `app_state.go` into `app.go`
3. Move stream commands from `app_stream.go` into `app.go`
4. Move shell logic from `app_shell.go` into `app.go`
5. Delete all the small split files

### Phase 2: Consolidate rendering into `app_view.go`
1. Merge `app_layout.go` into `app_view.go`
2. Merge `app_widgets.go` into `app_view.go`
3. Merge `app_markdown_cache.go` into `app_view.go`

### Phase 3: Keep `app_scroll.go` separate
- Already well-organized as a domain
- Complex enough to deserve its own file

### Phase 4: Clean up
- Update any cross-file references
- Ensure tests still pass
- Update documentation
- Verify Update switch cases are SHORT (1-5 lines each)

## Result: 3 Files Instead of 17

```
app.go          (~3,000-3,500 lines) - All model logic
app_view.go     (~1,500 lines)       - All rendering
app_scroll.go   (~500 lines)         - Scroll domain
```

## Why This Works (Evidence from Crush)

Crush successfully uses this pattern:
- Single `ui.go` file with 3,531 lines
- 64 methods on the model
- Short switch cases (1-5 lines)
- All handlers in the same file
- Complex domains (chat rendering) in separate packages

**Key insight**: Large files are NOT a problem if:
1. The file has a single clear purpose (the model)
2. Switch cases are short and delegate to handlers
3. You have editor support for navigation (outline, search)
4. Complex domains are extracted to packages when needed

## When to Extract to Packages

Consider creating a package (like `chat/` in Crush) when:
- Logic is complex enough to deserve multiple files
- Logic is reusable across the application
- Logic has clear boundaries and its own types

For this codebase:
- `app_scroll.go` is a good candidate for extraction if it grows
- Rendering widgets could move to a `widgets/` package if they become complex
- Agent interaction could move to an `agent/` package if it develops more complexity

**Start simple**: One giant file, extract only when complexity demands it.

## Metrics for Success

- **Average files to jump through for a feature**: 1-2 (currently ~3-5)
- **Files under 100 lines**: 0 (currently 5)
- **Total files**: 3 (currently 17)
- **Update switch cases**: All short (1-5 lines), just routing to handlers
- **Largest file**: ~3,500 lines (acceptable, matches Crush)
- **Editor requirement**: Need outline/symbol navigation (standard in modern editors)

## Key Principle: Short Switch Cases

The most important lesson from Crush is that the Update switch should have **short cases**:

```go
// ✅ Good: Short case, delegates to handler
case tea.KeyPressMsg:
    if cmd := m.handleKeyPressMsg(msg); cmd != nil {
        cmds = append(cmds, cmd)
    }

// ❌ Bad: Long case with inline logic
case tea.KeyPressMsg:
    if m.dialog != nil {
        // 20 lines of dialog handling...
    }
    if m.busy {
        // 15 lines of busy state handling...
    }
    // ... more inline logic
```

**Why short cases matter:**
- Easy to scan the switch and understand routing
- Handler functions have descriptive names
- Logic is organized by function, not by message type
- Easier to test individual handlers
