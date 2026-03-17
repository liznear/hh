use iocraft::prelude::*;

use super::theme;
use crate::theme::colors::UiLayout;

#[component]
pub fn AppRoot(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let layout = UiLayout::default();

    element! {
        View(
            width: width as u32,
            height: height as u32,
            background_color: theme::page_bg(),
            flex_direction: FlexDirection::Row,
        ) {
            // Main column
            View(
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
            ) {
                // Messages, processing, input
            }
            // Sidebar
            View(
                width: layout.sidebar_width as u32,
                background_color: theme::sidebar_bg(),
            ) {}
        }
    }
}
