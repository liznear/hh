pub mod components;
pub mod core;
pub mod events;
pub mod chat_state;
pub mod handlers;
pub mod input;
pub mod render;
pub mod state;
pub mod terminal;
pub mod utils;

use std::path::Path;
use std::time::Duration;

use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::cli::agent_init;
use crate::app::input::{
    InputEvent, apply_paste, handle_area_scroll, handle_input_batch, handle_key_event,
    handle_mouse_click, handle_mouse_drag, handle_mouse_release, load_session_messages,
};
use crate::app::chat_state::ChatApp;
use crate::app::core::AppAction;
use crate::app::events::{ScopedTuiEvent, TuiEvent, TuiEventSender};
use crate::app::state::{App as MvuApp, AppState};
use crate::config::Settings;

pub async fn run_interactive_chat(settings: Settings, cwd: &Path) -> anyhow::Result<()> {
    let terminal = terminal::setup_terminal()?;
    let mut tui_guard = terminal::TuiGuard::new(terminal);

    let mut app = ChatApp::new(utils::build_session_name(cwd), cwd);
    app.configure_models(
        settings.selected_model_ref().to_string(),
        utils::build_model_options(&settings),
    );

    let (agent_views, selected_agent) = agent_init::initialize_agents(&settings)?;
    app.set_agents(agent_views, selected_agent);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ScopedTuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);
    handlers::subagent::initialize_subagent_manager(settings.clone(), cwd.to_path_buf());

    run_interactive_chat_loop(
        &mut tui_guard,
        &mut app,
        InteractiveChatRunner {
            settings: &settings,
            cwd,
            event_sender: &event_sender,
            event_rx: &mut event_rx,
            scroll_down_lines: 3,
        },
    )
    .await
}

pub async fn run_single_prompt(
    settings: Settings,
    cwd: &Path,
    prompt: String,
) -> anyhow::Result<String> {
    crate::app::handlers::runner::run_single_prompt(settings, cwd, prompt).await
}

const EVENT_DRAIN_MAX: usize = 128;
const STREAM_CHUNK_FLUSH_INTERVAL: Duration = Duration::from_millis(75);
const STREAM_CHUNK_FLUSH_BYTES: usize = 8192;

struct InteractiveChatRunner<'a> {
    settings: &'a Settings,
    cwd: &'a Path,
    event_sender: &'a TuiEventSender,
    event_rx: &'a mut mpsc::UnboundedReceiver<ScopedTuiEvent>,
    scroll_down_lines: usize,
}

async fn run_interactive_chat_loop(
    tui_guard: &mut terminal::TuiGuard,
    app: &mut ChatApp,
    runner: InteractiveChatRunner<'_>,
) -> anyhow::Result<()> {
    let mut render_tick = tokio::time::interval(Duration::from_millis(100));
    let mut stream_flush_tick = tokio::time::interval(STREAM_CHUNK_FLUSH_INTERVAL);
    let mut mvu_app = MvuApp::new(AppState::new(runner.cwd.to_path_buf()));
    mvu_app.dispatch(AppAction::Redraw);
    let mut flush_stream_before_draw = false;
    let mut pending_assistant_delta = String::new();
    let mut pending_thinking = String::new();

    loop {
        if mvu_app.take_needs_redraw() {
            if flush_stream_before_draw {
                flush_stream_chunks(app, &mut pending_thinking, &mut pending_assistant_delta);
                flush_stream_before_draw = false;
            }
            tui_guard.get().draw(|f| {
                crate::app::render::render_app(f, app, &mvu_app);
                mvu_app.render_components(f, f.area());
            })?;
        }

        tokio::select! {
            input_result = handle_input_batch() => {
                let input_events = input_result?;
                let mut handled_any_input = false;
                for input_event in input_events {
                    handled_any_input = true;
                    mvu_app.dispatch(AppAction::Input(input_event.clone()));
                    mvu_app.handle_input_event(&input_event);
                    match input_event {
                    InputEvent::Key(key_event) => {
                        handle_key_event(
                            key_event,
                            app,
                            &mvu_app.messages,
                            runner.settings,
                            runner.cwd,
                            runner.event_sender,
                            || {
                                let size = tui_guard.get().size()?;
                                Ok((size.width, size.height))
                            },
                        )?;
                    }
                    InputEvent::Paste(text) => {
                        apply_paste(app, text);
                    }
                    InputEvent::ScrollUp { x, y } => {
                        let terminal_size = tui_guard.get().size()?;
                        let terminal_rect = Rect {
                            x: 0,
                            y: 0,
                            width: terminal_size.width,
                            height: terminal_size.height,
                        };
                        handle_area_scroll(app, &mvu_app.messages, terminal_rect, x, y, 3, 0);
                    }
                    InputEvent::ScrollDown { x, y } => {
                        let terminal_size = tui_guard.get().size()?;
                        let terminal_rect = Rect {
                            x: 0,
                            y: 0,
                            width: terminal_size.width,
                            height: terminal_size.height,
                        };
                        handle_area_scroll(
                            app, &mvu_app.messages,
                            terminal_rect,
                            x,
                            y,
                            0,
                            runner.scroll_down_lines,
                        );
                    }
                    InputEvent::Refresh => {
                        tui_guard.get().autoresize()?;
                        tui_guard.get().clear()?;
                    }
                    InputEvent::MouseClick { x, y } => {
                        handle_mouse_click(app, &mvu_app.messages, x, y, tui_guard.get(), runner.settings, runner.cwd);
                    }
                    InputEvent::MouseDrag { x, y } => {
                        handle_mouse_drag(app, &mvu_app.messages, x, y, tui_guard.get());
                    }
                    InputEvent::MouseRelease { x, y } => {
                        if let Some(action) = handle_mouse_release(app, &mvu_app.messages, x, y, tui_guard.get()) {
                            mvu_app.dispatch(action);
                        }
                    }
                    }
                }
                if handled_any_input { mvu_app.dispatch(AppAction::Redraw); }
            }
            event = runner.event_rx.recv() => {
                if let Some(event) = event
                    && event.session_epoch == app.session_epoch()
                    && event.run_epoch == app.run_epoch()
                {
                    let mut handled_non_stream_event = false;
                    merge_or_handle_event(
                        app,
                        &mut mvu_app,
                        event.event,
                        &mut pending_thinking,
                        &mut pending_assistant_delta,
                        &mut handled_non_stream_event,
                    );

                    for _ in 0..EVENT_DRAIN_MAX {
                        let Ok(next_event) = runner.event_rx.try_recv() else {
                            break;
                        };
                        if next_event.session_epoch == app.session_epoch()
                            && next_event.run_epoch == app.run_epoch()
                        {
                            merge_or_handle_event(
                                app,
                                &mut mvu_app,
                                next_event.event,
                                &mut pending_thinking,
                                &mut pending_assistant_delta,
                                &mut handled_non_stream_event,
                            );
                        }
                    }

                    if handled_non_stream_event {
                        mvu_app.dispatch(AppAction::Redraw);
                    }
                    if pending_assistant_delta.len() >= STREAM_CHUNK_FLUSH_BYTES
                        || pending_thinking.len() >= STREAM_CHUNK_FLUSH_BYTES
                    {
                        flush_stream_before_draw = true;
                        mvu_app.dispatch(AppAction::Redraw);
                    }
                }
            }
            _ = stream_flush_tick.tick() => {
                if !pending_assistant_delta.is_empty() || !pending_thinking.is_empty() {
                    flush_stream_before_draw = true;
                    mvu_app.dispatch(AppAction::Redraw);
                }
            }
            _ = render_tick.tick() => {
                mvu_app.dispatch(AppAction::PeriodicTick);
                if let Some(subagent_view) = app.active_subagent_session()
                    && let Ok(messages) = load_session_messages(
                        runner.settings,
                        runner.cwd,
                        &subagent_view.session_id,
                    )
                {
                    app.replace_active_subagent_messages(messages);
                    mvu_app.dispatch(AppAction::Redraw);
                }

                if app.on_periodic_tick() {
                    mvu_app.dispatch(AppAction::Redraw);
                }
            }
        }

        if app.should_quit || mvu_app.state.should_quit {
            break;
        }
    }

    Ok(())
}

fn merge_or_handle_event(
    app: &mut ChatApp,
    mvu_app: &mut MvuApp,
    event: TuiEvent,
    pending_thinking: &mut String,
    pending_assistant_delta: &mut String,
    handled_non_stream_event: &mut bool,
) {
    match event {
        TuiEvent::Thinking(chunk) => pending_thinking.push_str(&chunk),
        TuiEvent::AssistantDelta(chunk) => pending_assistant_delta.push_str(&chunk),
        other => {
            flush_stream_chunks(app, pending_thinking, pending_assistant_delta);
            mvu_app.dispatch(AppAction::AgentEvent(other.clone()));
            app.handle_event(&other);
            *handled_non_stream_event = true;
        }
    }
}

fn flush_stream_chunks(
    app: &mut ChatApp,
    pending_thinking: &mut String,
    pending_assistant_delta: &mut String,
) {
    if !pending_thinking.is_empty() {
        let chunk = std::mem::take(pending_thinking);
        app.handle_event(&TuiEvent::Thinking(chunk));
    }
    if !pending_assistant_delta.is_empty() {
        let chunk = std::mem::take(pending_assistant_delta);
        app.handle_event(&TuiEvent::AssistantDelta(chunk));
    }
}
