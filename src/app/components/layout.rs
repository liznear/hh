use crate::ui_compat::layout::{Constraint, Direction, Layout, Rect};
use crate::ui_compat::style::Style;
use crate::ui_compat::widgets::Block;

use crate::app::components::input;
use crate::app::state::AppState;
use crate::theme::colors::{AppLayoutRects, MAX_INPUT_LINES, SIDEBAR_BG, UiLayout};

pub(crate) struct RootColumns {
    pub main_area: Rect,
    pub sidebar_area: Option<Rect>,
}

pub(crate) struct MainLayout {
    pub messages_area: Rect,
    pub processing_area: Rect,
    pub input_area: Rect,
}

pub(crate) struct SubagentLayout {
    pub messages_area: Rect,
    pub back_indicator_area: Rect,
}

fn inset_rect(area: Rect, padding_x: u16, padding_y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(padding_x),
        y: area.y.saturating_add(padding_y),
        width: area.width.saturating_sub(padding_x.saturating_mul(2)),
        height: area.height.saturating_sub(padding_y.saturating_mul(2)),
    }
}

pub(crate) fn split_root_columns(area: Rect, layout: UiLayout) -> RootColumns {
    let app_area = inset_rect(
        area,
        layout.main_outer_padding_x,
        layout.main_outer_padding_y,
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(&[
            Constraint::Min(40),
            Constraint::Length(layout.left_column_right_margin),
            Constraint::Length(layout.sidebar_width),
        ])
        .split(app_area);

    RootColumns {
        main_area: columns[0],
        sidebar_area: if columns.len() > 2 {
            Some(columns[2])
        } else {
            None
        },
    }
}

pub(crate) fn build_subagent_layout(main_area: Rect) -> SubagentLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(&[
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(main_area);

    SubagentLayout {
        messages_area: chunks[0],
        back_indicator_area: chunks[2],
    }
}

pub(crate) fn build_main_layout(app: &AppState, input_text: &str, main_area: Rect) -> MainLayout {
    let layout = UiLayout::default();
    let input_content_width = main_area
        .width
        .saturating_sub(layout.user_bubble_indent() as u16 + 3)
        as usize;
    let input_line_count =
        input::input_line_count(input_text, input_content_width).clamp(1, MAX_INPUT_LINES);
    let input_area_height = if app.has_pending_question() {
        (input::question_prompt_line_count(app, input_content_width) + 2) as u16
    } else {
        (input_line_count + 4) as u16
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(&[
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(input_area_height),
        ])
        .split(main_area);

    MainLayout {
        messages_area: chunks[0],
        processing_area: chunks[2],
        input_area: chunks[4],
    }
}

pub(crate) fn compute_layout_rects(area: Rect, app: &AppState, input_text: &str) -> AppLayoutRects {
    let layout = UiLayout::default();
    let root = split_root_columns(area, layout);
    let main = build_main_layout(app, input_text, root.main_area);

    let sidebar_content = root.sidebar_area.and_then(|sidebar_area| {
        let sidebar_bottom = main.input_area.bottom();
        let clipped_sidebar_area = Rect {
            x: sidebar_area.x,
            y: sidebar_area.y,
            width: sidebar_area.width,
            height: sidebar_bottom.saturating_sub(sidebar_area.y),
        };
        if clipped_sidebar_area.width == 0 || clipped_sidebar_area.height == 0 {
            return None;
        }

        let block = Block::default().style(Style::default().bg(SIDEBAR_BG));
        let inner = block.inner(clipped_sidebar_area);
        let content = inset_rect(inner, 2, 0);
        if content.width == 0 || content.height == 0 {
            None
        } else {
            Some(content)
        }
    });

    let main_messages = if main.messages_area.height > 0 {
        Some(main.messages_area)
    } else {
        None
    };

    AppLayoutRects {
        main_messages,
        sidebar_content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_layout_rects_returns_main_and_sidebar_regions() {
        let app = AppState::new(std::path::Path::new(".").to_path_buf());

        let rects = compute_layout_rects(Rect::new(0, 0, 140, 40), &app, "hello");
        assert!(rects.main_messages.is_some());
        assert!(rects.sidebar_content.is_some());
    }

    #[test]
    fn compute_layout_rects_handles_small_terminal_without_panicking() {
        let app = AppState::new(std::path::Path::new(".").to_path_buf());

        let rects = compute_layout_rects(Rect::new(0, 0, 20, 6), &app, "x");
        assert!(rects.main_messages.is_some() || rects.sidebar_content.is_none());
    }
}
