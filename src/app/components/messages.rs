use crate::app::chat_state::{ScrollState, TextSelection};
use crate::app::components::viewport_cache::MessageViewportCache;
use crate::app::core::{AppAction, Component};
use crate::app::state::AppState;
use crate::app::ui::text::{Color, Line, Span, Style};
use crate::theme::colors::*;

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

pub(crate) fn apply_selection_highlight(lines: &mut [Line], app: &AppState, line_offset: usize) {
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

fn highlight_line_range(line: &mut Line, start: usize, end: usize) {
    let original_spans = std::mem::take(&mut line.spans);
    let mut highlighted = Vec::with_capacity(original_spans.len() + 2);
    let mut cursor = 0usize;

    for span in original_spans {
        let content = span.content.as_ref() as &str;
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

fn line_char_count(line: &Line) -> usize {
    line.spans
        .iter()
        .map(|span| (span.content.as_ref() as &str).chars().count())
        .sum()
}

fn char_slice(input: &str, start: usize, end: usize) -> String {
    input
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}
