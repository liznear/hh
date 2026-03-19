use crate::tui::components::{InputArea, Message, MessageArea};
use crate::tui::theme::current_theme;
use iocraft::prelude::*;

/// Layout configuration and computed dimensions for the app.
struct Layout {
    /// Terminal width in characters
    width: u32,
    /// Terminal height in characters
    height: u32,
    /// Padding applied to the app container (each side)
    padding: u32,
    /// Sidebar width in characters
    sidebar_width: u32,
    /// Minimum terminal width to show sidebar
    min_width_for_sidebar: u16,
}

impl Layout {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width: width as u32,
            height: height as u32,
            padding: 1,
            sidebar_width: 45,
            min_width_for_sidebar: 160,
        }
    }

    /// Whether the sidebar should be shown
    fn show_sidebar(&self) -> bool {
        self.width >= self.min_width_for_sidebar as u32
    }

    /// Available width for content (terminal width minus horizontal padding)
    fn available_width(&self) -> u32 {
        self.width - self.padding * 2
    }

    /// Available height for content (terminal height minus vertical padding)
    fn available_height(&self) -> u32 {
        self.height - self.padding * 2
    }

    /// Width of the main column
    fn main_width(&self) -> u32 {
        if self.show_sidebar() {
            self.available_width() - self.sidebar_width
        } else {
            self.available_width()
        }
    }
}

#[component]
fn App(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let layout = Layout::new(width, height);
    let theme = current_theme();
    let _system = hooks.use_context_mut::<SystemContext>();

    // State for messages
    let messages = hooks.use_state(Vec::<Message>::new);

    element! {
        View(
            flex_direction: FlexDirection::Row,
            width: layout.width,
            height: layout.height,
            padding: layout.padding,
        ) {
            // Main column - takes remaining space
            View(
                flex_direction: FlexDirection::Column,
                width: layout.main_width(),
                height: layout.available_height(),
                background_color: theme.background(),
                flex_grow: 1.0,
                flex_shrink: 0.0,
            ) {
                // Message area (top)
                View(
                    flex_direction: FlexDirection::Column,
                    width: 100pct,
                    flex_grow: 1.0,
                ) {
                    MessageArea(messages: messages.read().clone())
                }
                // Input area (bottom)
                InputArea(
                    on_submit: move |_content: String| {
                        // TODO: Implement submission to coding agent
                    }
                )
            }
            // Sidebar column - fixed width, conditionally shown
            #(if layout.show_sidebar() {
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        width: layout.sidebar_width,
                        height: layout.available_height(),
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
