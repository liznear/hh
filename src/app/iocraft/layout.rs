use iocraft::prelude::*;

use super::input_mapper::map_terminal_event;
use super::theme;
use crate::core::{Message, Role};
use crate::theme::colors::UiLayout;

#[component]
pub fn AppRoot(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = hooks.use_terminal_size();
    let layout = UiLayout::default();

    hooks.use_terminal_events(|event| {
        if let Some(mapped_event) = map_terminal_event(&event) {
            // Here we would dispatch mapped_event to MvuApp
            // For now, if Q is pressed, we exit
            if let crate::app::events::InputEvent::Key(k) = mapped_event {
                if k.code == crossterm::event::KeyCode::Char('q') {
                    // This is a temporary hack since MvuApp isn't fully integrated here
                    std::process::exit(0);
                }
            }
        }
    });

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
            ) {
                super::sidebar::Sidebar(
                    session_name: "Default Session".to_string(),
                    subagent_name: None,
                    cwd: "~/Developer/hh".to_string(),
                    context_percent: 35_u32,
                )
            }

            // Popups
            super::popups::CommandPalette(
                is_visible: false,
                query: "".to_string(),
                x: 0,
                y: 0,
                width: 50,
            )

            super::popups::ClipboardNotice(
                is_visible: false,
                x: 0,
                y: 0,
            )
        }
    }
}
