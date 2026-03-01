use crate::agent::{AgentLoader, AgentMode, AgentRegistry};
use crate::config::Settings;
use crate::core::agent::subagent_manager::{SubagentManager, SubagentRequest};
use crate::session::SessionStore;
use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct TaskToolRuntimeContext {
    pub manager: Arc<SubagentManager>,
    pub settings: Settings,
    pub workspace_root: PathBuf,
    pub parent_session_id: String,
    pub parent_task_id: Option<String>,
    pub depth: usize,
}

pub struct TaskTool {
    context: TaskToolRuntimeContext,
    available_subagents: Vec<AvailableSubagent>,
}

impl TaskTool {
    pub fn new(context: TaskToolRuntimeContext) -> Self {
        let available_subagents = discover_available_subagents();
        Self {
            context,
            available_subagents,
        }
    }
}

#[derive(Debug, Clone)]
struct AvailableSubagent {
    name: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct TaskToolOutput {
    task_id: String,
    name: String,
    description: String,
    status: String,
    message: String,
    agent_name: String,
    prompt: String,
    depth: usize,
    parent_task_id: Option<String>,
    started_at: u64,
    finished_at: Option<u64>,
    summary: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TaskToolArgs {
    name: String,
    description: String,
    prompt: String,
    subagent_type: String,
    #[serde(default)]
    task_id: Option<String>,
}

#[async_trait]
impl Tool for TaskTool {
    fn schema(&self) -> ToolSchema {
        let subagent_names: Vec<String> = self
            .available_subagents
            .iter()
            .map(|agent| agent.name.clone())
            .collect();
        let mut subagent_type_schema = json!({
            "type": "string",
            "description": "Registered sub-agent name"
        });
        if !subagent_names.is_empty() {
            subagent_type_schema["enum"] = json!(subagent_names);
        }

        ToolSchema {
            name: "task".to_string(),
            description: format!(
                "Spawn or resume a sub-agent task.\n\nParameter contract:\n- `name` (required): human-readable task label shown in UI.\n- `description` (required): short statement of delegated intent.\n- `prompt` (required): full instructions for the child agent.\n- `subagent_type` (required): which registered sub-agent to run.\n- `task_id` (optional): if provided, resume that existing child task in this parent session; if omitted, create a new task.\n\nReturn semantics:\n- Returns terminal status for this task (`done`/`error`/`cancelled`) after execution completes.\n\n{}",
                format_available_subagents(&self.available_subagents),
            ),
            capability: Some("task".to_string()),
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Required. Human-readable task name shown in UI subagent list"
                    },
                    "description": {
                        "type": "string",
                        "description": "Required. Short summary of what this delegated task should achieve"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Required. Full prompt/instructions executed by the selected sub-agent. Ask the child to keep output concise (short summary with only essential facts)."
                    },
                    "subagent_type": subagent_type_schema,
                    "task_id": {
                        "type": "string",
                        "description": "Optional. Existing task id to resume within the current parent session; omit to start a new task"
                    }
                },
                "required": ["name", "description", "prompt", "subagent_type"],
                "additionalProperties": false
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let parsed: TaskToolArgs = match serde_json::from_value(args) {
            Ok(value) => value,
            Err(err) => {
                return ToolResult::error(format!("invalid task args: {err}"));
            }
        };

        if parsed.name.trim().is_empty() {
            return ToolResult::error("name must not be empty");
        }
        if parsed.description.trim().is_empty() {
            return ToolResult::error("description must not be empty");
        }
        if parsed.prompt.trim().is_empty() {
            return ToolResult::error("prompt must not be empty");
        }

        let loader = match AgentLoader::new() {
            Ok(loader) => loader,
            Err(err) => return ToolResult::error(format!("failed to load agent registry: {err}")),
        };
        let registry = match loader.load_agents() {
            Ok(agents) => AgentRegistry::new(agents),
            Err(err) => return ToolResult::error(format!("failed to load agents: {err}")),
        };

        let Some(agent) = registry.get_agent(&parsed.subagent_type) else {
            return ToolResult::error(format!("unknown subagent_type: {}", parsed.subagent_type));
        };
        if agent.mode != AgentMode::Subagent {
            return ToolResult::error(format!(
                "agent '{}' is not a subagent (mode is {:?})",
                agent.name, agent.mode
            ));
        }

        let parent_session = match SessionStore::new(
            &self.context.settings.session.root,
            &self.context.workspace_root,
            Some(&self.context.parent_session_id),
            None,
        ) {
            Ok(store) => store,
            Err(err) => return ToolResult::error(format!("failed to open parent session: {err}")),
        };

        let task_description = parsed.description.clone();

        let accepted = match self
            .context
            .manager
            .start_or_resume(
                SubagentRequest {
                    name: parsed.name.clone(),
                    description: parsed.description,
                    prompt: parsed.prompt.clone(),
                    subagent_type: parsed.subagent_type.clone(),
                    resume_task_id: parsed.task_id,
                    parent_session_id: self.context.parent_session_id.clone(),
                    parent_task_id: self.context.parent_task_id.clone(),
                    depth: self.context.depth,
                },
                parent_session,
            )
            .await
        {
            Ok(accepted) => accepted,
            Err(err) => return ToolResult::error(err.to_string()),
        };

        let output = TaskToolOutput {
            task_id: accepted.task_id.clone(),
            name: parsed.name,
            description: task_description.clone(),
            status: accepted.status,
            message: accepted.message,
            agent_name: parsed.subagent_type,
            prompt: parsed.prompt,
            depth: self.context.depth.saturating_add(1),
            parent_task_id: self.context.parent_task_id.clone(),
            started_at: 0,
            finished_at: None,
            summary: None,
            error: None,
        };

        let completed = match self
            .context
            .manager
            .wait_for_terminal(&self.context.parent_session_id, &accepted.task_id)
            .await
        {
            Ok(node) => node,
            Err(err) => return ToolResult::error(err.to_string()),
        };

        let output = TaskToolOutput {
            task_id: output.task_id,
            name: completed.name,
            description: task_description,
            status: status_label(&completed.status).to_string(),
            message: output.message,
            agent_name: completed.agent_name,
            prompt: completed.prompt,
            depth: completed.depth,
            parent_task_id: completed.parent_task_id,
            started_at: completed.started_at,
            finished_at: Some(completed.updated_at),
            summary: completed.summary,
            error: completed.error,
        };

        ToolResult::ok_json_typed_serializable(
            "sub-agent completed",
            "application/vnd.hh.subagent.task+json",
            &output,
        )
    }
}

fn status_label(status: &crate::core::agent::subagent_manager::SubagentStatus) -> &'static str {
    match status {
        crate::core::agent::subagent_manager::SubagentStatus::Pending => "queued",
        crate::core::agent::subagent_manager::SubagentStatus::Running => "running",
        crate::core::agent::subagent_manager::SubagentStatus::Completed => "done",
        crate::core::agent::subagent_manager::SubagentStatus::Failed => "error",
        crate::core::agent::subagent_manager::SubagentStatus::Cancelled => "cancelled",
    }
}

fn discover_available_subagents() -> Vec<AvailableSubagent> {
    let loader = match AgentLoader::new() {
        Ok(loader) => loader,
        Err(_) => return Vec::new(),
    };

    let agents = match loader.load_agents() {
        Ok(agents) => agents,
        Err(_) => return Vec::new(),
    };

    let mut subagents = agents
        .into_iter()
        .filter(|agent| agent.mode == AgentMode::Subagent)
        .map(|agent| AvailableSubagent {
            name: agent.name,
            description: agent.description,
        })
        .collect::<Vec<_>>();
    subagents.sort_by(|left, right| left.name.cmp(&right.name));
    subagents
}

fn format_available_subagents(subagents: &[AvailableSubagent]) -> String {
    if subagents.is_empty() {
        return "<available_subagents>none</available_subagents>".to_string();
    }

    let mut description = String::from("<available_subagents>");
    for subagent in subagents {
        description.push_str("\n<subagent>");
        description.push_str("\n<name>");
        description.push_str(&subagent.name);
        description.push_str("</name>");
        description.push_str("\n<description>");
        description.push_str(&subagent.description);
        description.push_str("</description>");
        description.push_str("\n</subagent>");
    }
    description.push_str("\n</available_subagents>");
    description
}
