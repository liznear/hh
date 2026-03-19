use crate::tui::theme::current_theme;
use iocraft::prelude::*;

const HORIZONTAL_PADDING: u32 = 2;

/// A single message in the chat
#[derive(Clone, Debug)]
pub struct Message {
    /// The role of the message sender
    pub role: MessageRole,
    /// The content of the message
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MessageRole {
    #[default]
    User,
    #[allow(dead_code)]
    Assistant,
}

/// The props which can be passed to the [`MessageArea`] component.
#[non_exhaustive]
#[derive(Default, Props)]
pub struct MessageAreaProps {
    /// The messages to display
    pub messages: Vec<Message>,
}

#[component]
pub fn MessageArea(_hooks: Hooks, props: &mut MessageAreaProps) -> impl Into<AnyElement<'static>> {
    let theme = current_theme();
    let messages = props.messages.clone();

    // Create child elements for each message
    let children: Vec<AnyElement<'static>> = messages
        .iter()
        .map(|message| {
            element! {
                View(
                    flex_direction: FlexDirection::Column,
                    width: 100pct,
                    margin_bottom: 1,
                ) {
                    // Role label
                    Text(
                        content: if message.role == MessageRole::User {
                            "You"
                        } else {
                            "Assistant"
                        },
                        color: theme.foreground_tertiary(),
                        weight: Weight::Bold,
                    )
                    // Message content
                    View(
                        flex_direction: FlexDirection::Column,
                        width: 100pct,
                        padding: 1,
                        background_color: if message.role == MessageRole::User {
                            theme.background_tertiary()
                        } else {
                            theme.background_secondary()
                        },
                    ) {
                        Text(
                            content: message.content.clone(),
                            color: theme.foreground(),
                            wrap: TextWrap::Wrap,
                        )
                    }
                }
            }
            .into()
        })
        .collect();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
            padding_left: HORIZONTAL_PADDING,
            padding_right: HORIZONTAL_PADDING,
            background_color: theme.background(),
            overflow: Overflow::Scroll,
        ) {
            Fragment(
                children,
            )
        }
    }
}
