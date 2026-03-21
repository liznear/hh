# Session Design

## Overview

The session package provides structured state management for conversation sessions. Each session contains multiple turns, where each turn represents a single user-assistant interaction and tracks the model used.

## Core Types

### State

```go
type State struct {
    ID           string    // Unique session identifier (e.g., "20260321-143052")
    CreatedAt    time.Time // When the session was created
    CurrentModel string    // Current model for new turns
    Turns        []*Turn   // Ordered list of conversation turns
}
```

Key methods:
- `NewState(modelName)` - Create a new session
- `SetModel(modelName)` - Change the current model (affects future turns)
- `StartTurn()` - Begin a new turn with the current model
- `CurrentTurn()` - Get the active turn
- `AddItem(item)` - Add an item to the current turn
- `AllItems()` - Get all items across all turns
- `ItemCount()` - Total item count across turns

### Turn

```go
type Turn struct {
    ID        string     // Turn identifier
    ModelName string     // Model used for this turn
    StartedAt time.Time  // When the turn started
    EndedAt   *time.Time // When the turn completed (nil if in progress)
    Items     []Item     // Ordered list of items in this turn
}
```

Each turn captures:
- The model that was used (important for resuming or analyzing conversations)
- Timing information
- All items (messages, tool calls, etc.)

### Items

Items are typed structures that represent different content in a conversation:

| Type | Description |
|------|-------------|
| `Start` | Turn start. |
| `UserMessage` | User's input text. |
| `AssistantMessage` | Assistant's response text |
| `ThinkingBlock` | Assistant's thinking/reasoning |
| `ToolCallItem` | Tool invocation with status and result |
| `ErrorItem` | Error messages |
| `End` | Turn end. |

Each item has type, data (item-type-specific type), and timestamp.

#### ToolCallItem

```go
type ToolCallItem struct {
    ID        string
    Name      string        // Tool name (e.g., "GrepTool", "Bash")
    Arguments string        // JSON arguments
    Result    *tools.ToolResult // Nil if pending. Use Result.IsErro to decide if success or error.
}
```

## Tool Result Parsing

The `Arguments` and `Result.Result` allow the UI to display meaningful summaries.

It should cover the existing tools in tools package.

## Storage

Sessions are persisted in `~/.config/hh/sessions/<project-name>/` with two files per session:

<project-name> = replace_all(cwd, "/", "--")

### `<id>.meta.json` - Session Metadata

```json
{
  "id": "20260321-143052",
  "created_at": "2026-03-21T14:30:52Z",
  "current_model": "claude-3-opus",
  "turn_count": 3
}
```

### `<id>.jsonl` - Items (JSON Lines)

Each line is a JSON object with turn number and item:

```json
{"turn_num":1,"timestamp": "...", "item":{"type":"start","data":{"model":"glm-5"}}}
{"turn_num":1,"timestamp": "...", "item":{"type":"user_message","data":{"content":"Find all TODOs"}}}
{"turn_num":1,"timestamp": "...", "item":{"type":"tool_call","data":{"name":"GrepTool","arguments":"{\"pattern\":\"TODO\"}","status":"success","result":{"parsed":{"match_count":5,"file_count":2}}}}
{"turn_num":1,"timestamp": "...", "item":{"type":"assistant_message","data":{"content":"I found 5 TODOs..."}}}
{"turn_num":1,"timestamp": "...", "item":{"type":"end","data":{}}}
{"turn_num":2,"timestamp": "...", "item":{"type":"user_message","data":{"content":"Show me the first one"}}}
```

### Storage API

```go
storage, _ := NewStorage("/path/to/sessions")

// Save session
storage.SaveMeta(state)
storage.AppendItem(sessionID, turnNum, item)

// Load session
state, _ := storage.Load(sessionID)

// List sessions
metas, _ := storage.List()

// Delete session
storage.Delete(sessionID)
```

## Turn-Based Model Tracking

Model switching is tracked per-turn:

1. `State.CurrentModel` is the model for **new** turns
2. Each `Turn.ModelName` records which model was used
3. Changing `CurrentModel` doesn't affect in-progress turns

Example flow:
```
Session starts with claude-3-opus
  Turn 1: model=claude-3-opus, user asks question, assistant responds
  
User switches model to claude-3-sonnet
  State.CurrentModel = claude-3-sonnet
  
  Turn 2: model=claude-3-sonnet, user asks follow-up, assistant responds
  
Turn 1 still shows claude-3-opus was used
```

This enables:
- Accurate conversation history
- Model-specific behavior when resuming
- Analysis of which model handled which turns

## Integration with TUI

The app model uses session state instead of raw strings:

```go
type model struct {
    session   *session.State
    toolCalls map[string]*session.ToolCallItem  // Pending tool calls by key
    // ...
}
```

Event handling:
- `StartTurn()` called when user sends message
- `AddItem()` for each event (thinking, message delta, tool call)
- `Turn.End()` when assistant finishes

Rendering:
- `AllItems()` iterates through all items
- Each item type has dedicated rendering logic
- Tool calls show parsed summaries when available
