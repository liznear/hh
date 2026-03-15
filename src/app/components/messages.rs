use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
    Frame,
};

use hh_widgets::scrollable::VisibleRange;
use hh_widgets::scrollable::{measure_children, visible_range, ScrollableState};
use hh_widgets::widget::WidgetNode;

use crate::app::chat_state::{ScrollState, TextSelection};
use crate::app::components::messages_blocks::{
    build_legacy_blocks_from_starts, measured_heights, visible_message_range,
};
use crate::app::components::messages_layout::compute_layout;
use crate::app::components::viewport_cache::MessageViewportCache;
use crate::app::state::AppState;

pub struct MessagesComponent {
    pub viewport: MessageViewportCache,
    pub scroll: ScrollState,
    pub selection: TextSelection,
}

impl Default for MessagesComponent {
    fn default() -> Self {
        Self {
            viewport: MessageViewportCache::new(),
            scroll: ScrollState::new(true),
            selection: TextSelection::None,
        }
    }
}

impl Component for MessagesComponent {
    fn update(&mut self, action: &AppAction) -> Option<AppAction> {
        match action {
            AppAction::ScrollMessages(amount) => {
                let amount = *amount;
                if amount < 0 {
                    self.scroll.offset = self
                        .scroll
                        .offset
                        .saturating_add(amount.unsigned_abs() as usize);
                } else {
                    self.scroll.offset = self.scroll.offset.saturating_sub(amount as usize);
                }
                self.scroll.auto_follow = self.scroll.offset == 0;
                Some(AppAction::Redraw)
            }
            _ => None,
        }
    }
}

impl MessagesComponent {
    pub(crate) fn render_messages(
        &mut self,
        f: &mut Frame,
        app: &AppState,
        area: ratatui::layout::Rect,
    ) {
        render_messages_local(f, app, self, area);
    }
}

use crate::app::core::{AppAction, Component};
use crate::theme::colors::*;

fn render_messages_local(
    f: &mut Frame,
    app: &AppState,
    messages: &mut MessagesComponent,
    area: ratatui::layout::Rect,
) {
    let panel = Block::default().style(Style::default().bg(PAGE_BG));
    let inner = panel.inner(area);
    f.render_widget(panel, area);

    let content = inner;

    let wrap_width = content.width as usize;
    let visible_height = content.height as usize;

    let lines = messages.viewport.get_lines(app, wrap_width).clone();
    let starts = messages
        .viewport
        .get_message_starts(app, wrap_width)
        .clone();
    let total_lines = lines.len();

    let scroll_offset = app
        .message_scroll
        .effective_offset(total_lines, visible_height);

    let scroll_slice = compute_scroll_slice(
        app,
        &starts,
        total_lines,
        scroll_offset,
        visible_height,
        content.width,
        app.message_scroll.auto_follow,
    );

    let (rendered_lines, line_offset) = build_visible_lines_from_message_range(
        &lines,
        &starts,
        total_lines,
        scroll_slice.visible_height,
        scroll_slice.line_offset,
        scroll_slice.message_range,
    );

    let mut rendered_lines = rendered_lines;
    apply_selection_highlight(&mut rendered_lines, app, line_offset);

    let text = Text::from(rendered_lines);
    let paragraph = Paragraph::new(text)
        .style(Style::default().bg(PAGE_BG).fg(TEXT_PRIMARY))
        .scroll((0, 0));

    f.render_widget(paragraph, content);
}

fn build_visible_lines_from_message_range(
    lines: &[Line<'static>],
    starts: &[usize],
    total_lines: usize,
    visible_height: usize,
    scroll_offset: usize,
    visible_messages: VisibleRange,
) -> (Vec<Line<'static>>, usize) {
    let default_end = scroll_offset
        .saturating_add(visible_height)
        .min(total_lines);
    if visible_messages.start >= visible_messages.end || starts.is_empty() {
        return (lines[scroll_offset..default_end].to_vec(), scroll_offset);
    }

    let mut rendered = Vec::with_capacity(visible_height);
    let mut remaining = visible_height;
    let mut cursor = scroll_offset;
    let mut first_line_index = None;

    for msg_index in visible_messages.start..visible_messages.end {
        if remaining == 0 {
            break;
        }

        let msg_start = starts[msg_index];
        let msg_end = starts.get(msg_index + 1).copied().unwrap_or(total_lines);
        if msg_start >= msg_end || cursor >= msg_end {
            continue;
        }

        let take_start = cursor.max(msg_start);
        let available = msg_end.saturating_sub(take_start);
        let take_count = available.min(remaining);
        if take_count == 0 {
            continue;
        }

        if first_line_index.is_none() {
            first_line_index = Some(take_start);
        }

        rendered.extend_from_slice(&lines[take_start..take_start + take_count]);
        cursor = take_start.saturating_add(take_count);
        remaining = remaining.saturating_sub(take_count);
    }

    if rendered.is_empty() {
        (lines[scroll_offset..default_end].to_vec(), scroll_offset)
    } else {
        (rendered, first_line_index.unwrap_or(scroll_offset))
    }
}

pub(crate) fn apply_selection_highlight(
    lines: &mut [Line<'static>],
    app: &AppState,
    line_offset: usize,
) {
    let Some((start, end)) = app.text_selection.get_range() else {
        return;
    };

    for (visible_idx, line) in lines.iter_mut().enumerate() {
        let line_idx = line_offset.saturating_add(visible_idx);
        if line_idx < start.line || line_idx > end.line {
            continue;
        }

        let line_len = line_char_count(line);
        let start_col = if line_idx == start.line {
            start.column
        } else {
            0
        };
        let end_col = if line_idx == end.line {
            end.column
        } else {
            line_len
        };

        let clamped_start = start_col.min(line_len);
        let clamped_end = end_col.min(line_len);
        if clamped_start >= clamped_end {
            continue;
        }

        highlight_line_range(line, clamped_start, clamped_end);
    }
}

fn highlight_line_range(line: &mut Line<'static>, start: usize, end: usize) {
    let original_spans = std::mem::take(&mut line.spans);
    let mut highlighted = Vec::with_capacity(original_spans.len() + 2);
    let mut cursor = 0usize;

    for span in original_spans {
        let content = span.content.as_ref();
        let span_len = content.chars().count();
        let span_start = cursor;
        let span_end = span_start + span_len;

        if span_len == 0 || end <= span_start || start >= span_end {
            highlighted.push(span);
            cursor = span_end;
            continue;
        }

        let local_start = start.saturating_sub(span_start).min(span_len);
        let local_end = end.saturating_sub(span_start).min(span_len);

        if local_start > 0 {
            highlighted.push(Span::styled(
                char_slice(content, 0, local_start),
                span.style,
            ));
        }

        if local_start < local_end {
            let selected_style = span
                .style
                .patch(Style::default().bg(SELECTION_BG).fg(Color::White));
            highlighted.push(Span::styled(
                char_slice(content, local_start, local_end),
                selected_style,
            ));
        }

        if local_end < span_len {
            highlighted.push(Span::styled(
                char_slice(content, local_end, span_len),
                span.style,
            ));
        }

        cursor = span_end;
    }

    line.spans = highlighted;
}

fn line_char_count(line: &Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|span| span.content.as_ref().chars().count())
        .sum()
}

fn char_slice(input: &str, start: usize, end: usize) -> String {
    input
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

#[derive(Debug, Clone)]
struct ScrollSlice {
    pub message_range: VisibleRange,
    pub line_offset: usize,
    pub visible_height: usize,
}

fn measure_message_height_nodes(starts: &[usize], total_lines: usize) -> Vec<WidgetNode> {
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(total_lines);
            let height = end.saturating_sub(*start);
            WidgetNode::Spacer(height.min(u16::MAX as usize) as u16)
        })
        .collect()
}

fn compute_scroll_slice(
    app: &AppState,
    starts: &[usize],
    total_lines: usize,
    scroll_offset: usize,
    visible_height: usize,
    width: u16,
    auto_follow: bool,
) -> ScrollSlice {
    if app.ui_renderer_mode == crate::config::UiRendererMode::WidgetBlocks {
        return compute_scroll_slice_widget(
            starts,
            total_lines,
            scroll_offset,
            visible_height,
            width,
        );
    }

    let children = measure_message_height_nodes(starts, total_lines);
    let layout = measure_children(&children, width.max(1));

    let mut state = ScrollableState::default();
    state.offset = scroll_offset.min(u16::MAX as usize) as u16;
    state.viewport_height = visible_height.min(u16::MAX as usize) as u16;
    state.auto_follow = auto_follow;

    let range = visible_range(&layout, &state);
    ScrollSlice {
        message_range: range,
        line_offset: scroll_offset,
        visible_height,
    }
}

fn compute_scroll_slice_widget(
    starts: &[usize],
    total_lines: usize,
    scroll_offset: usize,
    visible_height: usize,
    width: u16,
) -> ScrollSlice {
    let blocks = build_legacy_blocks_from_starts(starts, total_lines);
    let heights = measured_heights(&blocks, width.max(1));
    let layout = compute_layout(&heights, scroll_offset, visible_height);

    ScrollSlice {
        message_range: {
            let range = visible_message_range(&layout.rows, layout.visible_range);
            VisibleRange {
                start: range.start,
                end: range.end,
            }
        },
        line_offset: scroll_offset,
        visible_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_slice_widget_matches_legacy_visible_range_for_same_input() {
        let starts = vec![0usize, 2, 5, 6, 9];
        let total_lines = 12usize;
        let scroll_offset = 3usize;
        let visible_height = 4usize;
        let width = 80u16;

        let mut app = AppState::new(std::path::PathBuf::from("."));

        app.ui_renderer_mode = crate::config::UiRendererMode::LegacyLines;
        let legacy = compute_scroll_slice(
            &app,
            &starts,
            total_lines,
            scroll_offset,
            visible_height,
            width,
            false,
        );

        app.ui_renderer_mode = crate::config::UiRendererMode::WidgetBlocks;
        let widget = compute_scroll_slice(
            &app,
            &starts,
            total_lines,
            scroll_offset,
            visible_height,
            width,
            false,
        );

        assert_eq!(legacy.message_range.start, widget.message_range.start);
        assert_eq!(legacy.message_range.end, widget.message_range.end);
        assert_eq!(legacy.line_offset, widget.line_offset);
        assert_eq!(legacy.visible_height, widget.visible_height);
    }
}
