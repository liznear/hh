use super::theme;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct CommandPaletteProps {
    pub is_visible: bool,
    pub query: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
}

#[component]
pub fn CommandPalette(props: &CommandPaletteProps) -> impl Into<AnyElement<'static>> {
    if props.is_visible {
        element! {
            View(
                position: Position::Absolute,
                left: props.x,
                top: props.y,
                width: props.width,
                flex_direction: FlexDirection::Column,
                background_color: theme::command_palette_bg(),
                border_style: BorderStyle::Round,
                border_color: theme::accent(),
            ) {
                Text(content: "  /new        Start a new session", color: theme::text_primary())
                Text(content: "  /model      Switch models", color: theme::text_primary())
                Text(content: "  /resume     Resume a session", color: theme::text_primary())
            }
        }
        .into_any()
    } else {
        element!(View(display: Display::None) {}).into_any()
    }
}

#[derive(Default, Props)]
pub struct ClipboardNoticeProps {
    pub is_visible: bool,
    pub x: i32,
    pub y: i32,
}

#[component]
pub fn ClipboardNotice(props: &ClipboardNoticeProps) -> impl Into<AnyElement<'static>> {
    if props.is_visible {
        element! {
            View(
                position: Position::Absolute,
                left: props.x,
                top: props.y,
                background_color: theme::notice_bg(),
                border_style: BorderStyle::Round,
                border_color: theme::accent(),
                padding_left: 1,
                padding_right: 1,
            ) {
                Text(content: "Copied", color: theme::text_primary())
            }
        }
        .into_any()
    } else {
        element!(View(display: Display::None) {}).into_any()
    }
}
