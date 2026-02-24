use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub struct FsRead;
pub struct FsWrite {
    workspace_root: PathBuf,
}
pub struct FsList;
pub struct FsGlob;
pub struct FsGrep;

#[async_trait]
impl Tool for FsRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read".to_string(),
            description: "Read a UTF-8 text file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        match std::fs::read_to_string(path) {
            Ok(content) => tool_ok(content),
            Err(err) => tool_err(err),
        }
    }
}

impl FsWrite {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn within_workspace(&self, path: &Path) -> bool {
        to_workspace_target(&self.workspace_root, path).starts_with(&self.workspace_root)
    }
}

#[async_trait]
impl Tool for FsWrite {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write".to_string(),
            description: "Write UTF-8 text to file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let path = PathBuf::from(
            args.get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        );
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if !self.within_workspace(&path) {
            return tool_err("write path is outside workspace");
        }

        let target = to_workspace_target(&self.workspace_root, &path);

        if let Some(parent) = target.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            return tool_err(err);
        }

        match std::fs::write(target, content) {
            Ok(_) => tool_ok("ok"),
            Err(err) => tool_err(err),
        }
    }
}

#[async_trait]
impl Tool for FsList {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list".to_string(),
            description: "List directory entries".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut names = Vec::new();
                for entry in entries.flatten() {
                    names.push(entry.path().display().to_string());
                }
                tool_ok(names.join("\n"))
            }
            Err(err) => tool_err(err),
        }
    }
}

#[async_trait]
impl Tool for FsGlob {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "glob".to_string(),
            description: "Glob files".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {"pattern": {"type": "string"}},
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let mut out = Vec::new();
        match glob::glob(pattern) {
            Ok(paths) => {
                for p in paths.flatten() {
                    out.push(p.display().to_string());
                }
                tool_ok(out.join("\n"))
            }
            Err(err) => tool_err(err),
        }
    }
}

#[async_trait]
impl Tool for FsGrep {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "grep".to_string(),
            description: "Search regex in files recursively".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "pattern": {"type": "string"}
                },
                "required": ["path", "pattern"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let root = PathBuf::from(args.get("path").and_then(|v| v.as_str()).unwrap_or("."));
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let re = match regex::Regex::new(pattern) {
            Ok(re) => re,
            Err(err) => return tool_err(err),
        };

        let mut results = Vec::new();
        if let Err(err) = walk_and_grep(&root, &re, &mut results) {
            return tool_err(err);
        }

        tool_ok(results.join("\n"))
    }
}

fn to_workspace_target(workspace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn tool_ok(output: impl Into<String>) -> ToolResult {
    ToolResult {
        is_error: false,
        output: output.into(),
    }
}

fn tool_err(err: impl ToString) -> ToolResult {
    ToolResult {
        is_error: true,
        output: err.to_string(),
    }
}

fn walk_and_grep(root: &Path, re: &regex::Regex, results: &mut Vec<String>) -> std::io::Result<()> {
    if root.is_file() {
        if let Ok(content) = std::fs::read_to_string(root) {
            for (idx, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    results.push(format!("{}:{}:{}", root.display(), idx + 1, line));
                }
            }
        }
        return Ok(());
    }

    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() || path.is_file() {
            let _ = walk_and_grep(&path, re, results);
        }
    }

    Ok(())
}
