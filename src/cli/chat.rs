use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEvent, MouseEventKind};
use tokio::sync::mpsc;

use crate::cli::render;
use crate::cli::tui::{self, ChatApp, DebugRenderer, TuiEvent, TuiEventSender};
use crate::config::Settings;
use crate::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::SessionStore;
use crate::tool::registry::ToolRegistry;

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd, settings.agent.token_budget);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    run_interactive_chat_loop(
        &mut tui_guard,
        &mut app,
        InteractiveChatRunner {
            settings: &settings,
            cwd,
            event_sender: &event_sender,
            event_rx: &mut event_rx,
            debug_renderer: None,
            scroll_down_lines: 3,
        },
    )
    .await?;

    Ok(())
}

/// Run interactive chat with debug frame dumping
pub async fn run_chat_with_debug(
    settings: Settings,
    cwd: &std::path::Path,
    debug_dir: PathBuf,
) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create debug renderer
    let mut debug_renderer = DebugRenderer::new(debug_dir.clone())?;

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd, settings.agent.token_budget);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    run_interactive_chat_loop(
        &mut tui_guard,
        &mut app,
        InteractiveChatRunner {
            settings: &settings,
            cwd,
            event_sender: &event_sender,
            event_rx: &mut event_rx,
            debug_renderer: Some(&mut debug_renderer),
            scroll_down_lines: 1,
        },
    )
    .await?;

    eprintln!(
        "Debug: {} frames written to {}",
        debug_renderer.frame_count(),
        debug_dir.display()
    );

    Ok(())
}

/// Run one prompt in headless debug mode and dump frames to files
pub async fn run_prompt_with_debug(
    settings: Settings,
    cwd: &std::path::Path,
    output_dir: PathBuf,
    prompt: String,
) -> anyhow::Result<()> {
    // Create debug renderer
    let mut renderer = DebugRenderer::new(output_dir.clone())?;

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd, settings.agent.token_budget);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    // Submit the prompt
    app.messages.push(tui::ChatMessage::User(prompt.clone()));
    app.begin_prompt_progress(prompt.clone());
    app.push_progress_line("user: submitted prompt".to_string());
    app.set_processing(true);

    // Render initial state with prompt
    renderer.render(&app)?;

    println!(
        "Debug mode: writing screen dumps to {}",
        output_dir.display()
    );

    // Run agent in background
    let settings_clone = settings.clone();
    let cwd_clone = cwd.to_path_buf();
    let sender_clone = event_sender.clone();
    let prompt_clone = prompt.clone();

    let mut agent_handle = tokio::spawn(async move {
        let _ = run_agent(settings_clone, &cwd_clone, prompt_clone, sender_clone).await;
    });

    // Main loop - process events and render
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    app.handle_event(&event);

                    // Render after each event
                    renderer.render(&app)?;

                    // Check if processing is done
                    if matches!(event, TuiEvent::AssistantDone) {
                        // Render final state
                        renderer.render(&app)?;
                        break;
                    }
                } else {
                    // Channel closed, we're done
                    break;
                }
            }

            result = &mut agent_handle => {
                // Agent task finished - just log, don't exit
                // Continue processing events until AssistantDone or channel closes
                if let Err(e) = result {
                    eprintln!("Agent task error: {}", e);
                }
            }
        }
    }

    println!(
        "Debug complete: {} frames written to {}",
        renderer.frame_count(),
        output_dir.display()
    );

    Ok(())
}

/// Input event from terminal
enum InputEvent {
    Key(event::KeyEvent),
    ScrollUp,
    ScrollDown,
}

async fn handle_input() -> anyhow::Result<Option<InputEvent>> {
    if event::poll(Duration::from_millis(16))? {
        match event::read()? {
            Event::Key(key) => Ok(Some(InputEvent::Key(key))),
            Event::Mouse(mouse) => Ok(handle_mouse_event(mouse)),
            _ => Ok(None),
        }
    } else {
        // No event available, yield and continue
        tokio::time::sleep(Duration::from_millis(8)).await;
        Ok(None)
    }
}

fn handle_key_event<F>(
    key_event: event::KeyEvent,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
    mut terminal_size: F,
) -> anyhow::Result<()>
where
    F: FnMut() -> anyhow::Result<(u16, u16)>,
{
    if key_event.code == KeyCode::Char('c') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return Ok(());
    }

    if key_event.code == KeyCode::Char('t') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        app.toggle_progress();
        return Ok(());
    }

    match key_event.code {
        KeyCode::Char(c) => {
            app.input.push(c);
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Enter => {
            let input = app.submit_input();
            if input == ":quit" {
                app.should_quit = true;
            } else if input == ":thinking" || input == ":progress" {
                app.toggle_progress();
            } else if !input.is_empty() {
                spawn_agent_task(settings, cwd, input, event_sender);
            }
        }
        KeyCode::Esc => {
            app.input.clear();
        }
        KeyCode::Up => {
            app.scroll_up();
        }
        KeyCode::Down => {
            let (width, height) = terminal_size()?;
            scroll_down_once(app, width, height);
        }
        KeyCode::PageUp => {
            let (_, height) = terminal_size()?;
            for _ in 0..app.message_viewport_height(height).saturating_sub(1) {
                app.scroll_up();
            }
        }
        KeyCode::PageDown => {
            let (width, height) = terminal_size()?;
            scroll_page_down(app, width, height);
        }
        _ => {}
    }

    Ok(())
}

fn scroll_down_once(app: &mut ChatApp, width: u16, height: u16) {
    let (total_lines, visible_height) = scroll_bounds(app, width, height);
    app.scroll_down(total_lines, visible_height);
}

fn scroll_page_down(app: &mut ChatApp, width: u16, height: u16) {
    let (total_lines, visible_height) = scroll_bounds(app, width, height);
    for _ in 0..visible_height.saturating_sub(1) {
        app.scroll_down(total_lines, visible_height);
    }
}

fn scroll_bounds(app: &ChatApp, width: u16, height: u16) -> (usize, usize) {
    let visible_height = app.message_viewport_height(height);
    let wrap_width = app.message_wrap_width(width);
    let lines = app.get_lines(wrap_width);
    let total_lines = lines.len();
    drop(lines);
    (total_lines, visible_height)
}

fn spawn_agent_task(settings: &Settings, cwd: &Path, input: String, event_sender: &TuiEventSender) {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let sender = event_sender.clone();
    tokio::spawn(async move {
        let _ = run_agent(settings, &cwd, input, sender).await;
    });
}

fn handle_mouse_event(mouse: MouseEvent) -> Option<InputEvent> {
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(InputEvent::ScrollUp),
        MouseEventKind::ScrollDown => Some(InputEvent::ScrollDown),
        _ => None,
    }
}

async fn run_interactive_chat_loop(
    tui_guard: &mut tui::TuiGuard,
    app: &mut ChatApp,
    mut runner: InteractiveChatRunner<'_>,
) -> anyhow::Result<()> {
    if let Some(renderer) = runner.debug_renderer.as_deref_mut() {
        renderer.render(app)?;
    }

    loop {
        tui_guard.get().draw(|f| tui::render_app(f, app))?;
        if let Some(renderer) = runner.debug_renderer.as_deref_mut() {
            renderer.render(app)?;
        }

        tokio::select! {
            input_result = handle_input() => {
                match input_result? {
                    Some(InputEvent::Key(key_event)) => {
                        handle_key_event(
                            key_event,
                            app,
                            runner.settings,
                            runner.cwd,
                            runner.event_sender,
                            || {
                                let size = tui_guard.get().size()?;
                                Ok((size.width, size.height))
                            },
                        )?;
                    }
                    Some(InputEvent::ScrollUp) => {
                        for _ in 0..3 {
                            app.scroll_up();
                        }
                    }
                    Some(InputEvent::ScrollDown) => {
                        let terminal_size = tui_guard.get().size()?;
                        for _ in 0..runner.scroll_down_lines {
                            scroll_down_once(app, terminal_size.width, terminal_size.height);
                        }
                    }
                    None => {}
                }
            }
            event = runner.event_rx.recv() => {
                if let Some(event) = event {
                    app.handle_event(&event);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

struct InteractiveChatRunner<'a> {
    settings: &'a Settings,
    cwd: &'a Path,
    event_sender: &'a TuiEventSender,
    event_rx: &'a mut mpsc::UnboundedReceiver<TuiEvent>,
    debug_renderer: Option<&'a mut DebugRenderer>,
    scroll_down_lines: usize,
}

fn build_session_name(cwd: &std::path::Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "Session".to_string())
}

async fn run_agent(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
    events: impl AgentEvents + 'static,
) -> anyhow::Result<()> {
    let loop_runner = create_agent_loop(settings, cwd, events)?;

    loop_runner
        .run(prompt, |_tool_name| {
            // For TUI mode, auto-approve tools (could prompt via TUI in future)
            Ok(true)
        })
        .await?;

    Ok(())
}

pub async fn run_single_prompt(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
) -> anyhow::Result<String> {
    run_single_prompt_with_events(settings, cwd, prompt, NoopEvents).await
}

pub async fn run_single_prompt_with_events<E>(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
    events: E,
) -> anyhow::Result<String>
where
    E: AgentEvents,
{
    let loop_runner = create_agent_loop(settings, cwd, events)?;

    loop_runner
        .run(prompt, |tool_name| {
            Ok(render::confirm(&format!(
                "Allow tool '{}' execution?",
                tool_name
            ))?)
        })
        .await
}

fn create_agent_loop<E>(
    settings: Settings,
    cwd: &std::path::Path,
    events: E,
) -> anyhow::Result<AgentLoop<OpenAiCompatibleProvider, E>>
where
    E: AgentEvents,
{
    let provider = OpenAiCompatibleProvider::new(
        settings.provider.base_url.clone(),
        settings.provider.model.clone(),
        settings.provider.api_key_env.clone(),
    );

    let tool_registry = ToolRegistry::new(&settings, cwd);
    let permissions = PermissionMatcher::new(settings.clone());
    let session = SessionStore::for_workspace(&settings.session.root, cwd)?;

    Ok(AgentLoop {
        provider,
        tool_registry,
        permissions,
        max_steps: settings.agent.max_steps,
        model: settings.provider.model,
        session,
        events,
    })
}
