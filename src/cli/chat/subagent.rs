use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use crate::agent::{AgentLoader, AgentMode, AgentRegistry};
use crate::cli::chat::agent_run::{AgentLoopOptions, create_agent_core};
use crate::cli::tui;
use crate::config::Settings;
use crate::core::SessionSink;
use crate::core::agent::RunnerOutput;
use crate::core::agent::subagent_manager::{
    SubagentExecutionRequest, SubagentExecutionResult, SubagentExecutor, SubagentManager,
    SubagentStatus,
};
use crate::core::{Message, Role};
use crate::session::types::SubAgentFailureReason;
use crate::session::{SessionEvent, event_id};

static GLOBAL_SUBAGENT_MANAGER: OnceLock<Arc<SubagentManager>> = OnceLock::new();

pub(super) fn initialize_subagent_manager(settings: Settings, cwd: PathBuf) {
    let _ = GLOBAL_SUBAGENT_MANAGER.get_or_init(|| Arc::new(build_subagent_manager(settings, cwd)));
}

pub(super) fn current_subagent_manager(settings: &Settings, cwd: &Path) -> Arc<SubagentManager> {
    Arc::clone(
        GLOBAL_SUBAGENT_MANAGER
            .get_or_init(|| Arc::new(build_subagent_manager(settings.clone(), cwd.to_path_buf()))),
    )
}

fn build_subagent_manager(settings: Settings, cwd: PathBuf) -> SubagentManager {
    let enabled = settings.agent.parallel_subagents;
    let max_parallel = settings.agent.max_parallel_subagents;
    let max_depth = settings.agent.sub_agent_max_depth;
    let executor_settings = settings.clone();
    let executor: SubagentExecutor = Arc::new(move |request| {
        let settings = executor_settings.clone();
        let cwd = cwd.clone();
        Box::pin(async move {
            if !enabled {
                return SubagentExecutionResult {
                    status: SubagentStatus::Failed,
                    summary: "parallel sub-agents are disabled by configuration".to_string(),
                    error: Some("agent.parallel_subagents=false".to_string()),
                    failure_reason: Some(SubAgentFailureReason::RuntimeError),
                };
            }
            run_subagent_execution(settings, cwd, request).await
        })
    });

    SubagentManager::new(max_parallel, max_depth, executor)
}

async fn run_subagent_execution(
    settings: Settings,
    cwd: PathBuf,
    request: SubagentExecutionRequest,
) -> SubagentExecutionResult {
    let loader = match AgentLoader::new() {
        Ok(loader) => loader,
        Err(err) => {
            return SubagentExecutionResult {
                status: SubagentStatus::Failed,
                summary: "failed to initialize agent loader".to_string(),
                error: Some(err.to_string()),
                failure_reason: Some(SubAgentFailureReason::RuntimeError),
            };
        }
    };
    let registry = match loader.load_agents() {
        Ok(agents) => AgentRegistry::new(agents),
        Err(err) => {
            return SubagentExecutionResult {
                status: SubagentStatus::Failed,
                summary: "failed to load agents".to_string(),
                error: Some(err.to_string()),
                failure_reason: Some(SubAgentFailureReason::RuntimeError),
            };
        }
    };

    let Some(agent) = registry.get_agent(&request.subagent_type).cloned() else {
        return SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: format!("unknown subagent_type: {}", request.subagent_type),
            error: None,
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        };
    };
    if agent.mode != AgentMode::Subagent {
        return SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: format!("agent '{}' is not a subagent", agent.name),
            error: None,
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        };
    }

    let mut child_settings = settings.clone();
    child_settings.apply_agent_settings(&agent);
    child_settings.selected_agent = Some(agent.name.clone());
    let model_ref = child_settings.selected_model_ref().to_string();
    let task_id = request.task_id.clone();
    let child_session_id = request.child_session_id.clone();

    let loop_runner = match create_agent_core(
        child_settings,
        &cwd,
        &model_ref,
        AgentLoopOptions {
            subagent_manager: Some(current_subagent_manager(&settings, &cwd)),
            parent_task_id: Some(request.task_id.clone()),
            depth: request.depth,
            session_id: Some(child_session_id.clone()),
            session_title: Some(request.description),
            session_parent_id: Some(request.parent_session_id),
        },
    ) {
        Ok(loop_runner) => loop_runner,
        Err(err) => {
            return SubagentExecutionResult {
                status: SubagentStatus::Failed,
                summary: "failed to initialize sub-agent runtime".to_string(),
                error: Some(err.to_string()),
                failure_reason: Some(SubAgentFailureReason::RuntimeError),
            };
        }
    };

    let (input_tx, input_rx) = tokio::sync::mpsc::channel(64);
    let _ = input_tx.try_send(crate::core::agent::RunnerInput::Message(Message {
        role: Role::User,
        content: request.prompt,
        attachments: Vec::new(),
        tool_call_id: None,
        tool_calls: Vec::new(),
    }));

    match loop_runner
        .run(
            input_rx,
            &mut |output| {
                if let crate::core::agent::RunnerOutput::ApprovalRequired {
                    call_id,
                    request: _,
                } = &output
                {
                    let tx = input_tx.clone();
                    let call_id = call_id.clone();
                    tokio::spawn(async move {
                        let _ = tx
                            .send(crate::core::agent::RunnerInput::ApprovalDecision {
                                call_id,
                                choice: crate::core::ApprovalChoice::AllowSession,
                            })
                            .await;
                    });
                }
                apply_runner_output_to_subagent_session(&loop_runner.session, output)
            },
            &mut Vec::new,
        )
        .await
    {
        Ok(output) => completed_subagent_result(output, &task_id, &child_session_id),
        Err(err) => SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: "sub-agent execution failed".to_string(),
            error: Some(err.to_string()),
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        },
    }
}

fn apply_runner_output_to_subagent_session(
    session: &impl SessionSink,
    output: RunnerOutput,
) -> anyhow::Result<()> {
    match output {
        RunnerOutput::ThinkingRecorded(content) => {
            session.append(&SessionEvent::Thinking {
                id: event_id(),
                content,
            })?;
        }
        RunnerOutput::ToolCallRecorded(call) => {
            session.append(&SessionEvent::ToolCall { call })?;
        }
        RunnerOutput::ToolEnd {
            call_id, result, ..
        } => {
            session.append(&SessionEvent::ToolResult {
                id: call_id,
                is_error: result.is_error,
                output: result.output.clone(),
                result: Some(result),
            })?;
        }
        RunnerOutput::SnapshotUpdated(snapshot) => {
            session.save_runner_state_snapshot(&snapshot)?;
        }
        RunnerOutput::ApprovalRecorded {
            tool_name,
            approved,
            action,
            choice,
        } => {
            session.append(&SessionEvent::Approval {
                id: event_id(),
                tool_name,
                approved,
                action,
                choice,
            })?;
        }
        RunnerOutput::MessageAdded(message) => {
            session.append(&SessionEvent::Message {
                id: event_id(),
                message,
            })?;
        }
        RunnerOutput::ThinkingDelta(_)
        | RunnerOutput::AssistantDelta(_)
        | RunnerOutput::StateUpdated(_)
        | RunnerOutput::ApprovalRequired { .. }
        | RunnerOutput::QuestionRequired { .. }
        | RunnerOutput::ToolStart { .. }
        | RunnerOutput::Cancelled
        | RunnerOutput::TurnComplete
        | RunnerOutput::Error(_) => {}
    }

    Ok(())
}

fn completed_subagent_result(
    output: String,
    task_id: &str,
    child_session_id: &str,
) -> SubagentExecutionResult {
    if output.trim().is_empty() {
        return SubagentExecutionResult {
            status: SubagentStatus::Failed,
            summary: "sub-agent produced no final response".to_string(),
            error: Some(format!(
                "empty final assistant response (task_id={}, session_id={})",
                task_id, child_session_id
            )),
            failure_reason: Some(SubAgentFailureReason::RuntimeError),
        };
    }

    SubagentExecutionResult {
        status: SubagentStatus::Completed,
        summary: output,
        error: None,
        failure_reason: None,
    }
}

pub(super) fn map_subagent_node_event(
    node: &crate::core::agent::subagent_manager::SubagentNode,
) -> tui::SubagentEventItem {
    let status = node.status.label().to_string();

    let finished_at = if node.status.is_terminal() {
        Some(node.updated_at)
    } else {
        None
    };

    tui::SubagentEventItem {
        task_id: node.task_id.clone(),
        session_id: node.session_id.clone(),
        name: node.name.clone(),
        agent_name: node.agent_name.clone(),
        status,
        prompt: node.prompt.clone(),
        depth: node.depth,
        parent_task_id: node.parent_task_id.clone(),
        started_at: node.started_at,
        finished_at,
        summary: node.summary.clone(),
        error: node.error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> SubagentExecutionRequest {
        SubagentExecutionRequest {
            task_id: "task-1".to_string(),
            name: "child".to_string(),
            parent_session_id: "parent-session".to_string(),
            parent_task_id: None,
            description: "desc".to_string(),
            prompt: "prompt".to_string(),
            subagent_type: "general".to_string(),
            child_session_id: "child-session".to_string(),
            depth: 1,
        }
    }

    #[test]
    fn completed_subagent_result_rejects_empty_output() {
        let request = sample_request();
        let result = completed_subagent_result(
            "   \n".to_string(),
            &request.task_id,
            &request.child_session_id,
        );

        assert_eq!(result.status, SubagentStatus::Failed);
        assert_eq!(result.summary, "sub-agent produced no final response");
        assert_eq!(
            result.error.as_deref(),
            Some("empty final assistant response (task_id=task-1, session_id=child-session)")
        );
        assert_eq!(
            result.failure_reason,
            Some(SubAgentFailureReason::RuntimeError)
        );
    }

    #[test]
    fn completed_subagent_result_keeps_non_empty_output() {
        let request = sample_request();
        let result = completed_subagent_result(
            "done".to_string(),
            &request.task_id,
            &request.child_session_id,
        );

        assert_eq!(result.status, SubagentStatus::Completed);
        assert_eq!(result.summary, "done");
        assert!(result.error.is_none());
        assert!(result.failure_reason.is_none());
    }
}
