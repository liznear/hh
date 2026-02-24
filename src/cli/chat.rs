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
use crate::session::{SessionStore, SessionEvent};
use crate::tool::registry::ToolRegistry;
use uuid::Uuid;

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

    let title = prompt.chars().take(50).collect::<String>();
    let title_clone = title.clone();

    let agent_handle = tokio::spawn(async move {
        let result = run_agent(settings_clone, &cwd_clone, prompt_clone, sender_clone.clone(), None, Some(title_clone)).await;
        if let Err(ref e) = result {
            sender_clone.send(TuiEvent::Error(e.to_string()));
        }
        result
    });
    drop(event_sender); // Close the channel from this side

    // Main loop - process events and render
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    let is_done_or_error = matches!(event, TuiEvent::AssistantDone | TuiEvent::Error(_));
                    app.handle_event(&event);

                    // Render after each event
                    renderer.render(&app)?;

                    // Check if processing is done
                    if is_done_or_error {
                        // Render final state
                        renderer.render(&app)?;
                        break;
                    }
                } else {
                    // Channel closed, we're done
                    break;
                }
            }
        }
    }

    if let Err(e) = agent_handle.await? {
        eprintln!("Agent task error: {}", e);
        return Err(e);
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

    match key_event.code {
        KeyCode::Char(c) => {
            app.input.push(c);
            app.update_command_filtering();
        }
        KeyCode::Backspace => {
            app.input.pop();
            app.update_command_filtering();
        }
        KeyCode::Enter => {
            let selected_name = if !app.filtered_commands.is_empty() {
                Some(app.filtered_commands[app.selected_command_index].name.clone())
            } else {
                None
            };

            if let Some(name) = selected_name {
                if app.input == name {
                    let input = app.submit_input();
                    app.update_command_filtering();
                    handle_submitted_input(input, app, settings, cwd, event_sender);
                } else {
                    app.input = name;
                    app.update_command_filtering();
                }
            } else {
                let input = app.submit_input();
                app.update_command_filtering();
                handle_submitted_input(input, app, settings, cwd, event_sender);
            }
        }
        KeyCode::Esc => {
            app.input.clear();
            app.update_command_filtering();
        }
        KeyCode::Up => {
            if !app.filtered_commands.is_empty() {
                if app.selected_command_index > 0 {
                    app.selected_command_index -= 1;
                } else {
                    app.selected_command_index = app.filtered_commands.len().saturating_sub(1);
                }
            } else {
                app.scroll_up();
            }
        }
        KeyCode::Down => {
            if !app.filtered_commands.is_empty() {
                 if app.selected_command_index < app.filtered_commands.len().saturating_sub(1) {
                     app.selected_command_index += 1;
                 } else {
                     app.selected_command_index = 0;
                 }
            } else {
                let (width, height) = terminal_size()?;
                scroll_down_once(app, width, height);
            }
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

fn spawn_agent_task(
    settings: &Settings,
    cwd: &Path,
    input: String,
    event_sender: &TuiEventSender,
    session_id: Option<String>,
    session_title: Option<String>,
) {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let sender = event_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = run_agent(settings, &cwd, input, sender.clone(), session_id, session_title).await {
            sender.send(TuiEvent::Error(e.to_string()));
        }
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
    session_id: Option<String>,
    session_title: Option<String>,
) -> anyhow::Result<()> {
    let loop_runner = create_agent_loop(settings, cwd, events, session_id, session_title)?;

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
    // For single prompt, we create a new session (or we could make it ephemeral if we wanted)
    // Using prompt as title
    let title = prompt.chars().take(50).collect::<String>();
    let loop_runner = create_agent_loop(settings, cwd, events, None, Some(title))?;

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
    session_id: Option<String>,
    session_title: Option<String>,
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
    // Use the new session store constructor
    let session = SessionStore::new(&settings.session.root, cwd, session_id.as_deref(), session_title)?;

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

use anyhow::Context;

fn handle_submitted_input(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if input.starts_with('/') {
        handle_slash_command(input, app, settings, cwd);
    } else if app.is_picking_session {
        if let Err(e) = handle_session_selection(input, app, settings, cwd) {
             app.messages.push(tui::ChatMessage::Assistant(e.to_string()));
             *app.needs_rebuild.borrow_mut() = true;
        }
        app.set_processing(false);
    } else {
        handle_chat_message(input, app, settings, cwd, event_sender);
    }
}

fn handle_slash_command(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
) {
    match input.as_str() {
        "/quit" => {
            app.should_quit = true;
        }
        "/resume" => {
            let sessions = SessionStore::list(&settings.session.root, cwd).unwrap_or_default();
            if sessions.is_empty() {
                app.messages.push(tui::ChatMessage::Assistant("No previous sessions found.".to_string()));
                *app.needs_rebuild.borrow_mut() = true;
                app.set_processing(false);
            } else {
                app.available_sessions = sessions;
                app.is_picking_session = true;
                
                let mut msg = String::from("Available sessions:\n");
                for (i, s) in app.available_sessions.iter().enumerate() {
                    msg.push_str(&format!("[{}] {}\n", i + 1, s.title));
                }
                msg.push_str("\nEnter number to resume:");
                app.messages.push(tui::ChatMessage::Assistant(msg));
                *app.needs_rebuild.borrow_mut() = true;
                app.set_processing(false);
            }
        }
        _ => {
            app.messages.push(tui::ChatMessage::Assistant(format!("Unknown command: {}", input)));
            *app.needs_rebuild.borrow_mut() = true;
            app.set_processing(false);
        }
    }
}

fn handle_session_selection(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
) -> anyhow::Result<()> {
    let idx = input.trim().parse::<usize>().context("Invalid number.")?;
    
    if idx == 0 || idx > app.available_sessions.len() {
        anyhow::bail!("Invalid session index.");
    }

    let session = &app.available_sessions[idx - 1];
    app.session_id = Some(session.id.clone());
    app.session_name = session.title.clone();
    app.is_picking_session = false;
    
    let store = SessionStore::new(&settings.session.root, cwd, Some(&session.id), None)
        .context("Failed to load session store")?;

    let events = store.replay_events().context("Failed to replay session")?;

    app.messages.clear();
    for event in events {
        match event {
            SessionEvent::Message { message, .. } => {
                let chat_msg = match message.role {
                    crate::core::Role::User => tui::ChatMessage::User(message.content),
                    crate::core::Role::Assistant => tui::ChatMessage::Assistant(message.content),
                    _ => continue,
                };
                app.messages.push(chat_msg);
            }
            SessionEvent::ToolCall { call } => {
                app.messages.push(tui::ChatMessage::ToolCall {
                    name: call.name,
                    args: call.arguments.to_string(),
                    output: None,
                    is_error: None,
                });
            }
            SessionEvent::ToolResult { id: _, is_error, output } => {
                for msg in app.messages.iter_mut().rev() {
                    if let tui::ChatMessage::ToolCall { output: out, is_error: err, .. } = msg {
                        if out.is_none() {
                            *out = Some(output.clone());
                            *err = Some(is_error);
                            break;
                        }
                    }
                }
            }
            SessionEvent::Thinking { content, .. } => {
                app.messages.push(tui::ChatMessage::Thinking(content));
            }
            _ => {}
        }
    }
    app.messages.push(tui::ChatMessage::Assistant(format!("Resumed session: {}", session.title)));
    *app.needs_rebuild.borrow_mut() = true;

    Ok(())
}

fn handle_chat_message(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if !input.is_empty() {
        let session_id = app.session_id.clone();
        let session_title = if session_id.is_none() {
            Some(input.chars().take(30).collect::<String>())
        } else {
            None
        };
        
        let current_session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if app.session_id.is_none() {
            app.session_id = Some(current_session_id.clone());
            if let Some(t) = &session_title {
                app.session_name = t.clone();
            }
        }

        spawn_agent_task(settings, cwd, input, event_sender, Some(current_session_id), session_title);
    } else {
        app.set_processing(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::config::settings::{AgentSettings, ProviderSettings, SessionSettings};

    fn create_dummy_settings(root: &Path) -> Settings {
        Settings {
            agent: AgentSettings {
                max_steps: 10,
                token_budget: 1000,
            },
            provider: ProviderSettings {
                base_url: "http://localhost:1234".to_string(),
                model: "test-model".to_string(),
                api_key_env: "TEST_KEY".to_string(),
            },
            session: SessionSettings {
                root: root.to_path_buf(),
            },
            tools: Default::default(),
            permission: Default::default(),
        }
    }

    #[test]
    fn test_resume_clears_processing() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();

        // Create a dummy session
        let session_id = "test-session-id";
        let _store = SessionStore::new(
            &settings.session.root,
            cwd,
            Some(session_id),
            Some("Test Session".to_string())
        ).unwrap();
        
        // Setup ChatApp
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        // Simulate typing "/resume"
        app.input = "/resume".to_string();
        // verify submit_input sets processing to true
        let input = app.submit_input();
        assert!(app.is_processing);

        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);
        
        // processing should be false after listing sessions
        assert!(!app.is_processing, "Processing should be cleared after /resume lists sessions");
        assert!(app.is_picking_session);

        // Simulate picking session "1"
        app.input = "1".to_string();
        let input = app.submit_input();
        assert!(app.is_processing);

        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        // processing should be false after picking session
        assert!(!app.is_processing, "Processing should be cleared after picking session");
        assert!(!app.is_picking_session);
        // The session ID might not match if listing logic uses UUIDs or if index logic is tricky.
        // But we provided title "Test Session", so it should be listed.
        // Let's verify session_id is SOME value, and name is correct.
        assert_eq!(app.session_name, "Test Session");
    }
}
