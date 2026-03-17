use super::utils::ui_line_to_mixed_text;
use crate::app::ui::UiLine;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct MessagesPanelProps {
    pub lines: Vec<UiLine>,
}

#[component]
pub fn MessagesPanel(props: &MessagesPanelProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
        ) {
            #(props.lines.iter().map(|line| {
                ui_line_to_mixed_text(line)
            }))
        }
    }
}
