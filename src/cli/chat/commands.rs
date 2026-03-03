use std::path::Path;

use anyhow::Context;

use crate::cli::chat::session;
use crate::cli::tui::{self, ChatApp, SubmittedInput, TuiEvent, TuiEventSender};
use crate::config::Settings;
use crate::session::SessionStore;

pub(super) fn handle_submitted_input(
    input: SubmittedInput,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    if input.text.starts_with('/') && input.attachments.is_empty() {
        if let Some(tui::ChatMessage::User(last)) = app.messages.last()
            && last == &input.text
        {
            app.messages.pop();
            app.mark_dirty();
        }
        handle_slash_command(input.text, app, settings, cwd, event_sender);
    } else if app.is_picking_session {
        if let Err(e) = session::handle_session_selection(input.text, app, settings, cwd) {
            app.messages
                .push(tui::ChatMessage::Assistant(e.to_string()));
            app.mark_dirty();
        }
        app.set_processing(false);
    } else {
        super::handle_chat_message(input, app, settings, cwd, event_sender);
    }
}

fn handle_slash_command(
    input: String,
    app: &mut ChatApp,
    settings: &Settings,
    cwd: &Path,
    event_sender: &TuiEventSender,
) {
    let scoped_sender = event_sender.scoped(app.session_epoch(), app.run_epoch());
    let mut parts = input.split_whitespace();
    let command = parts.next().unwrap_or_default();

    match command {
        "/new" => {
            app.start_new_session(super::build_session_name(cwd));
            finish_idle(app);
        }
        "/model" => {
            if let Some(model_ref) = parts.next() {
                if let Some(model) = settings.resolve_model_ref(model_ref) {
                    app.set_selected_model(model_ref);
                    finish_with_assistant(
                        app,
                        format!(
                            "Switched to {} ({} -> {}, context: {}, output: {})",
                            model_ref,
                            super::format_modalities(&model.model.modalities.input),
                            super::format_modalities(&model.model.modalities.output),
                            model.model.limits.context,
                            model.model.limits.output
                        ),
                    );
                } else {
                    finish_with_assistant(app, format!("Unknown model: {model_ref}"));
                }
            } else {
                let mut text = format!(
                    "Current model: {}\n\nAvailable models:\n",
                    app.selected_model_ref()
                );
                for option in &app.available_models {
                    text.push_str(&format!(
                        "- {} ({}, context: {} tokens)\n",
                        option.full_id, option.modality, option.max_context_size
                    ));
                }
                text.push_str("\nUse /model <provider-id/model-id> to switch.");
                finish_with_assistant(app, text);
            }
        }
        "/compact" => {
            let Some(session_id) = app.session_id.clone() else {
                finish_with_assistant(app, "No active session to compact yet.");
                return;
            };
            let model_ref = app.selected_model_ref().to_string();

            app.handle_event(&TuiEvent::CompactionStart);

            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let settings = settings.clone();
                let cwd = cwd.to_path_buf();
                let sender = scoped_sender.clone();
                handle.spawn(async move {
                    match session::compact_session_with_llm(settings, &cwd, &session_id, &model_ref)
                        .await
                    {
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
                        rt.block_on(session::compact_session_with_llm(
                            settings.clone(),
                            cwd,
                            &session_id,
                            &model_ref,
                        ))
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
                finish_with_assistant(app, "No previous sessions found.");
            } else {
                app.available_sessions = sessions;
                app.is_picking_session = true;

                let mut msg = String::from("Available sessions:\n");
                for (i, s) in app.available_sessions.iter().enumerate() {
                    msg.push_str(&format!("[{}] {}\n", i + 1, s.title));
                }
                msg.push_str("\nEnter number to resume:");
                finish_with_assistant(app, msg);
            }
        }
        _ => {
            finish_with_assistant(app, format!("Unknown command: {}", input));
        }
    }
}

fn finish_with_assistant(app: &mut ChatApp, message: impl Into<String>) {
    app.messages
        .push(tui::ChatMessage::Assistant(message.into()));
    finish_idle(app);
}

fn finish_idle(app: &mut ChatApp) {
    app.mark_dirty();
    app.set_processing(false);
}
