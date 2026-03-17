use iocraft::prelude::*;

use super::theme;
use crate::core::{Message, Role};
use crate::theme::colors::UiLayout;

#[component]
pub fn AppRoot(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let layout = UiLayout::default();

    let dummy_messages = vec![
        Message {
            role: Role::User,
            content: "Hello, agent!".to_string(),
            attachments: vec![],
            tool_call_id: None,
            tool_calls: vec![],
        },
        Message {
            role: Role::Assistant,
            content: "Hello! How can I help you today?".to_string(),
            attachments: vec![],
            tool_call_id: None,
            tool_calls: vec![],
        },
    ];

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
                // Messages
                super::messages::MessagesPanel(messages: dummy_messages)

                // Input
                super::input::InputPanel(
                    value: "Tell me more about this project...".to_string(),
                    is_question_mode: false,
                    active_agent: "AgentName".to_string(),
                    active_model: "Provider Model".to_string(),
                    duration: "1s".to_string(),
                )
            }
            // Sidebar
            View(
                width: layout.sidebar_width as u32,
                background_color: theme::sidebar_bg(),
            ) {}
        }
    }
}
