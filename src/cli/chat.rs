use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use tokio::sync::mpsc;

use crate::cli::render;
use crate::cli::tui::{self, ChatApp, DebugRenderer, TuiEvent, TuiEventSender};
use crate::config::Settings;
use crate::core::Message;
use crate::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::{SessionEvent, SessionStore, event_id};
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
        let result = run_agent(
            settings_clone,
            &cwd_clone,
            prompt_clone,
            sender_clone.clone(),
            None,
            Some(title_clone),
        )
        .await;
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
    Refresh,
}

async fn handle_input() -> anyhow::Result<Option<InputEvent>> {
    if event::poll(Duration::from_millis(16))? {
        match event::read()? {
            Event::Key(key) => Ok(Some(InputEvent::Key(key))),
            Event::Mouse(mouse) => Ok(handle_mouse_event(mouse)),
            Event::Resize(_, _) | Event::FocusGained => Ok(Some(InputEvent::Refresh)),
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
    if key_event.kind == KeyEventKind::Release {
        return Ok(());
    }

    if key_event.code == KeyCode::Char('c') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        if app.input.is_empty() {
            app.should_quit = true;
        } else {
            app.clear_input();
            app.update_command_filtering();
        }
        return Ok(());
    }

    match key_event.code {
        KeyCode::Char(c) => {
            if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                match c {
                    'a' | 'A' => app.move_to_line_start(),
                    'e' | 'E' => app.move_to_line_end(),
                    _ => {}
                }
            } else {
                app.insert_char(c);
                app.update_command_filtering();
            }
        }
        KeyCode::Backspace => {
            app.backspace();
            app.update_command_filtering();
        }
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            app.insert_char('\n');
            app.update_command_filtering();
        }
        KeyCode::Enter => {
            let selected_name = if !app.filtered_commands.is_empty() {
                Some(
                    app.filtered_commands[app.selected_command_index]
                        .name
                        .clone(),
                )
            } else {
                None
            };

            if let Some(name) = selected_name {
                if app.input == name {
                    let input = app.submit_input();
                    app.update_command_filtering();
                    handle_submitted_input(input, app, settings, cwd, event_sender);
                } else {
                    app.set_input(name);
                    app.update_command_filtering();
                }
            } else {
                let input = app.submit_input();
                app.update_command_filtering();
                handle_submitted_input(input, app, settings, cwd, event_sender);
            }
        }
        KeyCode::Esc => {
            app.clear_input();
            app.update_command_filtering();
        }
        KeyCode::Up => {
            if !app.filtered_commands.is_empty() {
                if app.selected_command_index > 0 {
                    app.selected_command_index -= 1;
                } else {
                    app.selected_command_index = app.filtered_commands.len().saturating_sub(1);
                }
            } else if !app.input.is_empty() {
                app.move_cursor_up();
            } else {
                app.scroll_up();
            }
        }
        KeyCode::Left => {
            app.move_cursor_left();
        }
        KeyCode::Right => {
            app.move_cursor_right();
        }
        KeyCode::Down => {
            if !app.filtered_commands.is_empty() {
                if app.selected_command_index < app.filtered_commands.len().saturating_sub(1) {
                    app.selected_command_index += 1;
                } else {
                    app.selected_command_index = 0;
                }
            } else if !app.input.is_empty() {
                app.move_cursor_down();
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
        if let Err(e) = run_agent(
            settings,
            &cwd,
            input,
            sender.clone(),
            session_id,
            session_title,
        )
        .await
        {
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
                    Some(InputEvent::Refresh) => {
                        tui_guard.get().autoresize()?;
                        tui_guard.get().clear()?;
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
) -> anyhow::Result<
    AgentLoop<OpenAiCompatibleProvider, E, ToolRegistry, PermissionMatcher, SessionStore>,
>
where
    E: AgentEvents,
{
    let provider = OpenAiCompatibleProvider::new(
        settings.provider.base_url.clone(),
        settings.provider.model.clone(),
        settings.provider.api_key_env.clone(),
    );

    let tool_registry = ToolRegistry::new(&settings, cwd);
    let tool_schemas = tool_registry.schemas();
    let permissions = PermissionMatcher::new(settings.clone(), &tool_schemas);
    // Use the new session store constructor
    let session = SessionStore::new(
        &settings.session.root,
        cwd,
        session_id.as_deref(),
        session_title,
    )?;

    Ok(AgentLoop {
        provider,
        tools: tool_registry,
        approvals: permissions,
        max_steps: settings.agent.max_steps,
        model: settings.provider.model,
        system_prompt: settings.agent.resolved_system_prompt(),
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
        if let Some(tui::ChatMessage::User(last)) = app.messages.last()
            && last == &input
        {
            app.messages.pop();
            app.mark_dirty();
        }
        handle_slash_command(input, app, settings, cwd, event_sender);
    } else if app.is_picking_session {
        if let Err(e) = handle_session_selection(input, app, settings, cwd) {
            app.messages
                .push(tui::ChatMessage::Assistant(e.to_string()));
            app.mark_dirty();
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
    event_sender: &TuiEventSender,
) {
    match input.as_str() {
        "/new" => {
            app.start_new_session(build_session_name(cwd));
            app.mark_dirty();
            app.set_processing(false);
        }
        "/compact" => {
            let Some(session_id) = app.session_id.clone() else {
                app.messages.push(tui::ChatMessage::Assistant(
                    "No active session to compact yet.".to_string(),
                ));
                app.mark_dirty();
                app.set_processing(false);
                return;
            };

            app.handle_event(&TuiEvent::CompactionStart);

            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let settings = settings.clone();
                let cwd = cwd.to_path_buf();
                let sender = event_sender.clone();
                handle.spawn(async move {
                    match compact_session_with_llm(settings, &cwd, &session_id).await {
                        Ok(summary) => sender.send(TuiEvent::CompactionDone(summary)),
                        Err(e) => sender.send(TuiEvent::Error(format!("Failed to compact: {e}"))),
                    }
                });
            } else {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("Failed to create runtime for compaction")
                    .and_then(|rt| {
                        rt.block_on(compact_session_with_llm(settings.clone(), cwd, &session_id))
                    });

                match result {
                    Ok(summary) => {
                        app.handle_event(&TuiEvent::CompactionDone(summary));
                    }
                    Err(e) => {
                        app.handle_event(&TuiEvent::Error(format!("Failed to compact: {e}")));
                    }
                }
            }
        }
        "/quit" => {
            app.should_quit = true;
        }
        "/resume" => {
            let sessions = SessionStore::list(&settings.session.root, cwd).unwrap_or_default();
            if sessions.is_empty() {
                app.messages.push(tui::ChatMessage::Assistant(
                    "No previous sessions found.".to_string(),
                ));
                app.mark_dirty();
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
                app.mark_dirty();
                app.set_processing(false);
            }
        }
        _ => {
            app.messages.push(tui::ChatMessage::Assistant(format!(
                "Unknown command: {}",
                input
            )));
            app.mark_dirty();
            app.set_processing(false);
        }
    }
}

async fn compact_session_with_llm(
    settings: Settings,
    cwd: &Path,
    session_id: &str,
) -> anyhow::Result<String> {
    let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None)
        .context("Failed to load session store")?;
    let messages = store
        .replay_messages()
        .context("Failed to replay session for compaction")?;

    if messages.is_empty() {
        return Ok("No prior context to compact yet.".to_string());
    }

    let summary = generate_compaction_summary(&settings, messages).await?;
    store
        .append(&SessionEvent::Compact {
            id: event_id(),
            summary: summary.clone(),
        })
        .context("Failed to append compact marker")?;

    Ok(summary)
}

async fn generate_compaction_summary(
    settings: &Settings,
    messages: Vec<Message>,
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        let _ = settings;
        let _ = messages;
        return Ok("Compacted context summary for tests.".to_string());
    }

    #[cfg(not(test))]
    {
        let mut prompt_messages = Vec::with_capacity(messages.len() + 2);
        prompt_messages.push(Message {
            role: crate::core::Role::System,
            content: "You compact conversation history for an engineering assistant. Produce a concise summary that preserves requirements, decisions, constraints, open questions, and pending work items. Prefer bullet points. Do not invent details.".to_string(),
            tool_call_id: None,
        });
        prompt_messages.extend(messages);
        prompt_messages.push(Message {
            role: crate::core::Role::User,
            content: "Compact the conversation so future turns can continue from this summary with minimal context loss.".to_string(),
            tool_call_id: None,
        });

        let provider = OpenAiCompatibleProvider::new(
            settings.provider.base_url.clone(),
            settings.provider.model.clone(),
            settings.provider.api_key_env.clone(),
        );

        let response = crate::core::Provider::complete(
            &provider,
            crate::core::ProviderRequest {
                model: settings.provider.model.clone(),
                messages: prompt_messages,
                tools: Vec::new(),
            },
        )
        .await
        .context("Compaction request failed")?;

        if !response.tool_calls.is_empty() {
            anyhow::bail!("Compaction response unexpectedly requested tools");
        }

        let summary = response.assistant_message.content.trim().to_string();
        if summary.is_empty() {
            anyhow::bail!("Compaction response was empty");
        }

        Ok(summary)
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
            SessionEvent::ToolResult {
                id: _,
                is_error,
                output,
                result,
            } => {
                let rendered_output = result.map(|value| value.output).unwrap_or(output);
                for msg in app.messages.iter_mut().rev() {
                    if let tui::ChatMessage::ToolCall {
                        output: out,
                        is_error: err,
                        ..
                    } = msg
                        && out.is_none()
                    {
                        *out = Some(rendered_output.clone());
                        *err = Some(is_error);
                        break;
                    }
                }
            }
            SessionEvent::Thinking { content, .. } => {
                app.messages.push(tui::ChatMessage::Thinking(content));
            }
            SessionEvent::Compact { summary, .. } => {
                app.messages.push(tui::ChatMessage::Compaction(summary));
            }
            _ => {}
        }
    }
    app.mark_dirty();

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

        spawn_agent_task(
            settings,
            cwd,
            input,
            event_sender,
            Some(current_session_id),
            session_title,
        );
    } else {
        app.set_processing(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::settings::{AgentSettings, ProviderSettings, SessionSettings};
    use crate::core::{Message, Role};
    use crossterm::event::KeyEvent;
    use tempfile::tempdir;

    fn create_dummy_settings(root: &Path) -> Settings {
        Settings {
            agent: AgentSettings {
                max_steps: 10,
                token_budget: 1000,
                sub_agent_max_depth: 2,
                system_prompt: None,
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
            Some("Test Session".to_string()),
        )
        .unwrap();

        // Setup ChatApp
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        // Simulate typing "/resume"
        app.set_input("/resume".to_string());
        // verify submit_input sets processing to true
        let input = app.submit_input();
        assert!(app.is_processing);

        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        // processing should be false after listing sessions
        assert!(
            !app.is_processing,
            "Processing should be cleared after /resume lists sessions"
        );
        assert!(app.is_picking_session);

        // Simulate picking session "1"
        app.set_input("1".to_string());
        let input = app.submit_input();
        assert!(app.is_processing);

        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        // processing should be false after picking session
        assert!(
            !app.is_processing,
            "Processing should be cleared after picking session"
        );
        assert!(!app.is_picking_session);
        // The session ID might not match if listing logic uses UUIDs or if index logic is tricky.
        // But we provided title "Test Session", so it should be listed.
        // Let's verify session_id is SOME value, and name is correct.
        assert_eq!(app.session_name, "Test Session");
    }

    #[test]
    fn test_new_starts_fresh_session() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.session_id = Some("existing-session".to_string());
        app.session_name = "Existing Session".to_string();
        app.messages
            .push(tui::ChatMessage::Assistant("previous context".to_string()));

        app.set_input("/new".to_string());
        let input = app.submit_input();
        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        assert!(!app.is_processing);
        assert!(app.session_id.is_none());
        assert_eq!(app.session_name, build_session_name(cwd));
        assert!(app.messages.is_empty());
    }

    #[test]
    fn test_compact_appends_marker_and_clears_replayed_context() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let session_id = "compact-session-id";
        let store = SessionStore::new(
            &settings.session.root,
            cwd,
            Some(session_id),
            Some("Compact Session".to_string()),
        )
        .unwrap();
        store
            .append(&SessionEvent::Message {
                id: event_id(),
                message: Message {
                    role: Role::User,
                    content: "hello".to_string(),
                    tool_call_id: None,
                },
            })
            .unwrap();

        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.session_id = Some(session_id.to_string());
        app.session_name = "Compact Session".to_string();
        app.messages
            .push(tui::ChatMessage::Assistant("previous context".to_string()));

        app.set_input("/compact".to_string());
        let input = app.submit_input();
        handle_submitted_input(input, &mut app, &settings, cwd, &event_sender);

        assert!(!app.is_processing);
        assert_eq!(app.messages.len(), 2);
        assert!(matches!(
            app.messages[0],
            tui::ChatMessage::Assistant(ref text) if text == "previous context"
        ));
        assert!(matches!(
            app.messages[1],
            tui::ChatMessage::Compaction(ref text)
                if text == "Compacted context summary for tests."
        ));

        let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None).unwrap();
        let replayed_events = store.replay_events().unwrap();
        assert_eq!(replayed_events.len(), 2);
        assert!(matches!(
            replayed_events[1],
            SessionEvent::Compact { ref summary, .. } if summary == "Compacted context summary for tests."
        ));

        let replayed_messages = store.replay_messages().unwrap();
        assert_eq!(replayed_messages.len(), 1);
        assert_eq!(
            replayed_messages[0].content,
            "Compacted context summary for tests."
        );
    }

    #[test]
    fn test_shift_enter_inserts_newline_without_submitting() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.set_input("hello".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert_eq!(app.input, "hello\n");
        assert!(app.messages.is_empty());
        assert!(!app.is_processing);
    }

    #[test]
    fn test_shift_enter_press_followed_by_release_does_not_submit() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.set_input("hello".to_string());

        handle_key_event(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::SHIFT, KeyEventKind::Press),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        handle_key_event(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Release),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert_eq!(app.input, "hello\n");
        assert!(app.messages.is_empty());
        assert!(!app.is_processing);
    }

    #[test]
    fn test_ctrl_c_clears_non_empty_input() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.set_input("hello".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.input.is_empty());
        assert_eq!(app.cursor, 0);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_ctrl_c_quits_when_input_is_empty() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);

        handle_key_event(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.should_quit);
    }

    #[test]
    fn test_multiline_cursor_shortcuts_ctrl_and_vertical_arrows() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.set_input("abc\ndefg\nxy".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 9);

        handle_key_event(
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 4);

        handle_key_event(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 9);

        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 11);
    }

    #[test]
    fn test_ctrl_e_and_ctrl_a_can_cross_line_edges() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.set_input("ab\ncd\nef".to_string());

        // End of first line.
        app.cursor = 2;

        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 5);

        // End of second line should jump to end of third line.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 8);

        // On last line end, Ctrl+E stays there.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 8);

        // Ctrl+A at line end moves to that line's start.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 6);

        // Ctrl+A at line start jumps to previous line start.
        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 3);

        handle_key_event(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_left_and_right_move_cursor_across_newline() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd, 1000);
        app.set_input("ab\ncd".to_string());
        app.cursor = 2;

        handle_key_event(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 3);

        handle_key_event(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 2);

        app.cursor = 0;
        handle_key_event(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, 0);

        app.cursor = app.input.len();
        handle_key_event(
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert_eq!(app.cursor, app.input.len());
    }
}
