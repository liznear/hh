use super::theme;
use iocraft::prelude::*;

#[derive(Default, Props)]
pub struct SidebarProps {
    pub session_name: String,
    pub subagent_name: Option<String>,
    pub cwd: String,
    pub context_percent: u32,
}

#[component]
pub fn Sidebar(props: &SidebarProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            flex_direction: FlexDirection::Column,
            padding: 1,
            gap: 1,
        ) {
            // Session Info
            View(flex_direction: FlexDirection::Column) {
                Text(content: props.session_name.clone(), color: theme::text_primary(), weight: Weight::Bold)

                #(if let Some(sub) = &props.subagent_name {
                    Some(element!(Text(content: format!("→ {}", sub), color: theme::text_primary())))
                } else {
                    None
                })

                Text(content: props.cwd.clone(), color: theme::text_muted())
            }

            // Context
            View(flex_direction: FlexDirection::Column) {
                Text(content: "▼ Context", color: theme::text_secondary())
                Text(content: format!("  {}% used", props.context_percent), color: theme::context_usage_yellow())
            }

            // Modified Files
            View(flex_direction: FlexDirection::Column) {
                Text(content: "▼ Modified Files", color: theme::text_secondary())
                Text(content: "  src/main.rs  +5 -2", color: theme::text_primary())
            }

            // TODO
            View(flex_direction: FlexDirection::Column) {
                Text(content: "▼ TODO (1/2)", color: theme::text_secondary())
                Text(content: "  [✓] First task", color: theme::text_muted())
                Text(content: "  [ ] Second task", color: theme::todo_active_fg())
            }
        }
    }
}
