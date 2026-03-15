//! Reusable terminal UI widgets for ratatui applications.
//!
//! This crate exposes a small, additive-first public surface intended for
//! composition in host applications.
//!
//! # Example
//!
//! ```rust
//! use hh_widgets::markdown::{MarkdownBlock, MarkdownOptions};
//! use hh_widgets::scrollable::ScrollableState;
//! use hh_widgets::widget::WidgetNode;
//!
//! let _opts = MarkdownOptions::default();
//! let mut state = ScrollableState::default();
//! let node = WidgetNode::Markdown(MarkdownBlock::new("# hello"));
//!
//! let _ = state.scroll_by(1, 100);
//! let _nodes = [node];
//! ```
//!
//! # Composition Example
//!
//! ```rust
//! use hh_widgets::codediff::{CodeDiffBlock, CodeDiffOptions, render_unified_diff};
//! use hh_widgets::markdown::{MarkdownBlock, markdown_to_lines_with_indent};
//! use hh_widgets::scrollable::{ScrollableState, measure_children, visible_range};
//! use hh_widgets::widget::WidgetNode;
//!
//! let children = vec![
//!     WidgetNode::Markdown(MarkdownBlock::new("# Summary")),
//!     WidgetNode::Spacer(1),
//!     WidgetNode::CodeDiff(CodeDiffBlock::new("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new")),
//! ];
//!
//! let layout = measure_children(&children, 80);
//! let mut state = ScrollableState::default();
//! state.viewport_height = 8;
//! let _range = visible_range(&layout, &state);
//!
//! let _markdown = markdown_to_lines_with_indent("hello", 40, "");
//! let _diff = render_unified_diff(
//!     &CodeDiffBlock::new("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new"),
//!     &CodeDiffOptions::default(),
//! );
//! ```

pub mod codediff;
pub mod markdown;
pub mod popup;
pub mod scrollable;
pub mod theme;
pub mod widget;
