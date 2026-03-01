use crate::config::Settings;
use crate::core::{ApprovalChoice, ToolExecutor};
use crate::tool::bash::BashTool;
use crate::tool::edit::EditTool;
use crate::tool::fs::{FileAccessController, FsGlob, FsGrep, FsList, FsRead, FsWrite};
use crate::tool::question::QuestionTool;
use crate::tool::skill::SkillTool;
use crate::tool::task::{TaskTool, TaskToolRuntimeContext};
use crate::tool::todo::{TodoReadTool, TodoWriteTool};
use crate::tool::web::{WebFetchTool, WebSearchTool};
use crate::tool::{Tool, ToolSchema};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    non_blocking_tools: std::collections::HashSet<String>,
    file_access: Option<FileAccessController>,
}

#[derive(Clone, Default)]
pub struct ToolRegistryContext {
    pub task: Option<TaskToolRuntimeContext>,
}

impl ToolRegistry {
    pub fn new(settings: &Settings, workspace_root: &Path) -> Self {
        Self::new_with_context(settings, workspace_root, ToolRegistryContext::default())
    }

    pub fn new_with_context(
        settings: &Settings,
        workspace_root: &Path,
        context: ToolRegistryContext,
    ) -> Self {
        let mut tools: HashMap<String, Arc<dyn Tool>> = HashMap::new();
        let mut non_blocking_tools = std::collections::HashSet::new();

        let mut file_access = None;

        if settings.tools.fs {
            let shared_file_access = match FileAccessController::new(workspace_root.to_path_buf()) {
                Ok(access) => access,
                Err(err) => panic!("failed to initialize file access controller: {err}"),
            };
            file_access = Some(shared_file_access.clone());

            register(&mut tools, "read", FsRead::new(shared_file_access.clone()));
            register(
                &mut tools,
                "write",
                FsWrite::new(shared_file_access.clone()),
            );
            register(&mut tools, "list", FsList::new(shared_file_access.clone()));
            register(&mut tools, "glob", FsGlob::new(shared_file_access.clone()));
            register(&mut tools, "grep", FsGrep::new(shared_file_access.clone()));
            register(&mut tools, "todo_read", TodoReadTool);
            register(&mut tools, "todo_write", TodoWriteTool);
            register(&mut tools, "question", QuestionTool);
            register(
                &mut tools,
                "edit",
                EditTool::new(shared_file_access.clone()),
            );
            register(
                &mut tools,
                "skill",
                SkillTool::new(workspace_root.to_path_buf()),
            );

            if let Some(task_context) = context.task {
                register(&mut tools, "task", TaskTool::new(task_context));
                non_blocking_tools.insert("task".to_string());
            }
        }

        if settings.tools.bash {
            register(&mut tools, "bash", BashTool::new());
        }

        if settings.tools.web {
            register(&mut tools, "web_fetch", WebFetchTool::new());
            register(&mut tools, "web_search", WebSearchTool::new());
        }

        Self {
            tools,
            non_blocking_tools,
            file_access,
        }
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value) -> crate::tool::ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.execute(args).await,
            None => {
                crate::tool::ToolResult::err_text("unknown_tool", format!("unknown tool: {}", name))
            }
        }
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.tools.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    fn schemas(&self) -> Vec<ToolSchema> {
        self.schemas()
    }

    async fn execute(&self, name: &str, args: serde_json::Value) -> crate::tool::ToolResult {
        self.execute(name, args).await
    }

    fn apply_approval_decision(
        &self,
        action: &serde_json::Value,
        choice: ApprovalChoice,
    ) -> anyhow::Result<bool> {
        let Some(operation) = action.get("operation").and_then(|value| value.as_str()) else {
            return Ok(false);
        };

        if operation != "allow_folder" {
            return Ok(false);
        }

        let Some(folder) = action.get("folder").and_then(|value| value.as_str()) else {
            anyhow::bail!("approval action missing folder");
        };

        let Some(file_access) = &self.file_access else {
            anyhow::bail!("file access controller is unavailable");
        };

        let folder_path = std::path::Path::new(folder);

        match choice {
            ApprovalChoice::AllowOnce => {
                file_access
                    .allow_folder_once(folder_path)
                    .map_err(|err| anyhow::anyhow!(err))?;
                Ok(true)
            }
            ApprovalChoice::AllowSession => {
                file_access
                    .allow_folder_for_session(folder_path)
                    .map_err(|err| anyhow::anyhow!(err))?;
                Ok(true)
            }
            ApprovalChoice::Deny => Ok(false),
        }
    }

    fn is_non_blocking(&self, name: &str) -> bool {
        self.non_blocking_tools.contains(name)
    }
}

fn register<T: Tool + 'static>(tools: &mut HashMap<String, Arc<dyn Tool>>, name: &str, tool: T) {
    tools.insert(name.to_string(), Arc::new(tool));
}
