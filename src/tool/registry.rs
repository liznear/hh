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
            tools.insert("read".to_string(), Arc::new(FsRead));
            tools.insert(
                "write".to_string(),
                Arc::new(FsWrite::new(workspace_root.to_path_buf())),
            );
            tools.insert("list".to_string(), Arc::new(FsList));
            tools.insert("glob".to_string(), Arc::new(FsGlob));
            tools.insert("grep".to_string(), Arc::new(FsGrep));
        }

        if settings.tools.bash {
            tools.insert("bash".to_string(), Arc::new(BashTool::new()));
        }

        if settings.tools.web {
            tools.insert("web_fetch".to_string(), Arc::new(WebFetchTool::new()));
            tools.insert("web_search".to_string(), Arc::new(WebSearchTool::new()));
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
