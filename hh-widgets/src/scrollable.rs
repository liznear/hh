use crate::widget::WidgetNode;

/// Caller-owned scroll state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ScrollableState {
    pub offset: u16,
    pub viewport_height: u16,
    pub auto_follow: bool,
}

/// Describes whether a state helper changed state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateChange {
    Unchanged,
    Changed,
}

/// Measured child metadata for viewport slicing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLayout {
    pub index: usize,
    pub start_y: u16,
    pub height: u16,
}

/// Scrollable measurement output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollLayout {
    pub children: Vec<ChildLayout>,
    pub total_height: u16,
}

/// Inclusive start, exclusive end visible range in child list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRange {
    pub start: usize,
    pub end: usize,
}

impl ScrollableState {
    pub fn scroll_by(&mut self, delta: i16, max_offset: u16) -> StateChange {
        let next = if delta.is_negative() {
            self.offset.saturating_sub(delta.unsigned_abs())
        } else {
            self.offset.saturating_add(delta as u16)
        }
        .min(max_offset);
        let changed = self.set_offset(next);
        if changed == StateChange::Changed {
            self.auto_follow = self.offset == max_offset;
        }
        changed
    }

    pub fn scroll_to_end(&mut self, max_offset: u16) -> StateChange {
        let changed = self.set_offset(max_offset);
        self.auto_follow = true;
        changed
    }

    pub fn clamp_offset(&mut self, max_offset: u16) -> StateChange {
        let changed = self.set_offset(self.offset.min(max_offset));
        self.auto_follow = self.offset == max_offset;
        changed
    }

    fn set_offset(&mut self, next: u16) -> StateChange {
        if self.offset == next {
            StateChange::Unchanged
        } else {
            self.offset = next;
            StateChange::Changed
        }
    }
}

pub fn measure_children(children: &[WidgetNode], width: u16) -> ScrollLayout {
    let mut measured = Vec::with_capacity(children.len());
    let mut cursor = 0u16;

    for (index, child) in children.iter().enumerate() {
        let height = child.measured_height(width.max(1));
        measured.push(ChildLayout {
            index,
            start_y: cursor,
            height,
        });
        cursor = cursor.saturating_add(height);
    }

    ScrollLayout {
        children: measured,
        total_height: cursor,
    }
}

pub fn max_offset_for(total_height: u16, viewport_height: u16) -> u16 {
    total_height.saturating_sub(viewport_height)
}

pub fn visible_range(layout: &ScrollLayout, state: &ScrollableState) -> VisibleRange {
    if layout.children.is_empty() {
        return VisibleRange { start: 0, end: 0 };
    }

    let viewport_start = state.offset;
    let viewport_end = state.offset.saturating_add(state.viewport_height.max(1));

    let mut start = layout.children.len();
    let mut end = layout.children.len();

    for (slot, child) in layout.children.iter().enumerate() {
        let child_start = child.start_y;
        let child_end = child.start_y.saturating_add(child.height.max(1));
        let intersects = child_end > viewport_start && child_start < viewport_end;
        if intersects {
            if start == layout.children.len() {
                start = slot;
            }
            end = slot + 1;
        }
    }

    if start == layout.children.len() {
        VisibleRange {
            start: layout.children.len(),
            end: layout.children.len(),
        }
    } else {
        VisibleRange { start, end }
    }
}

pub fn visible_children<'a>(
    children: &'a [WidgetNode],
    layout: &ScrollLayout,
    state: &ScrollableState,
) -> &'a [WidgetNode] {
    let range = visible_range(layout, state);
    if range.start >= range.end {
        &children[0..0]
    } else {
        &children[range.start..range.end]
    }
}

#[cfg(test)]
mod tests {
    use crate::{codediff::CodeDiffBlock, markdown::MarkdownBlock, widget::WidgetNode};

    use super::{
        ScrollableState, StateChange, max_offset_for, measure_children, visible_children,
        visible_range,
    };

    #[test]
    fn scroll_helpers_update_state_and_autofollow() {
        let mut state = ScrollableState {
            offset: 0,
            viewport_height: 5,
            auto_follow: true,
        };

        assert_eq!(state.scroll_by(3, 20), StateChange::Changed);
        assert_eq!(state.offset, 3);
        assert!(!state.auto_follow);

        assert_eq!(state.scroll_to_end(20), StateChange::Changed);
        assert_eq!(state.offset, 20);
        assert!(state.auto_follow);

        assert_eq!(state.clamp_offset(7), StateChange::Changed);
        assert_eq!(state.offset, 7);
        assert!(state.auto_follow);
    }

    #[test]
    fn measure_children_builds_cumulative_index() {
        let children = vec![
            WidgetNode::Spacer(2),
            WidgetNode::Markdown(MarkdownBlock::new("a\nb")),
            WidgetNode::CodeDiff(CodeDiffBlock::new("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new")),
        ];

        let layout = measure_children(&children, 40);
        assert_eq!(layout.children.len(), 3);
        assert_eq!(layout.children[0].start_y, 0);
        assert_eq!(layout.children[1].start_y, layout.children[0].height);
        assert_eq!(
            layout.children[2].start_y,
            layout.children[0]
                .height
                .saturating_add(layout.children[1].height)
        );
        assert_eq!(
            layout.total_height,
            layout.children.iter().map(|c| c.height).sum::<u16>()
        );
    }

    #[test]
    fn visible_range_slices_children_for_viewport() {
        let children = vec![
            WidgetNode::Spacer(2),
            WidgetNode::Spacer(3),
            WidgetNode::Spacer(4),
            WidgetNode::Spacer(5),
        ];
        let layout = measure_children(&children, 10);
        let mut state = ScrollableState {
            offset: 0,
            viewport_height: 4,
            auto_follow: false,
        };

        let r0 = visible_range(&layout, &state);
        assert_eq!((r0.start, r0.end), (0, 2));

        state.offset = 4;
        let r1 = visible_range(&layout, &state);
        assert_eq!((r1.start, r1.end), (1, 3));

        let slice = visible_children(&children, &layout, &state);
        assert_eq!(slice.len(), 2);
    }

    #[test]
    fn max_offset_respects_viewport_height() {
        assert_eq!(max_offset_for(20, 5), 15);
        assert_eq!(max_offset_for(5, 20), 0);
    }
}
