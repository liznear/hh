use super::theme;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct InputPanelProps {
    pub value: String,
    pub is_question_mode: bool,
    pub active_agent: String,
    pub active_model: String,
    pub duration: String,
}

#[component]
pub fn InputPanel(_hooks: Hooks, props: &InputPanelProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            flex_direction: FlexDirection::Column,
            background_color: theme::input_panel_bg(),
            border_style: BorderStyle::Round,
            border_color: theme::accent(),
        ) {
            #(if props.is_question_mode {
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Column,
                        padding: 1,
                    ) {
                        Text(content: "Question text here?", color: theme::text_primary(), weight: Weight::Bold)
                        Text(content: "1. Option One", color: theme::text_primary())
                        Text(content: "2. Option Two", color: theme::text_primary())
                    }
                })
            } else {
                Some(element! {
                    View(
                        flex_direction: FlexDirection::Row,
                        padding: 1,
                    ) {
                        Text(content: "▌ ", color: theme::accent())
                        TextInput(
                            has_focus: true,
                            value: props.value.clone(),
                            on_change: |_| {},
                        )
                    }
                })
            })

            // Status line
            View(
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                padding_left: 1,
                padding_right: 1,
            ) {
                View(flex_direction: FlexDirection::Row) {
                    Text(content: props.active_agent.clone(), color: theme::accent())
                    Text(content: "  ")
                    Text(content: props.active_model.clone(), color: theme::text_muted())
                }
                Text(content: props.duration.clone(), color: theme::text_muted())
            }
        }
    }
}
