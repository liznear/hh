use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use tokio::sync::mpsc;

use crate::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::cli::render;
use crate::cli::tui::{self, ChatApp, DebugRenderer, TuiEvent, TuiEventSender};
use crate::config::Settings;
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::SessionStore;
use crate::tool::registry::ToolRegistry;

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create app state and event channel
    let mut app = ChatApp::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    // Main loop
    loop {
        // Draw UI
        tui_guard.get().draw(|f| tui::render_app(f, &app))?;

        // Handle events with tokio::select!
        tokio::select! {
            // Handle terminal input events
            input_result = handle_input() => {
                match input_result? {
                    Some(key_event) => {
                        // Check for Ctrl+C first
                        if key_event.code == KeyCode::Char('c')
                            && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            app.should_quit = true;
                        } else {
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
                                    } else if input == ":thinking" {
                                        app.toggle_thinking();
                                    } else if !input.is_empty() {
                                        // Run agent in background
                                        let settings = settings.clone();
                                        let cwd = cwd.to_path_buf();
                                        let sender = event_sender.clone();
                                        tokio::spawn(async move {
                                            let _ = run_agent(settings, &cwd, input, sender).await;
                                        });
                                    }
                                }
                                KeyCode::Esc => {
                                    app.input.clear();
                                }
                                _ => {}
                            }
                        }
                    }
                    None => {
                        // No key event available, continue loop
                    }
                }
            }

            // Handle agent events
            event = event_rx.recv() => {
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
    let mut app = ChatApp::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    // Render initial state to debug file
    debug_renderer.render(&app)?;

    // Main loop
    loop {
        // Draw UI to terminal
        tui_guard.get().draw(|f| tui::render_app(f, &app))?;

        // Also render to debug file
        debug_renderer.render(&app)?;

        // Handle events with tokio::select!
        tokio::select! {
            // Handle terminal input events
            input_result = handle_input() => {
                match input_result? {
                    Some(key_event) => {
                        // Check for Ctrl+C first
                        if key_event.code == KeyCode::Char('c')
                            && key_event.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            app.should_quit = true;
                        } else {
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
                                    } else if input == ":thinking" {
                                        app.toggle_thinking();
                                    } else if !input.is_empty() {
                                        // Run agent in background
                                        let settings = settings.clone();
                                        let cwd = cwd.to_path_buf();
                                        let sender = event_sender.clone();
                                        tokio::spawn(async move {
                                            let _ = run_agent(settings, &cwd, input, sender).await;
                                        });
                                    }
                                }
                                KeyCode::Esc => {
                                    app.input.clear();
                                }
                                _ => {}
                            }
                        }
                    }
                    None => {
                        // No key event available, continue loop
                    }
                }
            }

            // Handle agent events
            event = event_rx.recv() => {
                if let Some(event) = event {
                    app.handle_event(&event);
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    eprintln!(
        "Debug: {} frames written to {}",
        debug_renderer.frame_count(),
        debug_dir.display()
    );

    Ok(())
}

/// Generate a debug directory path with timestamp
pub fn generate_debug_dir() -> PathBuf {
    let id = uuid::Uuid::new_v4();
    let short_id = &id.to_string()[..8];
    std::env::temp_dir().join(format!("hh-debug-{}", short_id))
}

/// Run chat in debug/headless mode - renders to files instead of terminal
pub async fn run_chat_debug(
    _settings: Settings,
    _cwd: &std::path::Path,
    output_dir: PathBuf,
) -> anyhow::Result<()> {
    // Create debug renderer
    let mut renderer = DebugRenderer::new(output_dir.clone())?;

    // Create app state and event channel
    let mut app = ChatApp::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let _event_sender = TuiEventSender::new(event_tx);

    // Render initial empty state
    renderer.render(&app)?;

    // For debug mode, we need a prompt from stdin or args
    // For now, just wait for events and render them
    // This is meant to be driven by piping input or running with a prompt

    println!("Debug mode: writing screen dumps to {}", output_dir.display());

    // Main loop - process events and render
    loop {
        // Handle agent events
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    app.handle_event(&event);

                    // Render after each event
                    renderer.render(&app)?;

                    // Check if processing is done
                    if matches!(event, TuiEvent::AssistantDone) {
                        // Wait a bit for any final rendering
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        // Render final state
                        renderer.render(&app)?;
                        break;
                    }
                } else {
                    // Channel closed, we're done
                    break;
                }
            }

            _ = tokio::time::sleep(Duration::from_secs(300)) => {
                // Timeout after 5 minutes of inactivity
                break;
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

/// Run chat in debug mode with a specific prompt
pub async fn run_chat_debug_with_prompt(
    settings: Settings,
    cwd: &std::path::Path,
    output_dir: PathBuf,
    prompt: String,
) -> anyhow::Result<()> {
    // Create debug renderer
    let mut renderer = DebugRenderer::new(output_dir.clone())?;

    // Create app state and event channel
    let mut app = ChatApp::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<TuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);

    // Submit the prompt
    app.messages.push(tui::ChatMessage::User(prompt.clone()));
    app.is_processing = true;

    // Render initial state with prompt
    renderer.render(&app)?;

    println!("Debug mode: writing screen dumps to {}", output_dir.display());

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
                // Agent finished (join handle resolved)
                if let Err(e) = result {
                    eprintln!("Agent task panicked: {}", e);
                }
                // Render final state
                renderer.render(&app)?;
                break;
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

async fn handle_input() -> anyhow::Result<Option<event::KeyEvent>> {
    if event::poll(Duration::from_millis(100))? {
        match event::read()? {
            Event::Key(key) => Ok(Some(key)),
            _ => Ok(None),
        }
    } else {
        // No event available, yield and continue
        tokio::time::sleep(Duration::from_millis(16)).await;
        Ok(None)
    }
}

async fn run_agent(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: String,
    events: impl AgentEvents + 'static,
) -> anyhow::Result<()> {
    let provider = OpenAiCompatibleProvider::new(
        settings.provider.base_url.clone(),
        settings.provider.model.clone(),
        settings.provider.api_key_env.clone(),
    );

    let tool_registry = ToolRegistry::new(&settings, cwd);
    let permissions = PermissionMatcher::new(settings.clone());
    let session = SessionStore::for_workspace(&settings.session.root, cwd)?;

    let loop_runner = AgentLoop {
        provider,
        tool_registry,
        permissions,
        max_steps: settings.agent.max_steps,
        model: settings.provider.model,
        session,
        events,
    };

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
    let provider = OpenAiCompatibleProvider::new(
        settings.provider.base_url.clone(),
        settings.provider.model.clone(),
        settings.provider.api_key_env.clone(),
    );

    let tool_registry = ToolRegistry::new(&settings, cwd);
    let permissions = PermissionMatcher::new(settings.clone());
    let session = SessionStore::for_workspace(&settings.session.root, cwd)?;

    let loop_runner = AgentLoop {
        provider,
        tool_registry,
        permissions,
        max_steps: settings.agent.max_steps,
        model: settings.provider.model,
        session,
        events,
    };

    loop_runner
        .run(prompt, |tool_name| {
            Ok(render::confirm(&format!(
                "Allow tool '{}' execution?",
                tool_name
            ))?)
        })
        .await
}
