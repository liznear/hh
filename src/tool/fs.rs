use crate::tool::diff::build_unified_line_diff;
use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};
use tokio::process::Command;

pub struct FsRead;
pub struct FsWrite {
    workspace_root: PathBuf,
}
pub struct FsList;
pub struct FsGlob;
pub struct FsGrep;

#[derive(Debug, Serialize)]
struct FileWriteSummary {
    added_lines: usize,
    removed_lines: usize,
}

#[derive(Debug, Serialize)]
struct FileWriteOutput {
    path: String,
    applied: bool,
    summary: FileWriteSummary,
    diff: String,
}

#[derive(Debug, Serialize)]
struct FileReadOutput {
    path: String,
    bytes: usize,
    lines: usize,
    content: String,
}

#[derive(Debug, Serialize)]
struct ListOutput {
    path: String,
    count: usize,
    entries: Vec<String>,
}

#[derive(Debug, Serialize)]
struct GlobOutput {
    pattern: String,
    count: usize,
    matches: Vec<String>,
}

#[derive(Debug, Serialize)]
struct GrepOutput {
    path: String,
    pattern: String,
    include: Option<String>,
    count: usize,
    shown_count: usize,
    truncated: bool,
    has_errors: bool,
    matches: Vec<String>,
}

#[async_trait]
impl Tool for FsRead {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read".to_string(),
            description: "Read a UTF-8 text file".to_string(),
            capability: Some("read".to_string()),
            mutating: Some(false),
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
            Ok(content) => {
                let output = FileReadOutput {
                    path: path.to_string(),
                    bytes: content.len(),
                    lines: content.lines().count(),
                    content,
                };
                ToolResult::ok_json_serializable("ok", &output)
            }
            Err(err) => ToolResult::error(err.to_string()),
        }
    }
}

impl FsWrite {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

#[async_trait]
impl Tool for FsWrite {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write".to_string(),
            description: "Write UTF-8 text to file".to_string(),
            capability: Some("write".to_string()),
            mutating: Some(true),
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
        let raw_path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let path = PathBuf::from(&raw_path);
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let target = match resolve_workspace_target(&self.workspace_root, &path) {
            Ok(path) => path,
            Err(err) => return ToolResult::error(err),
        };

        if let Some(parent) = target.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            return ToolResult::error(err.to_string());
        }

        let before = if target.exists() {
            match std::fs::read_to_string(&target) {
                Ok(text) => text,
                Err(err) => {
                    return ToolResult::error(format!(
                        "failed to read existing file before write: {err}"
                    ));
                }
            }
        } else {
            String::new()
        };

        match std::fs::write(target, content) {
            Ok(_) => {
                let diff = build_unified_line_diff(before.as_str(), content, &raw_path);
                let output = FileWriteOutput {
                    path: raw_path,
                    applied: before != content,
                    summary: FileWriteSummary {
                        added_lines: diff.added_lines,
                        removed_lines: diff.removed_lines,
                    },
                    diff: diff.unified,
                };

                ToolResult::ok_json_serializable("ok", &output)
            }
            Err(err) => ToolResult::error(err.to_string()),
        }
    }
}

#[async_trait]
impl Tool for FsList {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list".to_string(),
            description: "List directory entries".to_string(),
            capability: Some("list".to_string()),
            mutating: Some(false),
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
                let mut entries_list = Vec::new();
                for entry in entries.flatten() {
                    entries_list.push(entry.path().display().to_string());
                }
                let output = ListOutput {
                    path: path.to_string(),
                    count: entries_list.len(),
                    entries: entries_list,
                };
                ToolResult::ok_json_serializable("ok", &output)
            }
            Err(err) => ToolResult::error(err.to_string()),
        }
    }
}

#[async_trait]
impl Tool for FsGlob {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "glob".to_string(),
            description: "Glob files".to_string(),
            capability: Some("glob".to_string()),
            mutating: Some(false),
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
        let mut matches = Vec::new();
        match glob::glob(pattern) {
            Ok(paths) => {
                for p in paths.flatten() {
                    matches.push(p.display().to_string());
                }
                let output = GlobOutput {
                    pattern: pattern.to_string(),
                    count: matches.len(),
                    matches,
                };
                ToolResult::ok_json_serializable("ok", &output)
            }
            Err(err) => ToolResult::error(err.to_string()),
        }
    }
}

#[async_trait]
impl Tool for FsGrep {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "grep".to_string(),
            description: "Search regex in files recursively".to_string(),
            capability: Some("grep".to_string()),
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "pattern": {"type": "string"},
                    "include": {"type": "string"}
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let root = PathBuf::from(args.get("path").and_then(|v| v.as_str()).unwrap_or("."));
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let include = args
            .get("include")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);

        let mut command = Command::new("rg");
        command
            .arg("-nH")
            .arg("--hidden")
            .arg("--no-heading")
            .arg("--no-messages")
            .arg("--color")
            .arg("never")
            .arg("--regexp")
            .arg(pattern);
        if let Some(include) = include.as_deref() {
            command.arg("--glob").arg(include);
        }
        command.arg(&root);

        let output = match command.output().await {
            Ok(output) => output,
            Err(err) => {
                return ToolResult::error(format!("failed to run rg: {err}"));
            }
        };

        let all_results = if output.stdout.is_empty() {
            Vec::new()
        } else {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(ToOwned::to_owned)
                .collect()
        };

        let code = output.status.code().unwrap_or_default();
        if !output.status.success() && code != 1 && (code != 2 || all_results.is_empty()) {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.is_empty() {
                return ToolResult::error(format!("rg exited with code {code}"));
            }
            return ToolResult::error(stderr);
        }

        let has_errors = code == 2;
        let total_count = all_results.len();
        let limit = 100;
        let truncated = total_count > limit;
        let results = if truncated {
            all_results.into_iter().take(limit).collect()
        } else {
            all_results
        };
        let shown_count = results.len();

        let output = GrepOutput {
            path: root.display().to_string(),
            pattern: pattern.to_string(),
            include,
            count: total_count,
            shown_count,
            truncated,
            has_errors,
            matches: results,
        };

        ToolResult::ok_json_serializable("ok", &output)
    }
}

pub(crate) fn to_workspace_target(workspace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

pub(crate) fn resolve_workspace_target(
    workspace_root: &Path,
    path: &Path,
) -> Result<PathBuf, String> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("path must not contain parent directory traversal".to_string());
    }

    let workspace_root = std::fs::canonicalize(workspace_root)
        .map_err(|err| format!("failed to resolve workspace root: {err}"))?;
    let target = to_workspace_target(&workspace_root, path);

    let checked_target = if target.exists() {
        std::fs::canonicalize(&target)
            .map_err(|err| format!("failed to resolve target path: {err}"))?
    } else {
        let parent = target
            .parent()
            .ok_or_else(|| "target path has no parent directory".to_string())?;
        let canonical_parent = std::fs::canonicalize(parent)
            .map_err(|err| format!("failed to resolve target parent: {err}"))?;
        let file_name = target
            .file_name()
            .ok_or_else(|| "target path has no file name".to_string())?;
        canonical_parent.join(file_name)
    };

    if !checked_target.starts_with(&workspace_root) {
        return Err("path is outside workspace".to_string());
    }

    Ok(target)
}
