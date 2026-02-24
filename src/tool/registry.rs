use crate::config::Settings;
use crate::tool::bash::BashTool;
use crate::tool::fs::{FsGlob, FsGrep, FsList, FsRead, FsWrite};
use crate::tool::web::{WebFetchTool, WebSearchTool};
use crate::tool::{Tool, ToolSchema};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new(settings: &Settings, workspace_root: &Path) -> Self {
        let mut tools: HashMap<String, Arc<dyn Tool>> = HashMap::new();

        if settings.tools.fs {
            register(&mut tools, "read", FsRead);
            register(
                &mut tools,
                "write",
                FsWrite::new(workspace_root.to_path_buf()),
            );
            register(&mut tools, "list", FsList);
            register(&mut tools, "glob", FsGlob);
            register(&mut tools, "grep", FsGrep);
        }

        if settings.tools.bash {
            register(&mut tools, "bash", BashTool::new());
        }

        if settings.tools.web {
            register(&mut tools, "web_fetch", WebFetchTool::new());
            register(&mut tools, "web_search", WebSearchTool::new());
        }

        Self { tools }
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value) -> crate::tool::ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.execute(args).await,
            None => crate::tool::ToolResult {
                is_error: true,
                output: format!("unknown tool: {}", name),
            },
        }
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.tools.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }
}

fn register<T: Tool + 'static>(tools: &mut HashMap<String, Arc<dyn Tool>>, name: &str, tool: T) {
    tools.insert(name.to_string(), Arc::new(tool));
}
