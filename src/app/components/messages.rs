use ratatui::{
    Frame,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
};

use crate::app::chat_state::{ScrollState, TextSelection};
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

    let lines = messages.viewport.get_lines(app, wrap_width);
    let total_lines = lines.len();

    let scroll_offset = app
        .message_scroll
        .effective_offset(total_lines, visible_height);

    let rendered_lines =
        messages
            .viewport
            .get_visible_lines(app, wrap_width, visible_height, scroll_offset);
    let text = Text::from(rendered_lines.clone());
    let paragraph = Paragraph::new(text)
        .style(Style::default().bg(PAGE_BG).fg(TEXT_PRIMARY))
        .scroll((0, 0));

    f.render_widget(paragraph, content);
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
