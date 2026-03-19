use crate::tui::theme::current_theme;
use iocraft::prelude::*;

const MAX_INPUT_HEIGHT: usize = 8;
const INITIAL_HEIGHT: u32 = 5;
const HORIZONTAL_PADDING: u32 = 2;

/// The props which can be passed to [`InputArea`] component.
#[non_exhaustive]
#[derive(Default, Props)]
pub struct InputAreaProps {
    /// The handler to invoke when content is submitted (Enter pressed).
    pub on_submit: HandlerMut<'static, String>,
}

#[component]
pub fn InputArea(mut hooks: Hooks, props: &mut InputAreaProps) -> impl Into<AnyElement<'static>> {
    let theme = current_theme();
    let mut value = hooks.use_state(String::new);

    // Count lines in current value (minimum INITIAL_HEIGHT)
    let line_count = if value.read().is_empty() {
        INITIAL_HEIGHT
    } else {
        value.read().lines().count().max(INITIAL_HEIGHT as usize) as u32
    };
    let display_height = line_count.min(MAX_INPUT_HEIGHT as u32);

    // Handle keyboard input manually
    hooks.use_terminal_events({
        let mut value = value;
        let mut on_submit = props.on_submit.take();
        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind == KeyEventKind::Press
            {
                if key_event.code == KeyCode::Enter && key_event.modifiers.is_empty() {
                    let content = value.read().clone();
                    if !content.trim().is_empty() {
                        (on_submit)(content);
                        value.set(String::new());
                    }
                }
                if key_event.code == KeyCode::Char('j')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)
                {
                    let mut new_value = value.read().clone();
                    new_value.push('\n');
                    value.set(new_value);
                }
            }
        }
    });

    element! {
        View(
            width: 100pct,
            background_color: theme.background_secondary(),
            border_style: BorderStyle::Single,
            border_color: theme.background_tertiary(),
        ) {
            View(
                padding_left: HORIZONTAL_PADDING,
                padding_right: HORIZONTAL_PADDING,
                width: 100pct,
                height: display_height,
            ) {
                TextInput(
                    has_focus: true,
                    value: value.to_string(),
                    cursor_color: theme.background_tertiary(),
                    on_change: move |new_value| value.set(new_value),
                    multiline: true
                )
            }
        }
    }
}
