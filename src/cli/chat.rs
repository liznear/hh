use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use tokio::sync::mpsc;

use crate::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::cli::render;
use crate::cli::tui::{self, ChatApp, TuiEvent, TuiEventSender};
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
