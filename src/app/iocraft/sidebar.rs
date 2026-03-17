use iocraft::prelude::*;
use crate::app::ui::UiLine;
use super::utils::ui_line_to_mixed_text;

#[derive(Default, Props)]
pub struct SidebarProps {
    pub lines: Vec<UiLine>,
}

#[component]
pub fn Sidebar(props: &SidebarProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            flex_direction: FlexDirection::Column,
        ) {
            #(props.lines.iter().map(|line| {
                ui_line_to_mixed_text(line)
            }))
        }
    }
}
