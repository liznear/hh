# TUI Migration: ratatui → iocraft

**Goal:** Replace the current TUI implementation (ratatui-based) with iocraft while preserving the existing visual design and user experience.

**Architecture:** 
- Keep `AppState`, action dispatch, and event semantics unchanged
- Introduce UI-agnostic primitives layer between state and rendering
- Implement iocraft backend alongside ratatui, then cutover after parity verification
- Port components incrementally with visual regression checks

**Final Completion Criteria:**
- All existing TUI features render identically under iocraft
- `cargo test` passes
- `cargo clippy -- -D warnings` passes
- Manual parity verification via debug frame capture/replay matches baseline
- Performance remains acceptable (p95 < 16ms for typical workloads)

---

## TUI Layout Reference

### Overall Structure

```
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ 1px padding                                                                                    │
│   ┌────────────────────────────────────────────────────────────┐  ┌────────────────────────┐   │
│ 1 │                                                            │  │                        │   │
│ p │                     MESSAGES AREA                          │  │      SIDEBAR           │   │
│ x │                                                            │  │                        │   │
│   │   • Scrollable content                                     │  │   • Session name        │   │
│   │   • Auto-follows new messages                              │  │   • Context usage       │   │
│   │   • Text selection with mouse drag                         │  │   • Modified files     │   │
│   │                                                            │  │   • TODO list          │   │
│ L │                                                            │  │   • Foldable sections  │   │
│ A │────────────────────────────────────────────────────────────│  │                        │   │
│ Y │                                                            L  │                        │   │
│ O │   PROCESSING INDICATOR (1 row, only when agent running)    A  │                        │   │
│ U │────────────────────────────────────────────────────────────Y  │                        │   │
│ T │   ┌──────────────────────────────────────────────────────┐ O  │                        │   │
│   │   │ ▌ [cursor] Input text here...                         │ U  │                        │   │
│ I │   │                                                        │ T  │                        │   │
│ N │   │ AgentName  Provider Model                        1s  │    │                        │   │
│ P │   └──────────────────────────────────────────────────────┘    │                        │   │
│ U └────────────────────────────────────────────────────────────────┴────────────────────────┘   │
│ T                                                                                               │
└────────────────────────────────────────────────────────────────────────────────────────────────┘

     ◄────────────────────────────────────────────────────────────────────────────────────────►
                                      Main column (flexible width)                         Sidebar (38 cols)
```

### Horizontal Layout Calculation

```
Terminal Width = W
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                                                                                │
│  ┌─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┐  ┌─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐  │
│  │                                                                 │  │                     │  │
│  │                                                                 │  │                     │  │
│  │         Main Column                                            │  │    Sidebar          │  │
│  │                                                                 │  │                     │  │
│  │                                                                 │  │                     │  │
│  └─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┘  └─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘  │
│                                                                                                │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
  ↑                              ↑                                 ↑                     ↑
  1px                           W - 1 - 38 - 2                      38                    1px
  padding                       main_width                          sidebar_width         padding
                                = W - 42                             = 38
```

### Vertical Layout Calculation

```
Terminal Height = H
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ 1px padding                                                                                    │
├────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                │
│                                                                                                │
│                              Messages Area                                                     │
│                              (fills remaining space)                                           │
│                                                                                                │
│                                                                                                │
│                                                                                                │
├────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Processing Indicator (1 row, only when agent is running)                                      │
├────────────────────────────────────────────────────────────────────────────────────────────────┤
│ Input Panel Border                                                                             │
│ ▌ Input content (1-5 lines)                                                                    │
│ Status line (agent/provider/model/duration)                                                    │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
  ↑                     ↑                ↑                   ↑                        ↑
  1px                  H - 1 - 1         1 row               variable                bottom of
  padding              - input_h          (conditional)       input_h                 terminal
```

### Sidebar Content Layout

```
┌────────────────────────────────┐
│                                │
│  Session Name                  │  (bold, primary text color)
│                                │
│  → Subagent Session 1          │  (indented, if in subagent context)
│  → Subagent Session 2          │
│                                │
│  ~/path/to/project @ branch    │  (abbreviated path, git branch)
│                                │
│  ▼ Context                     │  (foldable if > 7 lines)
│    45 / 128 (35%)              │  (color based on %: green/yellow/orange/red)
│                                │
│  ▼ Modified Files              │  (foldable if > 7 lines)
│    src/main.rs    +5 -2        │  (file path, +/- line counts)
│    src/lib.rs     +3          │
│                                │
│  ▼ TODO (2 / 5)                │  (foldable if > 7 lines)
│    2 / 5 done                   │
│    [✓] Completed task          │  (checkmark, muted text)
│    [ ] In-progress task        │  (orange text for active)
│    [ ] Pending task            │  (normal text)
│                                │
└────────────────────────────────┘
```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│                                                                                      │ Page BG
│  ┌─────────────────────────────────────────────────────┐ ┌────────────────────────┐  │
│  │                                                     │ │                      │  │
│  │                    MESSAGES                        │ │      SIDEBAR         │  │
│  │                    AREA                            │ │                      │  │
│  │                                                     │ │                      │  │
│  │  (scrollable, auto-follow on new content)           │ │  (scrollable)        │  │
│  │                                                     │ │                      │  │
│  │                                                     │ │  Context: 45/128    │  │
│  │                                                     │ │  Modified Files     │  │
│  │                                                     │ │  TODO (2/5)         │  │
│  │                                                     │ │                      │  │
│  ├─────────────────────────────────────────────────────┤ ├────────────────────────┤  │
│  │  PROCESSING INDICATOR (when agent running)          │ │                      │  │
│  ├─────────────────────────────────────────────────────┤ │                      │  │
│  │  ┌───────────────────────────────────────────────┐  │ │                      │  │
│  │  │ INPUT PANEL                                      │  │ │                      │  │
│  │  │ ▌ Tell me more about this project...            │  │ │                      │  │
│  │  │                                                  │  │ │                      │  │
│  │  │ Agent  Provider Model                     1s    │  │ │                      │  │
│  │  └───────────────────────────────────────────────┘  │ │                      │  │
│  └─────────────────────────────────────────────────────┴────────────────────────┘  │
│                                                                                      │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

### Geometry Constants (from `UiLayout`)

| Constant | Value | Description |
|----------|-------|-------------|
| `sidebar_width` | 38 | Right sidebar width in columns |
| `left_column_right_margin` | 2 | Gap between main and sidebar |
| `main_outer_padding_x` | 1 | Horizontal padding from terminal edge |
| `main_outer_padding_y` | 1 | Vertical padding from terminal edge |
| `main_content_left_offset` | 2 | Left indent for content |
| `user_bubble_inner_padding` | 1 | Padding inside user message bubbles |
| `message_indent_width` | 4 | Indent for message content (2 + 2) |
| `command_palette_left_padding` | 2 | Left padding for command palette |
| `MAX_INPUT_LINES` | 5 | Maximum visible input lines |

### Color Palette (from `colors.rs`)

| Color Name | RGB Value | Usage |
|------------|-----------|-------|
| `PAGE_BG` | (246, 247, 251) | Main background |
| `SIDEBAR_BG` | (234, 238, 246) | Sidebar background |
| `INPUT_PANEL_BG` | (229, 233, 241) | Input panel background |
| `COMMAND_PALETTE_BG` | (214, 220, 232) | Command palette background |
| `TEXT_PRIMARY` | (37, 45, 58) | Primary text color |
| `TEXT_SECONDARY` | (98, 108, 124) | Secondary text color |
| `TEXT_MUTED` | (125, 133, 147) | Muted/hint text |
| `ACCENT` | (55, 114, 255) | Accent color (borders, selections) |
| `INPUT_ACCENT` | (19, 164, 151) | Checkmarks, active elements |
| `SELECTION_BG` | (55, 114, 255) | Text selection highlight |
| `NOTICE_BG` | (224, 227, 233) | Popup notice background |
| `PROGRESS_HEAD` | (124, 72, 227) | Processing indicator |
| `THINKING_LABEL` | (227, 152, 67) | "Thinking:" label color |
| `QUESTION_BORDER` | (220, 96, 180) | Question mode border |
| `DIFF_ADD_FG/BG` | (25,110,61)/(226,244,235) | Added lines in diffs |
| `DIFF_REMOVE_FG/BG` | (152,45,45)/(252,235,235) | Removed lines in diffs |
| `DIFF_META_FG` | (106, 114, 128) | Diff headers (@@, +++, ---) |

---

## Component Specifications

### 1. Messages Area

**Location:** Left column, fills available vertical space minus processing indicator and input panel.

**Content Structure:**
```
    [blank line]
    ▌ User message text here...                        █
    ▌ continued on next line if needed                 █
    ▌                                                  █
    [blank line]

    Assistant response with markdown rendering...
    More content here...

    ✓ Tool: name  result summary
      └ Error detail if failed

    ✓ Edit path/to/file  +12 -3
      | 1 |   | old line | ┃ | 1 |   | new line |
      | 2 | - | removed   | ┃ | 2 | + | added    |

    Thinking: Extended reasoning content shown here...
```

**User Message Bubble:**
- Left border: `▌` colored with agent color (or `ACCENT`/`QUESTION_BORDER`)
- Background: `INPUT_PANEL_BG`
- Inner padding: 1 space on each side
- Queued tag: " queued " with `QUEUED_TAG_BG` background when message is pending

**Assistant Message:**
- Plain text with markdown styling (bold, code, links)
- Indented by `message_indent_width` spaces
- Auto-scrolls to show new content

**Tool Call Display:**
- Pending: `→ ToolName(args...)` with `TEXT_MUTED` color
- Completed (success): `✓ ToolName result summary` with `INPUT_ACCENT` checkmark
- Completed (error): `✗ ToolName` with red checkmark, error detail on next line indented
- Edit/Write tools: Side-by-side diff with line numbers when width allows, otherwise single column

**Thinking Block:**
- Label: "Thinking: " in `THINKING_LABEL` italic
- Content: Dimmed markdown text
- Trailing blank line

**Compaction Block:**
- Centered label: `--- Compaction ---` in `TEXT_MUTED`
- Optional summary below

**Footer (on turn complete):**
```
    ✓ AgentName  Provider Model  1m 23s
    ✗ AgentName  Provider Model  45s  interrupted
```

### 2. Processing Indicator

**Location:** Single row between messages and input, aligned with message indent.

**States:**

```
INACTIVE (hidden, 0 height):
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│   (no content - row is collapsed)                                                            │
└────────────────────────────────────────────────────────────────────────────────────────────────┘

ACTIVE (agent running):
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│     ⬝ ⬝ ■ ■ ■ ⬝ ⬝ ⬝ ⬝    1m 23s  esc to interrupt                                         │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
      ↑         ↑              ↑           ↑
      2 spaces  scanner bar    2 spaces    duration + interrupt hint
                (10 chars)
                moves back and
                forth
```

**Scanner Bar Animation:**
- Total width: 6-10 characters based on available space
- Head character: `■` (solid block) in `PROGRESS_HEAD` color
- Trail characters:
  - Distance 1: `■` at 30% blend
  - Distance 2: `■` at 40% blend
  - Distance 3+: `⬝` (dotted block) at 52% blend
- Animation: Head moves left-to-right, pauses, then right-to-left, pauses, repeats

### 3. Input Panel

**Location:** Bottom of main column, height based on content (1-5 lines + borders).

**Structure:**
```
    ┌─────────────────────────────────────────────────────────┐
    │ ▌ [cursor] Input text here...                           │
    │                                                         │
    │ AgentName  Provider Model                          1s   │
    └─────────────────────────────────────────────────────────┘
```

**Elements:**
- Left border: `▌` colored with selected agent's color
- Background: `INPUT_PANEL_BG`
- Placeholder: "Tell me more about this project..." when empty
- Status line at bottom: `AgentName  Provider Model  duration`
- Cursor positioned at current insertion point

**Question Mode (when tool asks user):**
```
    ┌─────────────────────────────────────────────────────────┐
    │ Question text here?                                     │
    │                                                         │
    │ 1. Option One                                           │
    │    Description of option one                            │
    │ 2. Option Two (selected)                                │
    │ 3. Type your own answer                                 │
    │                                                         │
    │ ↑↓ select  enter submit  esc dismiss                   │
    └─────────────────────────────────────────────────────────┘
```
- Active option highlighted with `ACCENT` bold
- Selected options shown with `[x]` for multiple-choice
- Custom input field when "Type your own answer" is active

### 4. Command Palette (Autocomplete)

**Location:** Floating above input panel, anchored to input panel geometry.

**Structure:**
```
    ┌─────────────────────────────────────────────────────────┐
    │ /new              Start a new session                   │
    │ /model            Switch or list models              ←  │ ← selected
    │ /resume           Resume a prior session               │
    └─────────────────────────────────────────────────────────┘
    ┌─────────────────────────────────────────────────────────┐
    │ ▌ /mod                                                │ │
    └─────────────────────────────────────────────────────────┘
```

**Behavior:**
- Shows when input starts with `/`
- Max 5 items visible
- Selected item has white text on `ACCENT` background
- Name left-aligned, description right-aligned
- Width matches input panel width
- Left edge aligned with input panel left edge (including indent)

### 5. Sidebar

**Location:** Right column, full height clipped to input panel bottom.

**Structure:**
```
    ┌────────────────────────────────
    │                                │
    │ Session Name                   │
    │                               │
    │ → Subagent Session Title       │
    │                               │
    │ ~/path/to/project @ branch    │
    │                               │
    │ ▼ Context                      │
    │   45 / 128 (35%)              │
    │                               │
    │ ▼ Modified Files               │
    │   src/main.rs      +12 -3     │
    │   src/lib.rs       +5  -1     │
    │                               │
    │ ▶ TODO (2/5)                   │ ← folded
    │                               │
    └────────────────────────────────┘
```

**Elements:**
- Session name in bold
- Active subagent sessions shown with `→` prefix
- Working directory with optional git branch
- Sections can be folded/unfolded (▶/▼) by clicking headers
- Context usage colored: yellow (>30%), orange (>40%), red (>60%)
- Modified files show +/- line counts with color
- TODO items show status: `[ ]` pending, `[✓]` done, `[-]` cancelled

**Scrolling:**
- Independent scroll from messages area
- Scroll wheel targets whichever area the cursor is over

### 6. Popup Notices

**Clipboard Copied Notice:**
```
    ┌────────────┐
    │   Copied   │
    └────────────┘
```
- Appears near mouse cursor position after text selection copy
- Auto-dismisses after 1500ms
- Background: `NOTICE_BG`

---

## Subagent Session View

When viewing a subagent session, the layout changes:

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│                                                                                      │
│  [SUBAGENT MESSAGES - same format as main messages]                                 │
│                                                                                      │
├──────────────────────────────────────────────────────────────────────────────────────┤
│  ⬝⬝⬝■■■⬝⬝⬝  1m 23s  esc back to main agent                                       │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

**Differences from main view:**
- No input panel
- Footer shows: `✓/✗ AgentName  Provider Model  duration  (interrupted)`
- "esc back to main agent" or "esc back to upper subagent" hint
- Sidebar still visible on right

---

## Event Handling Reference

### Keyboard Shortcuts

| Key | Context | Action |
|-----|---------|--------|
| `Ctrl+C` | Empty input | Quit |
| `Ctrl+C` | Has input | Clear input |
| `Ctrl+A/E` | Input | Move to line start/end |
| `Ctrl+J` | Input | Insert newline |
| `Shift+Enter` | Input | Insert newline |
| `Enter` | Input | Submit (or complete command) |
| `Esc` | Processing | Interrupt (after double-press) |
| `Esc` | Not processing | Clear input |
| `Tab` | Any | Cycle agent |
| `↑/↓` | Empty input | Scroll messages |
| `↑/↓` | Has input | Move cursor |
| `↑/↓` | Command palette | Navigate commands |
| `PageUp/Down` | Any | Scroll messages by page |
| `Ctrl+V/Cmd+V` | Any | Paste (text or image) |

### Mouse Interactions

| Action | Target | Result |
|--------|--------|--------|
| Click | Sidebar section header | Toggle fold |
| Click | Task tool result | Open subagent session |
| Click+Drag | Messages area | Start text selection |
| Release | Messages area | Copy selection to clipboard, show notice |
| Scroll | Over messages | Scroll messages |
| Scroll | Over sidebar | Scroll sidebar |

---

## Migration Phases

### Phase 0: Baseline + Parity Contract

**Files:**
- Reference: `src/app/render.rs`, `src/app/components/*.rs`, `src/theme/colors.rs`

- [ ] **Step 1: Capture baseline frames**
  Use tmux to capture screen frames for representative scenarios:
  ```bash
  # Start tmux session with chat mode
  tmux new-session -d -s hh-baseline "cargo run -- chat"
  
  # After interaction, capture pane contents
  tmux capture-pane -t hh-baseline -p > baseline/chat-initial.txt
  
  # Or use screenshot tools for visual comparison
  ```
  Note: `run` mode doesn't have TUI; use `chat` mode for TUI testing.

- [ ] **Step 2: Document visual regression checklist**
  Create `docs/plans/tui-parity-checklist.md` with:
  - Screenshot comparisons for each component
  - Color sampling points
  - Layout measurements

- [ ] **Step 3: Run performance baseline**
  ```bash
  cargo run --bench tui_perf_probe -- --history 600 --typing 200
  ```
  Expected: p95 < 16ms for current implementation

### Phase 1: Decouple UI Data From ratatui Types

**Files:**
- Create: `src/app/ui/mod.rs`, `src/app/ui/primitives.rs`, `src/app/ui/geometry.rs`
- Create: `src/app/ui/ratatui_adapter.rs`
- Modify: `src/app/render.rs`, `src/theme/markdown.rs`, `src/app/components/viewport_cache.rs`

- [x] **Step 1: Define UI primitives** ✅

Create `src/app/ui/mod.rs`:
```rust
pub mod geometry;
pub mod primitives;
pub mod ratatui_adapter;

pub use geometry::*;
pub use primitives::*;
```

Create `src/app/ui/primitives.rs`:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl UiColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Clone, Copy, Default)]
pub struct UiStyle {
    pub fg: Option<UiColor>,
    pub bg: Option<UiColor>,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
}

#[derive(Clone)]
pub struct UiSpan {
    pub content: String,
    pub style: UiStyle,
}

#[derive(Clone)]
pub struct UiLine {
    pub spans: Vec<UiSpan>,
```

- [x] **Step 2: Run tests** ✅
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [x] **Step 3: Create geometry primitives** ✅

Create `src/app/ui/geometry.rs`:
```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct UiRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl UiRect {
    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub fn inset(&self, px: u16, py: u16) -> Self {
        Self {
            x: self.x.saturating_add(px),
            y: self.y.saturating_add(py),
            width: self.width.saturating_sub(px.saturating_mul(2)),
            height: self.height.saturating_sub(py.saturating_mul(2)),
        }
    }
}
```

- [x] **Step 4: Run tests** ✅
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [x] **Step 5: Create ratatui adapter** ✅

Create `src/app/ui/ratatui_adapter.rs`:
```rust
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::{UiColor, UiLine, UiSpan, UiStyle};

impl From<UiColor> for Color {
    fn from(c: UiColor) -> Self {
        Color::Rgb(c.r, c.g, c.b)
    }
}

impl From<UiStyle> for Style {
    fn from(s: UiStyle) -> Self {
        let mut style = Style::default();
        if let Some(fg) = s.fg {
            style = style.fg(fg.into());
        }
        if let Some(bg) = s.bg {
            style = style.bg(bg.into());
        }
        if s.bold {
            style = style.bold();
        }
        if s.italic {
            style = style.italic();
        }
        if s.dim {
            style = style.dim();
        }
        style
    }
}

pub fn ui_line_to_ratatui(line: &UiLine) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|s| Span::styled(s.content.clone(), s.style.into()))
            .collect::<Vec<_>>(),
    )
}
```

- [x] **Step 6: Run tests** ✅
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [>] **Step 7-10: Deferred to Phase 4**
  
  Porting viewport cache and render functions to UI primitives is deferred.
  This allows the iocraft implementation to use UI primitives while keeping
  the ratatui implementation unchanged, avoiding a massive refactoring.
  
  **Rationale:**
  - UI primitives layer exists and is tested (Steps 1-6)
  - ratatui implementation continues to work unchanged
  - iocraft implementation will use UI primitives directly
  - Conversion at boundaries is cleaner than full refactoring

- [ ] **Step 3: Create geometry primitives**

Create `src/app/ui/geometry.rs`:
```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct UiRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl UiRect {
    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }

    pub fn inset(&self, px: u16, py: u16) -> Self {
        Self {
            x: self.x.saturating_add(px),
            y: self.y.saturating_add(py),
            width: self.width.saturating_sub(px.saturating_mul(2)),
            height: self.height.saturating_sub(py.saturating_mul(2)),
        }
    }
}
```

- [ ] **Step 4: Run tests**
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [x] **Step 5: Create ratatui adapter** ✅

  
  Create `src/app/ui/ratatui_adapter.rs`:
  ```rust
  use ratatui::prelude::Stylize;
  use ratatui::style::{Color, Style};
  use ratatui::text::{Line, Span};

  use super::{UiColor, UiLine, UiSpan, UiStyle};

  impl From<UiColor> for Color {
        fn from(c: UiColor) -> Self {
            Color::Rgb(c.r, c.g, c.b)
        }
    }

    impl From<UiStyle> for Style {
        fn from(s: UiStyle) -> Self {
            let mut style = Style::default();
            if let Some(fg) = s.fg {
                style = style.fg(fg.into());
            }
            if let Some(bg) = s.bg {
                style = style.bg(bg.into());
            }
            if s.bold {
                style = style.bold();
            }
            if s.italic {
                style = style.italic();
            }
            if s.dim {
                style = style.dim();
            }
            style
        }
    }

    pub fn ui_line_to_ratatui(line: &UiLine) -> Line<'static> {
        Line::from(
            line.spans
                .iter()
                .map(|s| Span::styled(s.content.clone(), s.style.into()))
                .collect::<Vec<_>>(),
        )
    }
    ```

- [x] **Step 6: Run tests** ✅
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [ ] **Step 7: Port viewport cache to UI primitives**

Modify `src/app/components/viewport_cache.rs`:
```rust
use crate::app::ui::UiLine;

pub struct MessageViewportCache {
    cached_lines: Vec<UiLine>,
    // ... rest unchanged, just type swap
}
```

- [ ] **Step 8: Run tests**
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [ ] **Step 9: Port render functions to UI primitives**

Update `src/app/render.rs` and `src/theme/markdown.rs` to produce `UiLine` instead of `ratatui::text::Line`.

- [ ] **Step 10: Run tests**
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [x] **Step 7: Port viewport cache to UI primitives**
modify `src/app/components/viewport_cache.rs`:
```rust
use crate::app::ui::UiLine;

pub struct MessageViewportCache {
    cached_lines: Vec<UiLine>,
    // ... rest unchanged, just type swap
}
````

- [x] **Step 8: Run tests**
  ```bash
  cargo test
  ```
          Expected: All tests pass

- [ ] **Step 9: Port render functions to UI primitives**

Update `src/app/render.rs` and `src/theme/markdown.rs` to produce `UiLine` instead of `ratatui::text::Line`.

- [ ] **Step 10: Run tests**
  ```bash
  cargo test
  ```
          Expected: All tests pass

### Phase 2: Runtime Boundary

**Files:**
- Create: `src/app/runtime/mod.rs`, `src/app/runtime/ratatui_backend.rs`, `src/app/runtime/iocraft_backend.rs`
- Modify: `src/app/mod.rs`, `Cargo.toml`

- [x] **Step 1: Define runtime trait** ✅

Create `src/app/runtime/mod.rs`:
```rust
pub mod iocraft_backend;
pub mod ratatui_backend;

use crate::app::ui::UiRect;

pub trait FrameContext {
    fn area(&self) -> UiRect;
}
```

- [x] **Step 2: Add iocraft dependency** ✅

Modify `Cargo.toml`:
```toml
iocraft = "0.7"
smol = "2.0"
```

- [x] **Step 3: Run cargo check** ✅
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 4: Implement ratatui backend** ✅

Create `src/app/runtime/ratatui_backend.rs` with `RatatuiFrameContext` wrapping ratatui's `Frame`.

- [x] **Step 5: Run tests** ✅
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [x] **Step 6: Implement iocraft backend skeleton** ✅

Create `src/app/runtime/iocraft_backend.rs` with stub implementation.

- [x] **Step 7: Run cargo check** ✅
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 8: Add runtime selection flag** ✅

Modify `src/app/mod.rs`:
```rust
pub async fn run_interactive_chat(settings: Settings, cwd: &Path) -> anyhow::Result<()> {
    let use_iocraft = std::env::var("HH_USE_IOCRAFT").is_ok();
    if use_iocraft {
        run_interactive_chat_iocraft(settings, cwd).await
    } else {
        run_interactive_chat_ratatui(settings, cwd).await
    }
}
```

- [x] **Step 9: Run tests** ✅
  ```bash
  cargo test
  ```
  Expected: All tests pass (default ratatui path unchanged)

- [ ] **Step 10: Run visual regression**
  Use tmux to capture and compare:
  ```bash
  tmux capture-pane -t hh-baseline -p > phase2/chat-state.txt
  # Compare with baseline manually
  ```
  Expected: Identical (ratatui path)

- [ ] **Step 11: Commit**
  ```bash
  git add src/app/runtime/ src/app/terminal.rs src/app/events.rs src/app/mod.rs Cargo.toml
  git commit -m "feat: add terminal backend abstraction layer

  - Define TerminalBackend and FrameContext traits
  - Implement ratatui backend (existing behavior)
  - Add iocraft backend skeleton
  - Add HH_USE_IOCRAFT env flag for runtime selection"
  ```

### Phase 3: Build iocraft Root Shell

**Files:**
- Create: `src/app/iocraft/mod.rs`, `src/app/iocraft/root.rs`, `src/app/iocraft/layout.rs`, `src/app/iocraft/theme.rs`

- [x] **Step 1: Create iocraft module structure**

Create `src/app/iocraft/mod.rs`:
```rust
pub mod layout;
pub mod root;
pub mod theme;

pub use root::run_iocraft_app;
```

- [x] **Step 2: Implement theme adapter**

Create `src/app/iocraft/theme.rs`:
```rust
use iocraft::prelude::*;

use crate::theme::colors::*;

pub fn to_iocraft_color(color: ratatui::style::Color) -> Color {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
        _ => Color::Default,
    }
}

pub const fn page_bg() -> Color {
    Color::Rgb { r: 246, g: 247, b: 251 }
}

pub const fn sidebar_bg() -> Color {
    Color::Rgb { r: 234, g: 238, b: 246 }
}

pub const fn input_panel_bg() -> Color {
    Color::Rgb { r: 229, g: 233, b: 241 }
}

// ... etc for all colors
```

- [x] **Step 3: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 4: Implement layout component**

Create `src/app/iocraft/layout.rs`:
```rust
use iocraft::prelude::*;

use crate::theme::colors::UiLayout;

#[component]
pub fn AppRoot(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let layout = UiLayout::default();

    element! {
        View(
            width: width as u32,
            height: height as u32,
            background_color: Color::Rgb { r: 246, g: 247, b: 251 },
            flex_direction: FlexDirection::Row,
        ) {
            // Main column
            View(
                flex: 1.0,
                flex_direction: FlexDirection::Column,
            ) {
                // Messages, processing, input
            }
            // Sidebar
            View(
                width: layout.sidebar_width as u32,
                background_color: Color::Rgb { r: 234, g: 238, b: 246 },
            ) {}
        }
    }
}
```

- [x] **Step 5: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 6: Wire into main loop**

Create `src/app/iocraft/root.rs`:
```rust
use iocraft::prelude::*;

pub async fn run_iocraft_app(
    settings: crate::config::Settings,
    cwd: std::path::PathBuf,
) -> anyhow::Result<()> {
    element!(super::layout::AppRoot)
        .fullscreen()
        .await?;
    Ok(())
}
```

- [x] **Step 7: Run with iocraft**
  ```bash
  HH_USE_IOCRAFT=1 hh chat
  ```
  Expected: Empty shell renders, can quit with 'q'

- [x] **Step 8: Commit**
  ```bash
  git add src/app/iocraft/
  git commit -m "feat: add iocraft root shell with layout structure

  - Add theme adapter for iocraft colors
  - Implement AppRoot component with main/sidebar columns
  - Wire into main loop via HH_USE_IOCRAFT flag"
  ```

### Phase 4: Component-by-Component Port

#### 4a: Messages Component

**Files:**
- Create: `src/app/iocraft/messages.rs`
- Modify: `src/app/iocraft/layout.rs`, `src/app/iocraft/mod.rs`

- [x] **Step 1: Create Messages component skeleton**

Create `src/app/iocraft/messages.rs`:
```rust
use iocraft::prelude::*;

#[component]
pub fn MessagesPanel(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            flex: 1.0,
            flex_direction: FlexDirection::Column,
            overflow_y: Overflow::Scroll,
        ) {
            // Will render message lines
        }
    }
}
```

- [x] **Step 2: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 3: Port user bubble rendering**

Implement user message bubble with correct styling:
- Border character `▌`
- Background color
- Inner padding

- [x] **Step 4: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 5: Port assistant message rendering**

Implement markdown text rendering with styling.

- [x] **Step 6: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 7: Port tool call rendering**

Implement pending/completed tool display with checkmarks.

- [x] **Step 8: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [x] **Step 9: Wire into layout**

Update `src/app/iocraft/layout.rs` to include MessagesPanel.

- [x] **Step 10: Test messages rendering**
  Use tmux with HH_USE_IOCRAFT=1:
  ```bash
  tmux new-session -d -s hh-iocraft "HH_USE_IOCRAFT=1 cargo run -- chat"
  # Interact, then capture
  tmux capture-pane -t hh-iocraft -p > phase4a/messages.txt
  ```
  Expected: Messages render correctly

- [x] **Step 11: Visual regression check**
  Compare captured output with baseline manually.

- [x] **Step 12: Commit**
  ```bash
  git add src/app/iocraft/messages.rs src/app/iocraft/layout.rs src/app/iocraft/mod.rs
  git commit -m "feat(iocraft): port messages component

  - User message bubbles with correct borders and padding
  - Assistant message markdown rendering
  - Tool call display with pending/completed states"
  ```

#### 4b: Input Panel

**Files:**
- Create: `src/app/iocraft/input.rs`
- Modify: `src/app/iocraft/layout.rs`, `src/app/iocraft/mod.rs`

- [x] **Step 1: Create InputPanel component**

- [x] **Step 2: Implement text input with cursor**

- [x] **Step 3: Implement status line**

- [x] **Step 4: Implement question mode UI**

- [x] **Step 5: Wire into layout**

- [x] **Step 6: Test input rendering**

- [x] **Step 7: Commit**

#### 4c: Sidebar

**Files:**
- Create: `src/app/iocraft/sidebar.rs`
- Modify: `src/app/iocraft/layout.rs`, `src/app/iocraft/mod.rs`

- [x] **Step 1: Create Sidebar component**

- [x] **Step 2: Implement session info section**

- [x] **Step 3: Implement context usage display**

- [x] **Step 4: Implement modified files list**

- [x] **Step 5: Implement TODO list**

- [x] **Step 6: Implement section folding**

- [x] **Step 7: Wire into layout**

- [x] **Step 8: Test sidebar rendering**

- [x] **Step 9: Commit**

#### 4d: Popups and Command Palette

**Files:**
- Create: `src/app/iocraft/popups.rs`
- Modify: `src/app/iocraft/layout.rs`, `src/app/iocraft/mod.rs`

- [x] **Step 1: Create CommandPalette component**

- [x] **Step 2: Implement clipboard notice popup**

- [x] **Step 3: Wire into layout with correct z-order**

- [x] **Step 4: Test popups rendering**

- [x] **Step 5: Commit**

### Phase 5: Input + Mouse Parity

**Files:**
- Modify: `src/app/iocraft/root.rs`, `src/app/iocraft/messages.rs`, `src/app/iocraft/sidebar.rs`

- [x] **Step 1: Map keyboard events to InputEvent enum**

- [x] **Step 2: Wire key handling through existing dispatch**

- [x] **Step 3: Implement mouse scroll for messages**

- [x] **Step 4: Implement mouse scroll for sidebar**

- [x] **Step 5: Implement mouse click for section toggle**

- [x] **Step 6: Implement text selection and copy**

- [x] **Step 7: Test all keyboard shortcuts**

- [x] **Step 8: Test all mouse interactions**

- [x] **Step 9: Commit**

### Phase 6: Testing + Visual Regression

**Files:**
- Create: `src/app/iocraft/tests.rs` or tests in appropriate locations

- [x] **Step 1: Add mock terminal tests for input editing**

- [x] **Step 2: Add mock terminal tests for command palette**

- [x] **Step 3: Add mock terminal tests for question mode**

- [x] **Step 4: Run full test suite**
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [x] **Step 5: Run lint checks**
  ```bash
  cargo fmt --check
  cargo clippy -- -D warnings
  ```
  Expected: No errors

- [x] **Step 6: Run visual regression suite**
  Use tmux to capture and compare iocraft vs baseline:
  ```bash
  # Capture iocraft session
  tmux new-session -d -s hh-iocraft "HH_USE_IOCRAFT=1 cargo run -- chat"
  # ... interact ...
  tmux capture-pane -t hh-iocraft -p > phase6-iocraft/chat.txt
  tmux kill-session -t hh-iocraft
  
  # Compare with baseline manually
  ```
  Expected: Visually equivalent

- [ ] **Step 7: Run performance benchmark**
  ```bash
  cargo run --bench tui_perf_probe -- --history 600 --typing 200
  ```
  Expected: p95 < 16ms

- [ ] **Step 8: Commit**

### Phase 7: Cutover + Cleanup

**Files:**
- Modify: `src/app/mod.rs`, `Cargo.toml`, `README.md`
- Delete: `src/app/runtime/ratatui_backend.rs`, `src/app/ui/ratatui_adapter.rs`, old component files

- [ ] **Step 1: Flip default to iocraft**

Modify `src/app/mod.rs`:
```rust
pub async fn run_interactive_chat(settings: Settings, cwd: &Path) -> anyhow::Result<()> {
    // iocraft is now the default
    run_interactive_chat_iocraft(settings, cwd).await
}
```

- [ ] **Step 2: Run full test suite**
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [ ] **Step 3: Remove ratatui dependency**

Modify `Cargo.toml`:
```toml
# Remove: ratatui = "0.29"
# Remove: syntect-tui = "3.0"
```

- [ ] **Step 4: Run cargo check**
  ```bash
  cargo check
  ```
  Expected: No errors

- [ ] **Step 5: Remove old ratatui code**

Delete:
- `src/app/runtime/ratatui_backend.rs`
- `src/app/ui/ratatui_adapter.rs`
- Old ratatui-specific code in components

- [ ] **Step 6: Run full test suite**
  ```bash
  cargo test
  ```
  Expected: All tests pass

- [ ] **Step 7: Update documentation**

Modify `README.md`:
```markdown
- **Terminal TUI**: Interactive chat experience built with `iocraft`.
```

- [ ] **Step 8: Run lint checks**
  ```bash
  cargo fmt --check
  cargo clippy -- -D warnings
  ```
  Expected: No errors

- [ ] **Step 9: Final visual verification**
  Use tmux for final verification:
  ```bash
  tmux new-session -d -s hh-final "cargo run -- chat"
  # ... interact ...
  tmux capture-pane -t hh-final -p > final/chat.txt
  tmux kill-session -t hh-final
  ```
  Expected: Fully functional, visually correct

- [ ] **Step 10: Commit**
  ```bash
  git add -A
  git commit -m "feat: complete migration to iocraft TUI framework

  BREAKING CHANGE: removes ratatui dependency

  - Set iocraft as default TUI backend
  - Remove ratatui adapter and backend
  - Update documentation to reflect iocraft usage
  - All visual and behavioral parity verified"
  ```

---

## Rollback Plan

If issues are discovered after cutover:

1. **Immediate rollback**: Revert Phase 7 commits
   ```bash
   git revert HEAD~N  # N = number of Phase 7 commits
   ```

2. **Feature flag fallback**: Add `HH_USE_RATATUI=1` env var to force old backend while investigating

3. **Hotfix branch**: Create branch from last known-good ratatui commit for critical fixes

---

## Success Metrics

- [ ] All existing TUI features work identically under iocraft
- [ ] `cargo test` passes with 0 failures
- [ ] `cargo clippy -- -D warnings` passes with 0 warnings
- [ ] Visual regression tests show no unexpected differences
- [ ] Performance benchmark shows p95 < 16ms
- [ ] Manual testing of all keyboard shortcuts passes
- [ ] Manual testing of all mouse interactions passes
- [ ] Debug frame capture/replay works correctly
