use iocraft::prelude::*;

const SIDEBAR_WIDTH: u32 = 45;
const MIN_WIDTH_FOR_SIDEBAR: u16 = 160;

#[component]
fn App(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let show_sidebar = width >= MIN_WIDTH_FOR_SIDEBAR;

    // Light grey color for sidebar
    let sidebar_bg = Color::Rgb {
        r: 220,
        g: 220,
        b: 220,
    };
    let main_bg = Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    };

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
                background_color: main_bg,
                flex_grow: 1.0,
                flex_shrink: 0.0,
            ) {
                Text(content: "Main Column", color: Color::Black)
            }
            // Sidebar column - fixed 45 chars width, conditionally shown
            #(if show_sidebar {
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        width: SIDEBAR_WIDTH,
                        height: height as u32,
                        background_color: sidebar_bg,
                        flex_shrink: 0.0,
                    ) {
                        Text(content: "Sidebar", color: Color::Black)
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
