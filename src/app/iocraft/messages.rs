use super::theme;
use crate::core::{Message, Role};
use crate::theme::colors::UiLayout;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct MessagesPanelProps {
    pub messages: Vec<Message>,
}

#[component]
pub fn MessagesPanel(props: &MessagesPanelProps) -> impl Into<AnyElement<'static>> {
    let layout = UiLayout::default();

    element! {
        View(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            overflow_y: Overflow::Scroll,
        ) {
            #(props.messages.iter().map(|msg| {
                match msg.role {
                    Role::User => {
                        element! {
                            View(flex_direction: FlexDirection::Row, margin_bottom: 1) {
                                Text(content: "▌ ", color: theme::accent())
                                View(
                                    background_color: theme::input_panel_bg(),
                                    padding_left: layout.user_bubble_inner_padding as i32,
                                    padding_right: layout.user_bubble_inner_padding as i32,
                                ) {
                                    Text(content: msg.content.clone(), color: theme::text_primary())
                                }
                            }
                        }
                    }
                    Role::Assistant => {
                        element! {
                            View(
                                flex_direction: FlexDirection::Column,
                                margin_left: layout.message_indent_width as i32,
                                margin_bottom: 1,
                            ) {
                                Text(content: msg.content.clone(), color: theme::text_primary())
                                #(msg.tool_calls.iter().map(|_tc| {
                                    // Dummy tool call render for now
                                    element!(Text(content: "→ Tool Call", color: theme::text_muted()))
                                }))
                            }
                        }
                    }
                    Role::Tool => {
                        element! {
                            View(
                                flex_direction: FlexDirection::Row,
                                margin_left: layout.message_indent_width as i32,
                                margin_bottom: 1,
                            ) {
                                Text(content: "✓ Tool Output", color: theme::input_accent())
                            }
                        }
                    }
                    Role::System => {
                        element!(View() {})
                    }
                }
            }))
        }
    }
}
