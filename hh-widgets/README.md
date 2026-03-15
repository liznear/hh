# hh-widgets

Reusable terminal UI widget primitives for `ratatui` applications.

This crate provides extraction-friendly building blocks for markdown, popup geometry,
scrollable composition, and codediff rendering.

## Running examples

From the workspace root:

```bash
cargo run -p hh-widgets --example codediff_example
```

The example opens an interactive terminal preview in an alternate screen.
Press `q`, `Esc`, or `Enter` to exit.

From the `hh-widgets` crate directory:

```bash
cargo run --example codediff_example
```

## Examples

### Standalone markdown widget model

```rust
use hh_widgets::markdown::{MarkdownBlock, MarkdownOptions, markdown_to_lines_with_indent};

let block = MarkdownBlock::new("# Title\n\n- alpha\n- beta");
let _opts = MarkdownOptions::default();
let lines = markdown_to_lines_with_indent(&block.source, 80, "  ");
assert!(!lines.is_empty());
```

### Scrollable with mixed children

```rust
use hh_widgets::codediff::CodeDiff;
use hh_widgets::markdown::MarkdownBlock;
use hh_widgets::scrollable::{ScrollableState, measure_children, visible_range};
use hh_widgets::widget::WidgetNode;

let children = vec![
    WidgetNode::Markdown(MarkdownBlock::new("hello")),
    WidgetNode::Spacer(1),
    WidgetNode::CodeDiff(CodeDiff::from_unified_diff("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new")),
];
let layout = measure_children(&children, 80);
let mut state = ScrollableState::default();
state.viewport_height = 6;
let _visible = visible_range(&layout, &state);
```

### Popup geometry

```rust
use hh_widgets::popup::{Anchor, PopupOptions, PopupRequest, popup_from_request};
use hh_widgets::widget::Area;

let mut viewport = Area::default();
viewport.width = 80;
viewport.height = 24;

let mut opts = PopupOptions::default();
opts.anchor = Anchor::BottomLeft;

let mut req = PopupRequest::default();
req.anchor_x = 4;
req.anchor_y = 22;
req.width = 30;
req.height = 6;
req.options = opts;

let popup = popup_from_request(req, viewport);
assert!(popup.width <= viewport.width);
assert!(popup.height <= viewport.height);
```

### Codediff rendering

```rust
use hh_widgets::codediff::{CodeDiff, CodeDiffLayout};

let diff = CodeDiff::from_unified_diff("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new")
    .with_layout(CodeDiffLayout::SideBySide);
assert!(!diff.rendered_lines().is_empty());
```

## Stability and Versioning Policy

- Public config/state structs are `#[non_exhaustive]` when extension is expected.
- New capabilities are added additively (new fields/options/builders) in minor releases.
- Removing fields, changing semantic contracts, or non-backward-compatible API changes require a major release.
- Internal parsing/layout/cache details are not stable API unless explicitly documented.

## SemVer, MSRV, and Publish Strategy

- SemVer: this crate follows semantic versioning for public API behavior.
- MSRV: tracks the workspace Rust toolchain; explicit MSRV pinning is deferred until external publication.
- Publish strategy (current): workspace-only crate, not published to crates.io yet.
- Publish strategy (future): publish to crates.io after adapter-boundary parity and performance gates remain green across releases.
