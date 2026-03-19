use crate::tui::theme::current_theme;
use iocraft::prelude::*;

const SIDEBAR_WIDTH: u32 = 45;
const MIN_WIDTH_FOR_SIDEBAR: u16 = 160;

#[component]
fn App(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let show_sidebar = width >= MIN_WIDTH_FOR_SIDEBAR;
    let theme = current_theme();

    let main_width = if show_sidebar {
        width as u32 - SIDEBAR_WIDTH
    } else {
        width as u32
    };

    element! {
        View(
            flex_direction: FlexDirection::Row,
            width: width as u32,
            height: height as u32,
        ) {
            // Main column - takes remaining space
            View(
                flex_direction: FlexDirection::Column,
                width: main_width,
                height: height as u32,
                background_color: theme.background(),
                flex_grow: 1.0,
                flex_shrink: 0.0,
            ) {
                Text(content: "Main Column", color: theme.foreground())
            }
            // Sidebar column - fixed 45 chars width, conditionally shown
            #(if show_sidebar {
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        width: SIDEBAR_WIDTH,
                        height: height as u32,
                        background_color: theme.background_tertiary(),
                        flex_shrink: 0.0,
                    ) {
                        Text(content: "Sidebar", color: theme.foreground())
                    }
                })
            } else {
                None
            })
        }
    }
}

pub fn run_app() -> anyhow::Result<()> {
    smol::block_on(async { element!(App).fullscreen().await })?;
    Ok(())
}
