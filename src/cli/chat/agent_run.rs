use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::cli::chat::session::{fallback_session_title, spawn_session_title_generation_task};
use crate::cli::chat::subagent::{current_subagent_manager, map_subagent_node_event};
use crate::cli::render;
use crate::cli::tui::{ChatApp, SubmittedInput, TuiEvent, TuiEventSender};
use crate::config::{Settings, upsert_local_permission_rule};
use crate::core::agent::subagent_manager::SubagentManager;
use crate::core::agent::{AgentEvents, AgentLoop};
use crate::core::{Message, Role};
use crate::permission::PermissionMatcher;
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::SessionStore;
use crate::tool::registry::{ToolRegistry, ToolRegistryContext};
use crate::tool::task::TaskToolRuntimeContext;

#[derive(Clone)]
pub(super) struct AgentRunOptions {
    pub(super) session_id: Option<String>,
    pub(super) session_title: Option<String>,
    pub(super) allow_questions: bool,
}

pub(super) struct AgentLoopOptions {
    pub(super) subagent_manager: Option<Arc<SubagentManager>>,
    pub(super) parent_task_id: Option<String>,
    pub(super) depth: usize,
    pub(super) session_id: Option<String>,
    pub(super) session_title: Option<String>,
    pub(super) session_parent_id: Option<String>,
}

pub(super) fn spawn_agent_task(
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

async fn run_agent(
    settings: Settings,
    cwd: &Path,
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
    let mut subagent_poller = parent_session_id.as_ref().map(|session_id| {
        start_subagent_poller(
            Arc::clone(&subagent_manager),
            event_sender.clone(),
            session_id.clone(),
        )
    });
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

    if let Some((stop_tx, handle)) = subagent_poller.take() {
        let _ = stop_tx.send(());
        let _ = handle.await;
    }

    if let Some(parent_session_id) = parent_session_id.as_deref() {
        let nodes = subagent_manager.list_for_parent(parent_session_id).await;
        event_sender.send(TuiEvent::SubagentsChanged(
            nodes.iter().map(map_subagent_node_event).collect(),
        ));
    }

    Ok(())
}

fn start_subagent_poller(
    subagent_manager: Arc<SubagentManager>,
    event_sender: TuiEventSender,
    parent_session_id: String,
) -> (oneshot::Sender<()>, JoinHandle<()>) {
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        loop {
            let nodes = subagent_manager.list_for_parent(&parent_session_id).await;
            event_sender.send(TuiEvent::SubagentsChanged(
                nodes.iter().map(map_subagent_node_event).collect(),
            ));

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                _ = &mut stop_rx => break,
            }
        }
    });
    (stop_tx, handle)
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
        super::format_modalities(&selected.model.modalities.input)
    )
}

pub(super) fn approval_request_to_question_prompt(
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
pub(super) fn parse_approval_choice(
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
pub(super) fn persist_approval_choice_if_needed(
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

pub(super) fn create_agent_loop<E>(
    settings: Settings,
    cwd: &Path,
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
            parent_task_id.clone(),
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

pub(super) fn handle_chat_message(
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
            role: Role::User,
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

pub(super) async fn run_single_prompt_with_events<E>(
    settings: Settings,
    cwd: &Path,
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
            let generated = match super::session::generate_session_title(
                &settings, &model_ref, &prompt,
            )
            .await
            {
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
