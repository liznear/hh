use ratatui::layout::Rect;
use ratatui::style::Color;

pub(super) const MAX_TOOL_OUTPUT_LEN: usize = 200;
pub(super) const MIN_DIFF_COLUMN_WIDTH: usize = 24;
pub(super) const DIFF_LINE_NUMBER_WIDTH: usize = 4;
pub(super) const TOOL_PENDING_MARKER: &str = "→ ";
pub(super) const PROCESSING_STATUS_GAP: &str = "  ";
pub(super) const SIDEBAR_INDENT: &str = "  ";
pub(super) const SIDEBAR_LABEL_INDENT: &str = " ";

pub(super) const PAGE_BG: Color = Color::Rgb(246, 247, 251);
pub(super) const SIDEBAR_BG: Color = Color::Rgb(234, 238, 246);
pub(super) const INPUT_PANEL_BG: Color = Color::Rgb(229, 233, 241);
pub(super) const COMMAND_PALETTE_BG: Color = Color::Rgb(214, 220, 232);
pub(super) const TEXT_PRIMARY: Color = Color::Rgb(37, 45, 58);
pub(super) const TEXT_SECONDARY: Color = Color::Rgb(98, 108, 124);
pub(super) const TEXT_MUTED: Color = Color::Rgb(125, 133, 147);
pub(super) const ACCENT: Color = Color::Rgb(55, 114, 255);
pub(super) const INPUT_ACCENT: Color = Color::Rgb(19, 164, 151);
pub(super) const SELECTION_BG: Color = Color::Rgb(55, 114, 255);
pub(super) const NOTICE_BG: Color = Color::Rgb(224, 227, 233);
pub(super) const PROGRESS_HEAD: Color = Color::Rgb(124, 72, 227);
pub(super) const THINKING_LABEL: Color = Color::Rgb(227, 152, 67);
pub(super) const QUESTION_BORDER: Color = Color::Rgb(220, 96, 180);
pub(super) const CONTEXT_USAGE_YELLOW: Color = Color::Rgb(214, 168, 46);
pub(super) const CONTEXT_USAGE_ORANGE: Color = Color::Rgb(227, 136, 46);
pub(super) const CONTEXT_USAGE_RED: Color = Color::Rgb(196, 64, 64);
pub(super) const DIFF_ADD_FG: Color = Color::Rgb(25, 110, 61);
pub(super) const DIFF_ADD_BG: Color = Color::Rgb(226, 244, 235);
pub(super) const DIFF_REMOVE_FG: Color = Color::Rgb(152, 45, 45);
pub(super) const DIFF_REMOVE_BG: Color = Color::Rgb(252, 235, 235);
pub(super) const DIFF_META_FG: Color = Color::Rgb(106, 114, 128);
pub(super) const MAX_RENDERED_DIFF_LINES: usize = 120;
pub(super) const MAX_RENDERED_DIFF_CHARS: usize = 8_000;
pub(super) const MAX_INPUT_LINES: usize = 5;

#[derive(Clone, Copy)]
pub(crate) struct UiLayout {
    pub(super) sidebar_width: u16,
    pub(super) left_column_right_margin: u16,
    pub(super) main_outer_padding_x: u16,
    pub(super) main_outer_padding_y: u16,
    pub(super) main_content_left_offset: usize,
    pub(super) user_bubble_inner_padding: usize,
    pub(super) message_indent_width: usize,
    pub(super) command_palette_left_padding: usize,
}

impl Default for UiLayout {
    fn default() -> Self {
        let main_content_left_offset = 2;
        Self {
            sidebar_width: 38,
            left_column_right_margin: 2,
            main_outer_padding_x: 1,
            main_outer_padding_y: 1,
            main_content_left_offset,
            user_bubble_inner_padding: 1,
            message_indent_width: main_content_left_offset + 2,
            command_palette_left_padding: main_content_left_offset,
        }
    }
}

impl UiLayout {
    #[cfg(test)]
    pub(crate) const fn main_content_left_offset(&self) -> usize {
        self.main_content_left_offset
    }

    #[cfg(test)]
    pub(crate) const fn message_indent_width(&self) -> usize {
        self.message_indent_width
    }

    pub(super) fn user_bubble_indent(&self) -> usize {
        self.main_content_left_offset
    }

    pub(super) fn message_indent(&self) -> String {
        " ".repeat(self.message_indent_width)
    }

    pub(super) fn message_child_indent(&self) -> String {
        " ".repeat(self.message_indent_width + 2)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AppLayoutRects {
    pub main_messages: Option<Rect>,
    pub sidebar_content: Option<Rect>,
}
