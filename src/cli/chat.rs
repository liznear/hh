use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::cli::agent_init;
use crate::cli::render;
use crate::cli::tui::{
    self, ChatApp, ModelOptionView, ScopedTuiEvent, SubmittedInput, TuiEvent, TuiEventSender,
};
use crate::config::{Settings, upsert_local_permission_rule};
use crate::core::agent::subagent_manager::SubagentManager;
use crate::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use crate::core::{Message, Role};
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::SessionStore;
use crate::tool::registry::{ToolRegistry, ToolRegistryContext};
use crate::tool::task::TaskToolRuntimeContext;
use uuid::Uuid;

mod commands;
mod input;
mod session;
mod subagent;

use self::commands::handle_submitted_input;
use self::input::{
    InputEvent, apply_paste, handle_area_scroll, handle_input_batch, handle_key_event,
    handle_mouse_click, handle_mouse_drag, handle_mouse_release,
};
#[cfg(test)]
use self::session::handle_session_selection;
use self::session::{
    fallback_session_title, generate_session_title, spawn_session_title_generation_task,
};
use self::subagent::{
    current_subagent_manager, initialize_subagent_manager, map_subagent_node_event,
};

pub async fn run_chat(settings: Settings, cwd: &std::path::Path) -> anyhow::Result<()> {
    // Setup terminal
    let terminal = tui::setup_terminal()?;
    let mut tui_guard = tui::TuiGuard::new(terminal);

    // Create app state and event channel
    let mut app = ChatApp::new(build_session_name(cwd), cwd);
    app.configure_models(
        settings.selected_model_ref().to_string(),
        build_model_options(&settings),
    );

    // Initialize agents
    let (agent_views, selected_agent) = agent_init::initialize_agents(&settings)?;
    app.set_agents(agent_views, selected_agent);

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<ScopedTuiEvent>();
    let event_sender = TuiEventSender::new(event_tx);
    initialize_subagent_manager(settings.clone(), cwd.to_path_buf());

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
    .await?;

    Ok(())
}

const EVENT_DRAIN_MAX: usize = 128;
const STREAM_CHUNK_FLUSH_INTERVAL: Duration = Duration::from_millis(75);
const STREAM_CHUNK_FLUSH_BYTES: usize = 8192;

fn spawn_agent_task(
    settings: &Settings,
    cwd: &Path,
    input: Message,
    model_ref: String,
    event_sender: &TuiEventSender,
    subagent_manager: Arc<SubagentManager>,
    run_options: AgentRunOptions,
) -> tokio::task::JoinHandle<()> {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let sender = event_sender.clone();
    tokio::spawn(async move {
        if let Err(e) = run_agent(
            settings,
            &cwd,
            input,
            model_ref,
            sender.clone(),
            subagent_manager,
            run_options,
        )
        .await
        {
            sender.send(TuiEvent::Error(e.to_string()));
        }
    })
}

async fn run_interactive_chat_loop(
    tui_guard: &mut tui::TuiGuard,
    app: &mut ChatApp,
    runner: InteractiveChatRunner<'_>,
) -> anyhow::Result<()> {
    let mut render_tick = tokio::time::interval(Duration::from_millis(100));
    let mut stream_flush_tick = tokio::time::interval(STREAM_CHUNK_FLUSH_INTERVAL);
    let mut needs_redraw = true;
    let mut flush_stream_before_draw = false;
    let mut pending_assistant_delta = String::new();
    let mut pending_thinking = String::new();

    loop {
        if needs_redraw {
            if flush_stream_before_draw {
                flush_stream_chunks(app, &mut pending_thinking, &mut pending_assistant_delta);
                flush_stream_before_draw = false;
            }
            tui_guard.get().draw(|f| tui::render_app(f, app))?;
            needs_redraw = false;
        }

        tokio::select! {
            input_result = handle_input_batch() => {
                let input_events = input_result?;
                let mut handled_any_input = false;
                for input_event in input_events {
                    handled_any_input = true;
                    match input_event {
                    InputEvent::Key(key_event) => {
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
                        handle_area_scroll(app, terminal_rect, x, y, 3, 0);
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
                            app,
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
                        handle_mouse_click(app, x, y, tui_guard.get());
                    }
                    InputEvent::MouseDrag { x, y } => {
                        handle_mouse_drag(app, x, y, tui_guard.get());
                    }
                    InputEvent::MouseRelease { x, y } => {
                        handle_mouse_release(app, x, y, tui_guard.get());
                    }
                    }
                }
                if handled_any_input {
                    needs_redraw = true;
                }
            }
            event = runner.event_rx.recv() => {
                if let Some(event) = event
                    && event.session_epoch == app.session_epoch()
                    && event.run_epoch == app.run_epoch()
                {
                    let mut handled_non_stream_event = false;
                    merge_or_handle_event(
                        app,
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
                                next_event.event,
                                &mut pending_thinking,
                                &mut pending_assistant_delta,
                                &mut handled_non_stream_event,
                            );
                        }
                    }

                    if handled_non_stream_event {
                        needs_redraw = true;
                    }
                    if pending_assistant_delta.len() >= STREAM_CHUNK_FLUSH_BYTES
                        || pending_thinking.len() >= STREAM_CHUNK_FLUSH_BYTES
                    {
                        flush_stream_before_draw = true;
                        needs_redraw = true;
                    }
                }
            }
            _ = stream_flush_tick.tick() => {
                if !pending_assistant_delta.is_empty() || !pending_thinking.is_empty() {
                    flush_stream_before_draw = true;
                    needs_redraw = true;
                }
            }
            _ = render_tick.tick() => {
                if app.on_periodic_tick() {
                    needs_redraw = true;
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn merge_or_handle_event(
    app: &mut ChatApp,
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

struct InteractiveChatRunner<'a> {
    settings: &'a Settings,
    cwd: &'a Path,
    event_sender: &'a TuiEventSender,
    event_rx: &'a mut mpsc::UnboundedReceiver<ScopedTuiEvent>,
    scroll_down_lines: usize,
}

#[derive(Clone)]
struct AgentRunOptions {
    session_id: Option<String>,
    session_title: Option<String>,
    allow_questions: bool,
}

struct AgentLoopOptions {
    subagent_manager: Option<Arc<SubagentManager>>,
    parent_task_id: Option<String>,
    depth: usize,
    session_id: Option<String>,
    session_title: Option<String>,
    session_parent_id: Option<String>,
}

fn build_session_name(cwd: &std::path::Path) -> String {
    let _ = cwd;
    "New Session".to_string()
}

fn build_model_options(settings: &Settings) -> Vec<ModelOptionView> {
    settings
        .model_refs()
        .into_iter()
        .filter_map(|model_ref| {
            settings
                .resolve_model_ref(&model_ref)
                .map(|resolved| ModelOptionView {
                    full_id: model_ref,
                    provider_name: if resolved.provider.display_name.trim().is_empty() {
                        resolved.provider_id.clone()
                    } else {
                        resolved.provider.display_name.clone()
                    },
                    model_name: if resolved.model.display_name.trim().is_empty() {
                        resolved.model_id.clone()
                    } else {
                        resolved.model.display_name.clone()
                    },
                    modality: format!(
                        "{} -> {}",
                        format_modalities(&resolved.model.modalities.input),
                        format_modalities(&resolved.model.modalities.output)
                    ),
                    max_context_size: resolved.model.limits.context,
                })
        })
        .collect()
}

fn format_modalities(modalities: &[crate::config::settings::ModelModalityType]) -> String {
    modalities
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

async fn run_agent(
    settings: Settings,
    cwd: &std::path::Path,
    prompt: Message,
    model_ref: String,
    events: TuiEventSender,
    subagent_manager: Arc<SubagentManager>,
    options: AgentRunOptions,
) -> anyhow::Result<()> {
    validate_image_input_model_support(&settings, &model_ref, &prompt)?;

    let event_sender = events.clone();
    let question_event_sender = event_sender.clone();
    let approval_event_sender = event_sender.clone();
    let allow_questions = options.allow_questions;
    let parent_session_id = options.session_id.clone();
    let loop_runner = create_agent_loop(
        settings,
        cwd,
        &model_ref,
        events,
        AgentLoopOptions {
            subagent_manager: Some(Arc::clone(&subagent_manager)),
            parent_task_id: None,
            depth: 0,
            session_id: options.session_id,
            session_title: options.session_title,
            session_parent_id: None,
        },
    )?;
    loop_runner
        .run_with_question_tool(
            prompt,
            move |request| {
                let event_sender = approval_event_sender.clone();
                let cwd = cwd.to_path_buf();
                async move {
                    let question = approval_request_to_question_prompt(&request);
                    let (tx, rx) = oneshot::channel();
                    event_sender.send(TuiEvent::QuestionPrompt {
                        questions: vec![question],
                        responder: std::sync::Arc::new(std::sync::Mutex::new(Some(tx))),
                    });

                    let answers = rx.await.unwrap_or_else(|_| {
                        Err(anyhow::anyhow!("approval prompt was cancelled"))
                    })?;

                    let choice = parse_approval_choice(&request, &answers)
                        .unwrap_or(crate::core::ApprovalChoice::Deny);
                    persist_approval_choice_if_needed(&cwd, &request, choice)?;
                    Ok(choice)
                }
            },
            move |questions| {
                let event_sender = question_event_sender.clone();
                async move {
                    if !allow_questions {
                        anyhow::bail!("question tool is not available in this mode")
                    }
                    let (tx, rx) = oneshot::channel();
                    event_sender.send(TuiEvent::QuestionPrompt {
                        questions,
                        responder: std::sync::Arc::new(std::sync::Mutex::new(Some(tx))),
                    });
                    rx.await
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("question prompt was cancelled")))
                }
            },
        )
        .await?;

    if let Some(parent_session_id) = parent_session_id.as_deref() {
        loop {
            let nodes = subagent_manager.list_for_parent(parent_session_id).await;
            event_sender.send(TuiEvent::SubagentsChanged(
                nodes.iter().map(map_subagent_node_event).collect(),
            ));

            if nodes.iter().all(|node| node.status.is_terminal()) {
                break;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    Ok(())
}

fn validate_image_input_model_support(
    settings: &Settings,
    model_ref: &str,
    prompt: &Message,
) -> anyhow::Result<()> {
    if prompt.attachments.is_empty() {
        return Ok(());
    }

    let selected = settings
        .resolve_model_ref(model_ref)
        .with_context(|| format!("unknown model reference: {model_ref}"))?;
    let supports_image_input = selected
        .model
        .modalities
        .input
        .contains(&crate::config::settings::ModelModalityType::Image);

    if supports_image_input {
        return Ok(());
    }

    anyhow::bail!(
        "Model `{model_ref}` does not support image input (input modalities: {}).",
        format_modalities(&selected.model.modalities.input)
    )
}

fn approval_request_to_question_prompt(
    request: &crate::core::ApprovalRequest,
) -> crate::core::QuestionPrompt {
    let is_bash = request
        .action
        .get("approval_kind")
        .and_then(|value| value.as_str())
        == Some("bash");

    let permission_rule = request
        .action
        .get("permission_rule")
        .and_then(|value| value.as_str());
    let allow_desc = permission_rule
        .map(|rule| format!("Persist allow rule `{rule}` in .hh/config.local.json."))
        .unwrap_or_else(|| "Persist an allow rule in .hh/config.local.json.".to_string());
    let deny_desc = permission_rule
        .map(|rule| format!("Reject and persist deny rule `{rule}` in .hh/config.local.json."))
        .unwrap_or_else(|| "Reject and persist a deny rule in .hh/config.local.json.".to_string());

    let options = if is_bash {
        vec![
            crate::core::QuestionOption {
                label: "Allow Once".to_string(),
                description: "Approve this command a single time.".to_string(),
            },
            crate::core::QuestionOption {
                label: "Always Allow".to_string(),
                description: allow_desc,
            },
            crate::core::QuestionOption {
                label: "Deny".to_string(),
                description: deny_desc,
            },
        ]
    } else {
        vec![
            crate::core::QuestionOption {
                label: "Allow Once".to_string(),
                description: "Approve this action a single time.".to_string(),
            },
            crate::core::QuestionOption {
                label: "Always Allow in this Session".to_string(),
                description: "Remember this approval for the current session.".to_string(),
            },
            crate::core::QuestionOption {
                label: "Deny".to_string(),
                description: "Reject the action.".to_string(),
            },
        ]
    };

    crate::core::QuestionPrompt {
        question: request.body.clone(),
        header: request.title.clone(),
        options,
        multiple: false,
        custom: false,
    }
}

// Approval option labels are protocol between question prompt rendering and parsing.
// Keep prompt labels and parse_approval_choice mapping in lock-step.
fn parse_approval_choice(
    request: &crate::core::ApprovalRequest,
    answers: &crate::core::QuestionAnswers,
) -> Option<crate::core::ApprovalChoice> {
    let is_bash = request
        .action
        .get("approval_kind")
        .and_then(|value| value.as_str())
        == Some("bash");
    let label = answers.first()?.first()?.as_str();
    if is_bash {
        match label {
            "Allow Once" => Some(crate::core::ApprovalChoice::AllowOnce),
            "Always Allow" => Some(crate::core::ApprovalChoice::AllowAlways),
            "Deny" => Some(crate::core::ApprovalChoice::Deny),
            _ => None,
        }
    } else {
        match label {
            "Allow Once" => Some(crate::core::ApprovalChoice::AllowOnce),
            "Always Allow in this Session" => Some(crate::core::ApprovalChoice::AllowSession),
            "Deny" => Some(crate::core::ApprovalChoice::Deny),
            _ => None,
        }
    }
}

// Only bash approvals persist to local permission rules.
// Non-bash approvals remain per-request/per-session decisions.
fn persist_approval_choice_if_needed(
    cwd: &Path,
    request: &crate::core::ApprovalRequest,
    choice: crate::core::ApprovalChoice,
) -> anyhow::Result<()> {
    let is_bash = request
        .action
        .get("approval_kind")
        .and_then(|value| value.as_str())
        == Some("bash");
    if !is_bash {
        return Ok(());
    }

    let Some(rule) = request
        .action
        .get("permission_rule")
        .and_then(|value| value.as_str())
    else {
        return Ok(());
    };

    match choice {
        crate::core::ApprovalChoice::AllowAlways => {
            upsert_local_permission_rule(cwd, "allow", rule)
        }
        crate::core::ApprovalChoice::Deny => upsert_local_permission_rule(cwd, "deny", rule),
        _ => Ok(()),
    }
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
    let default_model_ref = settings.selected_model_ref().to_string();
    let session_id = Uuid::new_v4().to_string();
    let fallback_title = fallback_session_title(&prompt);

    {
        let settings = settings.clone();
        let cwd = cwd.to_path_buf();
        let session_id = session_id.clone();
        let model_ref = default_model_ref.clone();
        let prompt = prompt.clone();
        tokio::spawn(async move {
            let generated = match generate_session_title(&settings, &model_ref, &prompt).await {
                Ok(title) => title,
                Err(_) => return,
            };

            let store =
                match SessionStore::new(&settings.session.root, &cwd, Some(&session_id), None) {
                    Ok(store) => store,
                    Err(_) => return,
                };

            let _ = store.update_title(generated);
        });
    }

    let loop_runner = create_agent_loop(
        settings.clone(),
        cwd,
        &default_model_ref,
        events,
        AgentLoopOptions {
            subagent_manager: Some(current_subagent_manager(&settings, cwd)),
            parent_task_id: None,
            depth: 0,
            session_id: Some(session_id),
            session_title: Some(fallback_title),
            session_parent_id: None,
        },
    )?;

    loop_runner
        .run_with_question_tool(
            Message {
                role: Role::User,
                content: prompt,
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |request| {
                let cwd = cwd.to_path_buf();
                async move {
                    let question = approval_request_to_question_prompt(&request);
                    let answers = render::ask_questions(&[question])?;
                    let choice = parse_approval_choice(&request, &answers)
                        .unwrap_or(crate::core::ApprovalChoice::Deny);
                    persist_approval_choice_if_needed(&cwd, &request, choice)?;
                    Ok::<crate::core::ApprovalChoice, anyhow::Error>(choice)
                }
            },
            |questions| async move { Ok(render::ask_questions(&questions)?) },
        )
        .await
}

fn create_agent_loop<E>(
    settings: Settings,
    cwd: &std::path::Path,
    model_ref: &str,
    events: E,
    options: AgentLoopOptions,
) -> anyhow::Result<
    AgentLoop<OpenAiCompatibleProvider, E, ToolRegistry, PermissionMatcher, SessionStore>,
>
where
    E: AgentEvents,
{
    let AgentLoopOptions {
        subagent_manager,
        parent_task_id,
        depth,
        session_id,
        session_title,
        session_parent_id,
    } = options;

    let selected = settings
        .resolve_model_ref(model_ref)
        .with_context(|| format!("unknown model reference: {model_ref}"))?;
    let provider = OpenAiCompatibleProvider::new(
        selected.provider.base_url.clone(),
        selected.model.id.clone(),
        selected.provider.api_key_env.clone(),
    );

    let session = match session_parent_id {
        Some(parent_session_id) => SessionStore::new_with_parent(
            &settings.session.root,
            cwd,
            session_id.as_deref(),
            session_title,
            Some(parent_session_id),
        )?,
        None => SessionStore::new(
            &settings.session.root,
            cwd,
            session_id.as_deref(),
            session_title,
        )?,
    };

    let tool_context = if let Some(manager) = subagent_manager {
        ToolRegistryContext {
            task: Some(TaskToolRuntimeContext {
                manager,
                settings: settings.clone(),
                workspace_root: cwd.to_path_buf(),
                parent_session_id: session.id.clone(),
                parent_task_id,
                depth,
            }),
        }
    } else {
        ToolRegistryContext::default()
    };

    let tool_registry = ToolRegistry::new_with_context(&settings, cwd, tool_context);
    let tool_schemas = tool_registry.schemas();
    let permissions = PermissionMatcher::new(settings.clone(), &tool_schemas, cwd);

    Ok(AgentLoop {
        provider,
        tools: tool_registry,
        approvals: permissions,
        max_steps: settings.agent.max_steps,
        model: selected.model.id.clone(),
        system_prompt: settings.agent.resolved_system_prompt(),
        session,
        events,
    })
}

fn handle_chat_message(
    input: SubmittedInput,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if !input.text.is_empty() || !input.attachments.is_empty() {
        // Ensure any run-epoch bump from replacing an existing task happens
        // before we scope events for the new run.
        app.cancel_agent_task();

        let scoped_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
        let session_id = app.session_id.clone();
        let session_title = if session_id.is_none() {
            Some(fallback_session_title(&input.text))
        } else {
            None
        };

        let current_session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if app.session_id.is_none() {
            app.session_id = Some(current_session_id.clone());
            if let Some(t) = &session_title {
                app.session_name = t.clone();
            }
            if !input.text.trim().is_empty() {
                spawn_session_title_generation_task(
                    settings,
                    cwd,
                    current_session_id.clone(),
                    app.selected_model_ref().to_string(),
                    input.text.clone(),
                    &scoped_sender,
                );
            }
        }

        let message = Message {
            role: crate::core::Role::User,
            content: input.text,
            attachments: input.attachments,
            tool_call_id: None,
        };

        let subagent_manager = current_subagent_manager(settings, cwd);
        let handle = spawn_agent_task(
            settings,
            cwd,
            message,
            app.selected_model_ref().to_string(),
            &scoped_sender,
            subagent_manager,
            AgentRunOptions {
                session_id: Some(current_session_id),
                session_title,
                allow_questions: true,
            },
        );
        app.set_agent_task(handle);
    } else {
        app.set_processing(false);
    }
}

#[cfg(test)]
mod tests {
    use super::input::{handle_mouse_event, prepare_paste, scroll_up_steps};
    use super::*;
    use crate::config::settings::{
        AgentSettings, ModelLimits, ModelMetadata, ModelModalities, ModelModalityType,
        ModelSettings, ProviderConfig, SessionSettings,
    };
    use crate::core::{Message, Role};
    use crate::session::{SessionEvent, event_id};
    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
    };
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn create_dummy_settings(root: &Path) -> Settings {
        Settings {
            models: ModelSettings {
                default: "test/test-model".to_string(),
            },
            providers: BTreeMap::from([(
                "test".to_string(),
                ProviderConfig {
                    display_name: "Test Provider".to_string(),
                    base_url: "http://localhost:1234".to_string(),
                    api_key_env: "TEST_KEY".to_string(),
                    models: BTreeMap::from([(
                        "test-model".to_string(),
                        ModelMetadata {
                            id: "provider-test-model".to_string(),
                            display_name: "Test Model".to_string(),
                            modalities: ModelModalities {
                                input: vec![ModelModalityType::Text],
                                output: vec![ModelModalityType::Text],
                            },
                            limits: ModelLimits {
                                context: 64_000,
                                output: 8_000,
                            },
                        },
                    )]),
                },
            )]),
            agent: AgentSettings {
                max_steps: 10,
                sub_agent_max_depth: 2,
                parallel_subagents: false,
                max_parallel_subagents: 2,
                system_prompt: None,
            },
            session: SessionSettings {
                root: root.to_path_buf(),
            },
            tools: Default::default(),
            permissions: Default::default(),
            selected_agent: None,
            agents: BTreeMap::new(),
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
        let mut app = ChatApp::new("Session".to_string(), cwd);
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
    fn test_session_selection_restores_todos_from_todo_write_and_replaces_stale_items() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();

        let session_id = "todo-session-id";
        let store = SessionStore::new(
            &settings.session.root,
            cwd,
            Some(session_id),
            Some("Todo Session".to_string()),
        )
        .unwrap();

        store
            .append(&SessionEvent::ToolCall {
                call: crate::core::ToolCall {
                    id: "call-1".to_string(),
                    name: "todo_write".to_string(),
                    arguments: serde_json::json!({"todos": []}),
                },
            })
            .unwrap();
        store
            .append(&SessionEvent::ToolResult {
                id: "call-1".to_string(),
                is_error: false,
                output: "".to_string(),
                result: Some(crate::tool::ToolResult::ok_json_typed(
                    "todo list updated",
                    "application/vnd.hh.todo+json",
                    serde_json::json!({
                        "todos": [
                            {"content": "Resume pending", "status": "pending", "priority": "medium"},
                            {"content": "Resume done", "status": "completed", "priority": "high"}
                        ],
                        "counts": {"total": 2, "pending": 1, "in_progress": 0, "completed": 1, "cancelled": 0}
                    }),
                )),
            })
            .unwrap();

        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.handle_event(&TuiEvent::ToolStart {
            name: "todo_write".to_string(),
            args: serde_json::json!({"todos": []}),
        });
        app.handle_event(&TuiEvent::ToolEnd {
            name: "todo_write".to_string(),
            result: crate::tool::ToolResult::ok_json_typed(
                "todo list updated",
                "application/vnd.hh.todo+json",
                serde_json::json!({
                    "todos": [
                        {"content": "Stale item", "status": "pending", "priority": "low"}
                    ],
                    "counts": {"total": 1, "pending": 1, "in_progress": 0, "completed": 0, "cancelled": 0}
                }),
            ),
        });

        app.available_sessions = vec![crate::session::SessionMetadata {
            id: session_id.to_string(),
            title: "Todo Session".to_string(),
            created_at: 0,
            last_updated_at: 0,
            parent_session_id: None,
        }];
        app.is_picking_session = true;

        handle_session_selection("1".to_string(), &mut app, &settings, cwd).unwrap();

        let backend = ratatui::backend::TestBackend::new(120, 25);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| tui::render_app(frame, &app))
            .expect("draw app");
        let full_text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(full_text.contains("TODO"));
        assert!(full_text.contains("1 / 2 done"));
        assert!(full_text.contains("[ ] Resume pending"));
        assert!(full_text.contains("[x] Resume done"));
        assert!(!full_text.contains("Stale item"));
    }

    #[test]
    fn test_new_starts_fresh_session() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let mut app = ChatApp::new("Session".to_string(), cwd);
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
    fn test_new_session_ignores_stale_scoped_events() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);

        let old_scope_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
        app.start_new_session("New Session".to_string());

        old_scope_sender.send(TuiEvent::AssistantDelta("stale".to_string()));
        let stale_event = rx.blocking_recv().unwrap();
        if stale_event.session_epoch == app.session_epoch()
            && stale_event.run_epoch == app.run_epoch()
        {
            app.handle_event(&stale_event.event);
        }
        assert!(app.messages.is_empty());

        let current_scope_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
        current_scope_sender.send(TuiEvent::AssistantDelta("fresh".to_string()));
        let fresh_event = rx.blocking_recv().unwrap();
        if fresh_event.session_epoch == app.session_epoch()
            && fresh_event.run_epoch == app.run_epoch()
        {
            app.handle_event(&fresh_event.event);
        }

        assert!(matches!(
            app.messages.first(),
            Some(tui::ChatMessage::Assistant(text)) if text == "fresh"
        ));
    }

    #[test]
    fn test_set_agent_task_without_existing_task_keeps_run_epoch_and_allows_events() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        app.set_processing(true);

        let initial_run_epoch = app.run_epoch();
        let scoped_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        #[allow(clippy::async_yields_async)]
        let handle = runtime.block_on(async { tokio::spawn(async {}) });
        app.set_agent_task(handle);

        assert_eq!(app.run_epoch(), initial_run_epoch);

        scoped_sender.send(TuiEvent::AssistantDone);
        let event = rx.blocking_recv().expect("event");
        if event.session_epoch == app.session_epoch() && event.run_epoch == app.run_epoch() {
            app.handle_event(&event.event);
        }

        assert!(!app.is_processing);
        app.cancel_agent_task();
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
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
            })
            .unwrap();

        let mut app = ChatApp::new("Session".to_string(), cwd);
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
    fn test_esc_requires_two_presses_to_interrupt_processing() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_processing(true);

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.is_processing);
        assert!(app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc again to interrupt");

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(!app.is_processing);
        assert!(!app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc interrupt");
    }

    #[test]
    fn test_non_esc_key_clears_pending_interrupt_confirmation() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_processing(true);

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        assert!(app.should_interrupt_on_esc());

        handle_key_event(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.is_processing);
        assert!(!app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc interrupt");

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(app.is_processing);
        assert!(app.should_interrupt_on_esc());
        assert_eq!(app.processing_interrupt_hint(), "esc again to interrupt");
    }

    #[test]
    fn test_cancelled_run_ignores_queued_events_from_previous_run_epoch() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_processing(true);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        #[allow(clippy::async_yields_async)]
        let handle = runtime.block_on(async {
            tokio::spawn(async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            })
        });
        app.set_agent_task(handle);

        let old_scope_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());

        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();
        handle_key_event(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert!(!app.is_processing);

        old_scope_sender.send(TuiEvent::AssistantDelta("stale-stream".to_string()));
        let stale_event = rx.blocking_recv().unwrap();
        if stale_event.session_epoch == app.session_epoch()
            && stale_event.run_epoch == app.run_epoch()
        {
            app.handle_event(&stale_event.event);
        }

        assert!(!app.messages.iter().any(
            |message| matches!(message, tui::ChatMessage::Assistant(text) if text.contains("stale-stream"))
        ));

        app.cancel_agent_task();
    }

    #[test]
    fn test_replacing_finished_task_scopes_events_to_new_run_epoch() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        #[allow(clippy::async_yields_async)]
        let first_handle = runtime.block_on(async { tokio::spawn(async {}) });
        app.set_agent_task(first_handle);
        app.set_processing(true);

        let submitted = SubmittedInput {
            text: "follow-up".to_string(),
            attachments: vec![crate::core::MessageAttachment::Image {
                media_type: "image/png".to_string(),
                data_base64: "aGVsbG8=".to_string(),
            }],
        };

        let _enter = runtime.enter();
        handle_chat_message(submitted, &mut app, &settings, cwd, &event_sender);
        drop(_enter);

        runtime.block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        });

        while let Ok(event) = rx.try_recv() {
            if event.session_epoch == app.session_epoch() && event.run_epoch == app.run_epoch() {
                app.handle_event(&event.event);
            }
        }

        assert!(
            app.messages
                .iter()
                .any(|message| matches!(message, tui::ChatMessage::Error(_))),
            "expected an error event from the newly started run"
        );
        assert!(
            !app.is_processing,
            "processing should stop when the run emits a scoped error event"
        );

        app.cancel_agent_task();
    }

    #[test]
    fn test_shift_enter_inserts_newline_without_submitting() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
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
        let mut app = ChatApp::new("Session".to_string(), cwd);
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
        let mut app = ChatApp::new("Session".to_string(), cwd);
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
        let mut app = ChatApp::new("Session".to_string(), cwd);

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
        let mut app = ChatApp::new("Session".to_string(), cwd);
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
        let mut app = ChatApp::new("Session".to_string(), cwd);
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
        let mut app = ChatApp::new("Session".to_string(), cwd);
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

    #[test]
    fn test_paste_transforms_single_image_path_into_attachment() {
        let temp_dir = tempdir().unwrap();
        let image_path = temp_dir.path().join("example.png");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();

        let prepared = prepare_paste(image_path.to_string_lossy().as_ref());
        assert_eq!(prepared.insert_text, "[pasted image: example.png]");
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn test_paste_transforms_shell_escaped_image_path_into_attachment() {
        let temp_dir = tempdir().unwrap();
        let image_path = temp_dir.path().join("my image.png");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();
        let escaped = image_path.to_string_lossy().replace(' ', "\\ ");

        let prepared = prepare_paste(&escaped);
        assert_eq!(prepared.insert_text, "[pasted image: my image.png]");
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn test_paste_transforms_file_url_image_path_into_attachment() {
        let temp_dir = tempdir().unwrap();
        let image_path = temp_dir.path().join("my image.jpeg");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();
        let file_url = format!(
            "file://{}",
            image_path.to_string_lossy().replace(' ', "%20")
        );

        let prepared = prepare_paste(&file_url);
        assert_eq!(prepared.insert_text, "[pasted image: my image.jpeg]");
        assert_eq!(prepared.attachments.len(), 1);
    }

    #[test]
    fn test_paste_leaves_plain_text_unchanged() {
        let prepared = prepare_paste("hello\nworld");
        assert_eq!(prepared.insert_text, "hello\nworld");
        assert!(prepared.attachments.is_empty());
    }

    #[test]
    fn test_apply_paste_inserts_content_at_cursor() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("abcXYZ".to_string());
        app.cursor = 3;

        let image_path = temp_dir.path().join("shot.png");
        std::fs::write(&image_path, [1u8, 2, 3, 4]).unwrap();

        apply_paste(&mut app, image_path.to_string_lossy().to_string());

        assert_eq!(app.input, "abc[pasted image: shot.png]XYZ");
        assert_eq!(app.pending_attachments.len(), 1);
    }

    #[test]
    fn test_cmd_v_does_not_insert_literal_v() {
        let temp_dir = tempdir().unwrap();
        let settings = create_dummy_settings(temp_dir.path());
        let cwd = temp_dir.path();
        let (tx, _rx) = mpsc::unbounded_channel();
        let event_sender = TuiEventSender::new(tx);
        let mut app = ChatApp::new("Session".to_string(), cwd);
        app.set_input("abc".to_string());

        handle_key_event(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::SUPER),
            &mut app,
            &settings,
            cwd,
            &event_sender,
            || Ok((120, 40)),
        )
        .unwrap();

        assert_ne!(app.input, "abcv");
    }

    #[test]
    fn test_mouse_wheel_event_keeps_cursor_coordinates() {
        let event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 77,
            row: 14,
            modifiers: KeyModifiers::NONE,
        };

        let translated = handle_mouse_event(event);
        assert!(matches!(
            translated,
            Some(InputEvent::ScrollDown { x: 77, y: 14 })
        ));
    }

    #[test]
    fn test_sidebar_wheel_scroll_only_applies_inside_sidebar_column() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);

        for idx in 0..120 {
            app.messages.push(tui::ChatMessage::ToolCall {
                name: "edit".to_string(),
                args: "{}".to_string(),
                output: Some(
                    serde_json::json!({
                        "path": format!("src/file-{idx}.rs"),
                        "applied": true,
                        "summary": {"added_lines": 1, "removed_lines": 0},
                        "diff": ""
                    })
                    .to_string(),
                ),
                is_error: Some(false),
            });
        }

        let terminal_rect = Rect {
            x: 0,
            y: 0,
            width: 120,
            height: 40,
        };
        let layout_rects = tui::compute_layout_rects(terminal_rect, &app);
        let sidebar_content = layout_rects
            .sidebar_content
            .expect("sidebar should be visible");
        let main_messages = layout_rects
            .main_messages
            .expect("main messages area should be visible");

        // Test: scrolling in sidebar area scrolls sidebar
        let inside_scrolled = handle_area_scroll(
            &mut app,
            terminal_rect,
            sidebar_content.x,
            sidebar_content.y,
            0,
            3,
        );
        assert!(inside_scrolled);
        assert!(app.sidebar_scroll.offset > 0);

        let previous_sidebar_offset = app.sidebar_scroll.offset;
        let previous_message_offset = app.message_scroll.offset;

        // Test: scrolling in main messages area scrolls messages, not sidebar
        let in_main_scrolled = handle_area_scroll(
            &mut app,
            terminal_rect,
            main_messages.x,
            main_messages.y,
            0,
            3,
        );
        assert!(in_main_scrolled);
        assert!(app.message_scroll.offset > previous_message_offset);
        assert_eq!(app.sidebar_scroll.offset, previous_sidebar_offset);
    }

    #[test]
    fn test_scroll_up_from_auto_scroll_moves_immediately() {
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path();
        let mut app = ChatApp::new("Session".to_string(), cwd);

        for i in 0..120 {
            app.messages
                .push(tui::ChatMessage::Assistant(format!("line {i}")));
        }
        app.mark_dirty();
        app.message_scroll.auto_follow = true;
        app.message_scroll.offset = 0;

        scroll_up_steps(&mut app, 120, 30, 1);

        assert!(!app.message_scroll.auto_follow);
        assert!(app.message_scroll.offset > 0);
    }

    #[test]
    fn bash_approval_prompt_uses_always_allow_option() {
        let request = crate::core::ApprovalRequest {
            title: "Tool Execution Approval".to_string(),
            body: "Allow `git diff --name-only`".to_string(),
            action: serde_json::json!({
                "operation": "tool_execution",
                "tool_name": "bash",
                "approval_kind": "bash",
                "permission_rule": "Bash(git status*)"
            }),
        };

        let prompt = approval_request_to_question_prompt(&request);
        assert_eq!(prompt.question, "Allow `git diff --name-only`");
        assert_eq!(prompt.options[1].label, "Always Allow");
        assert!(prompt.options[1].description.contains("Bash(git status*)"));
    }

    #[test]
    fn parse_approval_choice_maps_bash_and_non_bash_labels() {
        let bash_request = crate::core::ApprovalRequest {
            title: "Tool Execution Approval".to_string(),
            body: "Allow bash?".to_string(),
            action: serde_json::json!({"approval_kind": "bash"}),
        };
        let generic_request = crate::core::ApprovalRequest {
            title: "Tool Execution Approval".to_string(),
            body: "Allow action?".to_string(),
            action: serde_json::json!({"approval_kind": "tool"}),
        };

        let bash_always = vec![vec!["Always Allow".to_string()]];
        let generic_session = vec![vec!["Always Allow in this Session".to_string()]];

        assert_eq!(
            parse_approval_choice(&bash_request, &bash_always),
            Some(crate::core::ApprovalChoice::AllowAlways)
        );
        assert_eq!(
            parse_approval_choice(&generic_request, &generic_session),
            Some(crate::core::ApprovalChoice::AllowSession)
        );
    }

    #[test]
    fn parse_approval_choice_returns_none_for_unknown_labels() {
        let request = crate::core::ApprovalRequest {
            title: "Tool Execution Approval".to_string(),
            body: "Allow action?".to_string(),
            action: serde_json::json!({"approval_kind": "bash"}),
        };

        let answers = vec![vec!["Not a valid label".to_string()]];
        assert_eq!(parse_approval_choice(&request, &answers), None);
    }

    #[test]
    fn persist_bash_always_allow_updates_local_config() {
        let temp_dir = tempdir().expect("tempdir");
        let request = crate::core::ApprovalRequest {
            title: "Tool Execution Approval".to_string(),
            body: "Allow bash?".to_string(),
            action: serde_json::json!({
                "operation": "tool_execution",
                "tool_name": "bash",
                "approval_kind": "bash",
                "permission_rule": "Bash(git status*)"
            }),
        };

        persist_approval_choice_if_needed(
            temp_dir.path(),
            &request,
            crate::core::ApprovalChoice::AllowAlways,
        )
        .expect("persist approval");

        let local = std::fs::read_to_string(temp_dir.path().join(".hh/config.local.json"))
            .expect("read local config");
        assert!(local.contains("Bash(git status*)"));
    }

    #[test]
    fn persist_bash_deny_updates_local_config() {
        let temp_dir = tempdir().expect("tempdir");
        let request = crate::core::ApprovalRequest {
            title: "Tool Execution Approval".to_string(),
            body: "Deny bash?".to_string(),
            action: serde_json::json!({
                "operation": "tool_execution",
                "tool_name": "bash",
                "approval_kind": "bash",
                "permission_rule": "Bash(rm -rf*)"
            }),
        };

        persist_approval_choice_if_needed(
            temp_dir.path(),
            &request,
            crate::core::ApprovalChoice::Deny,
        )
        .expect("persist deny approval");

        let local = std::fs::read_to_string(temp_dir.path().join(".hh/config.local.json"))
            .expect("read local config");
        assert!(local.contains("Bash(rm -rf*)"));
        assert!(local.contains("\"deny\""));
    }

    #[test]
    fn persist_non_bash_or_non_persistent_choices_do_not_write_local_config() {
        let temp_dir = tempdir().expect("tempdir");
        let generic_request = crate::core::ApprovalRequest {
            title: "Approval".to_string(),
            body: "Allow action?".to_string(),
            action: serde_json::json!({"approval_kind": "tool", "permission_rule": "Any(*)"}),
        };
        let bash_request = crate::core::ApprovalRequest {
            title: "Approval".to_string(),
            body: "Allow once?".to_string(),
            action: serde_json::json!({"approval_kind": "bash", "permission_rule": "Bash(ls*)"}),
        };

        persist_approval_choice_if_needed(
            temp_dir.path(),
            &generic_request,
            crate::core::ApprovalChoice::AllowAlways,
        )
        .expect("non-bash should be ignored");
        persist_approval_choice_if_needed(
            temp_dir.path(),
            &bash_request,
            crate::core::ApprovalChoice::AllowOnce,
        )
        .expect("allow once should not persist");

        assert!(!temp_dir.path().join(".hh/config.local.json").exists());
    }
}
