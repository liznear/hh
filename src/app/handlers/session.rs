use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;

use crate::app::chat_state::{
    ChatApp, ChatMessage, SubagentItemView, SubagentStatusView,
};
use crate::app::events::{TuiEvent, TuiEventSender};
use crate::config::Settings;
use crate::core::Message;
#[cfg(not(test))]
use crate::provider::openai_compatible::OpenAiCompatibleProvider;
use crate::session::{SessionEvent, SessionStore, event_id};

pub(super) async fn compact_session_with_llm(
    settings: Settings,
    cwd: &Path,
    session_id: &str,
    model_ref: &str,
) -> anyhow::Result<String> {
    let store = SessionStore::new(&settings.session.root, cwd, Some(session_id), None)
        .context("Failed to load session store")?;
    let messages = store
        .replay_messages()
        .context("Failed to replay session for compaction")?;

    if messages.is_empty() {
        return Ok("No prior context to compact yet.".to_string());
    }

    let summary = generate_compaction_summary(&settings, messages, model_ref).await?;
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
    model_ref: &str,
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        let _ = settings;
        let _ = messages;
        let _ = model_ref;
        Ok("Compacted context summary for tests.".to_string())
    }

    #[cfg(not(test))]
    {
        let mut prompt_messages = Vec::with_capacity(messages.len() + 2);
        prompt_messages.push(Message {
            role: crate::core::Role::System,
            content: "You compact conversation history for an engineering assistant. Produce a concise summary that preserves requirements, decisions, constraints, open questions, and pending work items. Prefer bullet points. Do not invent details.".to_string(),
            attachments: Vec::new(),
            tool_call_id: None, tool_calls: Vec::new(),
        });
        prompt_messages.extend(messages);
        prompt_messages.push(Message {
            role: crate::core::Role::User,
            content: "Compact the conversation so future turns can continue from this summary with minimal context loss.".to_string(),
            attachments: Vec::new(),
            tool_call_id: None, tool_calls: Vec::new(),
        });

        let selected = settings
            .resolve_model_ref(model_ref)
            .with_context(|| format!("model is not configured: {model_ref}"))?;

        let provider = OpenAiCompatibleProvider::new(
            selected.provider.base_url.clone(),
            selected.model.id.clone(),
            selected.provider.api_key_env.clone(),
        );

        let response = crate::core::Provider::complete(
            &provider,
            crate::core::ProviderRequest {
                model: selected.model.id.clone(),
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

pub(crate) fn handle_session_selection(
    input: String,
    app: &mut ChatApp,
    _actions: &mut Vec<crate::app::core::AppAction>,
    settings: &Settings,
    cwd: &Path,
) -> anyhow::Result<()> {
    let idx = input.trim().parse::<usize>().context("Invalid number.")?;

    if idx == 0 || idx > app.available_sessions.len() {
        anyhow::bail!("Invalid session index.");
    }

    let session = app.available_sessions[idx - 1].clone();
    app.bump_session_epoch();
    app.session_id = Some(session.id.clone());
    app.session_name = session.title.clone();
    app.last_context_tokens = None;
    app.is_picking_session = false;

    let store = SessionStore::new(&settings.session.root, cwd, Some(&session.id), None)
        .context("Failed to load session store")?;

    let events = store.replay_events().context("Failed to replay session")?;

    app.messages.clear();
    app.todo_items.clear();
    app.subagent_items.clear();
    let mut subagent_items_by_task: HashMap<String, SubagentItemView> = HashMap::new();
    for event in events {
        match event {
            SessionEvent::Message { message, .. } => {
                let chat_msg = match message.role {
                    crate::core::Role::User => ChatMessage::User {
                        text: message.content,
                        queued: false,
                    },
                    crate::core::Role::Assistant => ChatMessage::Assistant(message.content),
                    _ => continue,
                };
                app.messages.push(chat_msg);
            }
            SessionEvent::ToolCall { call } => {
                app.messages.push(ChatMessage::ToolCall {
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
                let pending_tool_name = app.messages.iter().rev().find_map(|msg| match msg {
                    ChatMessage::ToolCall { name, output, .. } if output.is_none() => {
                        Some(name.clone())
                    }
                    _ => None,
                });
                if let Some(name) = pending_tool_name {
                    let replayed_result = result.unwrap_or_else(|| {
                        if is_error {
                            crate::tool::ToolResult::err_text("error", output)
                        } else {
                            crate::tool::ToolResult::ok_text("ok", output)
                        }
                    });
                    app.handle_event(&TuiEvent::ToolEnd {
                        name,
                        result: replayed_result,
                    });
                }
            }
            SessionEvent::Thinking { content, .. } => {
                app.messages.push(ChatMessage::Thinking(content));
            }
            SessionEvent::Compact { summary, .. } => {
                app.messages.push(ChatMessage::Compaction(summary));
            }
            SessionEvent::SubAgentStart {
                id,
                task_id,
                session_id,
                name,
                parent_id,
                agent_name,
                prompt,
                depth,
                created_at,
                status,
                ..
            } => {
                let task_id = task_id.unwrap_or(id);
                subagent_items_by_task.insert(
                    task_id.clone(),
                    SubagentItemView {
                        task_id,
                        session_id: session_id.unwrap_or_default(),
                        name: name
                            .or_else(|| agent_name.clone())
                            .unwrap_or_else(|| "subagent".to_string()),
                        parent_task_id: parent_id,
                        agent_name: agent_name.unwrap_or_else(|| "subagent".to_string()),
                        prompt,
                        summary: None,
                        depth,
                        started_at: created_at,
                        finished_at: None,
                        status: SubagentStatusView::from_lifecycle(status),
                    },
                );
            }
            SessionEvent::SubAgentResult {
                id,
                task_id,
                status,
                summary,
                output,
                ..
            } => {
                let task_id = task_id.unwrap_or(id);
                let entry = subagent_items_by_task
                    .entry(task_id.clone())
                    .or_insert_with(|| SubagentItemView {
                        task_id,
                        session_id: String::new(),
                        name: "subagent".to_string(),
                        parent_task_id: None,
                        agent_name: "subagent".to_string(),
                        prompt: String::new(),
                        summary: None,
                        depth: 0,
                        started_at: 0,
                        finished_at: None,
                        status: SubagentStatusView::Running,
                    });
                entry.status = SubagentStatusView::from_lifecycle(status);
                if entry.status.is_terminal() {
                    entry.finished_at = Some(entry.started_at);
                }
                entry.summary = if let Some(summary) = summary {
                    Some(summary)
                } else if output.trim().is_empty() {
                    None
                } else {
                    Some(output)
                };
            }
            _ => {}
        }
    }
    app.subagent_items = subagent_items_by_task.into_values().collect();
    for item in &mut app.subagent_items {
        if item.status.is_active() {
            item.status = SubagentStatusView::Failed;
            if item.summary.is_none() {
                item.summary = Some("interrupted_by_restart".to_string());
            }
        }
    }
    app.mark_dirty();

    Ok(())
}

pub(crate) fn fallback_session_title(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return "Image input".to_string();
    }

    trimmed
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_session_title(raw: &str, fallback: &str) -> String {
    let cleaned = raw
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"')
        .trim_matches('`')
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");

    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

pub(crate) fn spawn_session_title_generation_task(
    settings: &Settings,
    cwd: &Path,
    session_id: String,
    model_ref: String,
    prompt: String,
    event_sender: &TuiEventSender,
) {
    let settings = settings.clone();
    let cwd = cwd.to_path_buf();
    let event_sender = event_sender.clone();
    tokio::spawn(async move {
        let fallback = fallback_session_title(&prompt);
        let generated = match generate_session_title(&settings, &model_ref, &prompt).await {
            Ok(title) => title,
            Err(_) => return,
        };

        let store = match SessionStore::new(&settings.session.root, &cwd, Some(&session_id), None) {
            Ok(store) => store,
            Err(_) => return,
        };

        let title = normalize_session_title(&generated, &fallback);
        if store.update_title(title.clone()).is_ok() {
            event_sender.send(TuiEvent::SessionTitle(title));
        }
    });
}

pub(super) async fn generate_session_title(
    settings: &Settings,
    model_ref: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    #[cfg(test)]
    {
        let _ = settings;
        let _ = model_ref;
        Ok(normalize_session_title(
            "Generated test title",
            &fallback_session_title(prompt),
        ))
    }

    #[cfg(not(test))]
    {
        let selected = settings
            .resolve_model_ref(model_ref)
            .with_context(|| format!("model is not configured: {model_ref}"))?;

        let provider = OpenAiCompatibleProvider::new(
            selected.provider.base_url.clone(),
            selected.model.id.clone(),
            selected.provider.api_key_env.clone(),
        );

        let request = crate::core::ProviderRequest {
            model: selected.model.id.clone(),
            messages: vec![
                Message {
                    role: crate::core::Role::System,
                    content: "Generate a concise session title for this prompt. Return only the title, no punctuation wrappers, and keep it to 12 words or fewer.".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None, tool_calls: Vec::new(),
                },
                Message {
                    role: crate::core::Role::User,
                    content: prompt.to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None, tool_calls: Vec::new(),
                },
            ],
            tools: Vec::new(),
        };

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 1..=3 {
            if attempt > 1 {
                tokio::time::sleep(std::time::Duration::from_millis(200 * attempt as u64)).await;
            }
            match crate::core::Provider::complete(&provider, request.clone()).await {
                Ok(response) => {
                    let fallback = fallback_session_title(prompt);
                    return Ok(normalize_session_title(
                        &response.assistant_message.content,
                        &fallback,
                    ));
                }
                Err(err) => {
                    last_error =
                        Some(err.context(format!("title generation attempt {attempt}/3 failed")));
                }
            }
        }

        let err = last_error.unwrap_or_else(|| anyhow::anyhow!("unknown title request failure"));
        Err(err).context("Session title request failed")
    }
}
