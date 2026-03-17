use iocraft::prelude::*;

use super::input_mapper::map_terminal_event;
use super::theme;
use crate::app::core::AppAction;
use crate::app::iocraft::root::IocraftAppRunner;
use crate::theme::colors::UiLayout;
use smol::stream::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[component]
pub fn AppRoot(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut system = hooks.use_context_mut::<iocraft::SystemContext>();
    let runner_arc = hooks.use_context::<Arc<Mutex<IocraftAppRunner>>>();

    // We use a state just to trigger redraws
    let mut redraw_trigger = hooks.use_state(|| 0);

    let (width, height) = hooks.use_terminal_size();
    let term_size = (width, height);
    let layout = UiLayout::default();

    let runner_arc_for_events = runner_arc.clone();
    hooks.use_terminal_events({
        move |event| {
            if let Some(mapped_event) = map_terminal_event(&event) {
                let runner = runner_arc_for_events.lock().unwrap();

                let mut mvu_app = runner.mvu_app.lock().unwrap();
                mvu_app.dispatch(AppAction::Input(mapped_event.clone()));
                mvu_app.handle_input_event_with_runtime(
                    &mapped_event,
                    Some(&runner.settings),
                    Some(&runner.cwd),
                    Some(&runner.event_sender),
                );

                match mapped_event {
                    crate::app::events::InputEvent::Key(k) => {
                        let _ = mvu_app.process_key_event(
                            k,
                            &runner.settings,
                            &runner.cwd,
                            &runner.event_sender,
                            || Ok(term_size),
                        );
                    }
                    crate::app::events::InputEvent::Paste(text) => {
                        mvu_app.process_paste(text);
                    }
                    crate::app::events::InputEvent::ScrollUp { x, y } => {
                        let terminal_rect = crate::app::ui::geometry::Rect {
                            x: 0,
                            y: 0,
                            width: term_size.0,
                            height: term_size.1,
                        };
                        mvu_app.process_area_scroll(terminal_rect, x, y, 3, 0);
                    }
                    crate::app::events::InputEvent::ScrollDown { x, y } => {
                        let terminal_rect = crate::app::ui::geometry::Rect {
                            x: 0,
                            y: 0,
                            width: term_size.0,
                            height: term_size.1,
                        };
                        mvu_app.process_area_scroll(terminal_rect, x, y, 0, 3);
                    }
                    crate::app::events::InputEvent::MouseClick { x, y } => {
                        mvu_app.process_mouse_click(
                            x,
                            y,
                            term_size,
                            &runner.settings,
                            &runner.cwd,
                            &runner.event_sender,
                        );
                    }
                    crate::app::events::InputEvent::MouseDrag { x, y } => {
                        mvu_app.process_mouse_drag(x, y, term_size);
                    }
                    crate::app::events::InputEvent::MouseRelease { x, y } => {
                        mvu_app.process_mouse_release(x, y, term_size);
                    }
                    _ => {}
                }

                redraw_trigger.set(redraw_trigger.get() + 1);
            }
        }
    });

    let runner_arc_for_tick = runner_arc.clone();
    let mut redraw_trigger_for_tick = redraw_trigger;
    hooks.use_future(async move {
        let mut tick_interval = smol::Timer::interval(Duration::from_millis(100));
        let mut event_rx = {
            let mut runner = runner_arc_for_tick.lock().unwrap();
            runner.event_rx.take().expect("event_rx should be present")
        };
        loop {
            tokio::select! {
                _ = tick_interval.next() => {
                    let runner = runner_arc_for_tick.lock().unwrap();
                    let mut mvu_app = runner.mvu_app.lock().unwrap();
                    mvu_app.dispatch(AppAction::PeriodicTick);
                    mvu_app.process_periodic_tick(
                        &runner.settings,
                        &runner.cwd,
                        &runner.event_sender,
                    );
                    redraw_trigger_for_tick.set(redraw_trigger_for_tick.get() + 1);
                }
                event = event_rx.recv() => {
                    if let Some(event) = event {
                        let runner = runner_arc_for_tick.lock().unwrap();
                        let mut mvu_app = runner.mvu_app.lock().unwrap();
                        mvu_app.dispatch(AppAction::AgentEvent(event.event));
                        redraw_trigger_for_tick.set(redraw_trigger_for_tick.get() + 1);
                    }
                }
            }
        }
    });

    if runner_arc
        .lock()
        .unwrap()
        .mvu_app
        .lock()
        .unwrap()
        .state
        .should_quit
    {
        system.exit();
    }

    let runner = runner_arc.lock().unwrap();
    let mut mvu_app = runner.mvu_app.lock().unwrap();

    // We assume the available height for messages is height - input_panel_height.
    // For now we'll just give it a rough estimate (e.g. height - 3).
    let messages_height = height.saturating_sub(5) as usize;
    let messages_width = width.saturating_sub(layout.sidebar_width + 3) as usize;

    let ratatui_lines = mvu_app.get_message_lines(messages_width, messages_height);
    let ui_lines = ratatui_lines;

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
                super::messages::MessagesPanel(lines: ui_lines)

                // Input
                super::input::InputPanel(
                    value: mvu_app.state.input.clone(),
                    is_question_mode: false,
                    active_agent: mvu_app.state.selected_agent().map(|a| a.name.as_str()).unwrap_or("").to_string(),
                    active_model: mvu_app.state.current_model_ref.clone(),
                    duration: "".to_string(),
                )
            }
            // Sidebar
            View(
                width: layout.sidebar_width as u32,
                background_color: theme::sidebar_bg(),
            ) {
                super::sidebar::Sidebar(
                    lines: {
                        let ratatui_lines = mvu_app.get_sidebar_lines(layout.sidebar_width, messages_height);
                        ratatui_lines.into_iter().collect::<Vec<_>>()
                    }
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
