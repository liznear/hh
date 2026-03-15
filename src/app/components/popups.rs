use ratatui::{
    Frame,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Padding, Paragraph, Wrap},
};

use crate::theme::colors::*;

use crate::app::chat_state::ClipboardNotice;
use crate::app::core::{AppAction, Component};
use hh_widgets::popup::{Anchor, Offset, PopupOptions, PopupRequest, popup_from_request};
use hh_widgets::widget::Area;

#[derive(Default)]
pub struct PopupComponent {
    clipboard_notice: Option<ClipboardNotice>,
}

impl Component for PopupComponent {
    fn update(&mut self, action: &AppAction) -> Option<AppAction> {
        match action {
            AppAction::ShowClipboardNotice { x, y } => {
                self.clipboard_notice = Some(ClipboardNotice {
                    x: *x,
                    y: *y,
                    expires_at: std::time::Instant::now() + std::time::Duration::from_millis(1500),
                });
                Some(AppAction::Redraw)
            }
            AppAction::PeriodicTick => {
                if let Some(notice) = &self.clipboard_notice
                    && std::time::Instant::now() > notice.expires_at
                {
                    self.clipboard_notice = None;
                    return Some(AppAction::Redraw);
                }
                None
            }
            _ => None,
        }
    }

    fn render(
        &self,
        f: &mut ratatui::Frame<'_>,
        _area: ratatui::layout::Rect,
        _state: &crate::app::state::SessionContext,
    ) {
        render_clipboard_notice_local(f, &self.clipboard_notice);
    }
}

impl PopupComponent {
    pub(crate) fn render_command_palette_above_input(
        &self,
        f: &mut Frame,
        input_comp: &crate::app::components::input::InputComponent,
        input_area: ratatui::layout::Rect,
        layout: UiLayout,
    ) {
        if input_comp.filtered_commands.is_empty() {
            return;
        }

        let item_count = input_comp.filtered_commands.len().min(5) as u16;
        let popup_height = item_count;
        let input_left = input_area
            .x
            .saturating_add(layout.user_bubble_indent() as u16);
        let input_width = input_area
            .width
            .saturating_sub(layout.user_bubble_indent() as u16);

        let popup_area = command_palette_popup_area(
            input_left,
            input_area.y,
            input_width,
            popup_height,
            f.area(),
        );

        self.render_command_palette(f, input_comp, popup_area, layout);
    }

    pub(crate) fn render_command_palette(
        &self,
        f: &mut Frame,
        input_comp: &crate::app::components::input::InputComponent,
        area: ratatui::layout::Rect,
        layout: UiLayout,
    ) {
        render_command_palette_local(f, input_comp, area, layout);
    }
}

pub(crate) fn render_clipboard_notice_local(
    f: &mut ratatui::Frame,
    notice: &Option<ClipboardNotice>,
) {
    let Some(notice) = notice else {
        return;
    };

    let label = "Copied";
    let width = (label.len() as u16).saturating_add(4);
    let height = 3u16;
    let area = f.area();

    if area.width < width || area.height < height {
        return;
    }

    let popup = clipboard_popup_area(notice.x, notice.y, width, height, area);

    f.render_widget(Clear, popup);
    let block = Block::default()
        .style(Style::default().bg(NOTICE_BG).fg(TEXT_MUTED))
        .padding(Padding::new(2, 2, 1, 1));
    let content = block.inner(popup);
    f.render_widget(block, popup);
    f.render_widget(
        Paragraph::new(label)
            .style(Style::default().fg(TEXT_PRIMARY).bg(NOTICE_BG))
            .wrap(Wrap { trim: true }),
        content,
    );
}

fn render_command_palette_local(
    f: &mut Frame,
    input_comp: &crate::app::components::input::InputComponent,
    area: ratatui::layout::Rect,
    layout: UiLayout,
) {
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(COMMAND_PALETTE_BG)),
        area,
    );

    let name_width = input_comp
        .filtered_commands
        .iter()
        .take(5)
        .map(|cmd| cmd.name.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(12, 24)
        + 1;

    let content_width = area.width as usize;
    let list_left_padding = layout.command_palette_left_padding;
    let left_padding = " ".repeat(list_left_padding);
    let description_width = content_width.saturating_sub(list_left_padding + name_width + 1);

    let items: Vec<ListItem> = input_comp
        .filtered_commands
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, cmd)| {
            let style = if i == input_comp.selected_command_index {
                Style::default().fg(Color::White).bg(ACCENT)
            } else {
                Style::default().fg(TEXT_PRIMARY).bg(COMMAND_PALETTE_BG)
            };

            let description = truncate_chars(&cmd.description, description_width);

            ListItem::new(Line::from(vec![
                Span::raw(left_padding.clone()),
                Span::styled(format!("{:<name_width$}", cmd.name), Style::default()),
                Span::raw(" "),
                Span::styled(
                    description,
                    if i == input_comp.selected_command_index {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(TEXT_SECONDARY)
                    },
                ),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).style(Style::default().bg(COMMAND_PALETTE_BG));
    f.render_widget(list, area);
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn compute_popup_area(
    request: PopupRequest,
    viewport: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let mut area = Area::default();
    area.x = viewport.x;
    area.y = viewport.y;
    area.width = viewport.width;
    area.height = viewport.height;

    let popup = popup_from_request(request, area);
    ratatui::layout::Rect {
        x: popup.x,
        y: popup.y,
        width: popup.width,
        height: popup.height,
    }
}

fn command_palette_popup_area(
    input_left: u16,
    input_top: u16,
    popup_width: u16,
    popup_height: u16,
    viewport: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let mut options = PopupOptions::default();
    options.anchor = Anchor::BottomLeft;
    options.clear_background = true;

    let mut request = PopupRequest::default();
    request.anchor_x = input_left;
    request.anchor_y = input_top.saturating_sub(1);
    request.width = popup_width;
    request.height = popup_height;
    request.options = options;
    request.offset = Offset::default();

    compute_popup_area(request, viewport)
}

fn clipboard_popup_area(
    anchor_x: u16,
    anchor_y: u16,
    width: u16,
    height: u16,
    viewport: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let mut options = PopupOptions::default();
    options.anchor = Anchor::TopLeft;
    options.clear_background = true;

    let mut offset = Offset::default();
    offset.dx = 1;
    offset.dy = -1;

    let mut request = PopupRequest::default();
    request.anchor_x = anchor_x;
    request.anchor_y = anchor_y;
    request.width = width;
    request.height = height;
    request.options = options;
    request.offset = offset;

    compute_popup_area(request, viewport)
}
