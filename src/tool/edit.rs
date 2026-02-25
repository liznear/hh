use crate::tool::fs::resolve_workspace_target;
use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::path::PathBuf;

pub struct EditTool {
    workspace_root: PathBuf,
}

impl EditTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

#[derive(Debug, Deserialize)]
struct EditArgs {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[derive(Debug, Serialize)]
struct EditSummary {
    added_lines: usize,
    removed_lines: usize,
}

#[derive(Debug, Serialize)]
struct EditOutput {
    path: String,
    applied: bool,
    summary: EditSummary,
    diff: String,
}

#[async_trait]
impl Tool for EditTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "edit".to_string(),
            description: "Edit a file by replacing an exact string".to_string(),
            capability: Some("edit".to_string()),
            mutating: Some(true),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string"},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean"}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let parsed: EditArgs = match serde_json::from_value(args) {
            Ok(value) => value,
            Err(err) => return tool_err(format!("invalid edit args: {err}")),
        };

        if parsed.old_string.is_empty() {
            return tool_err("old_string must not be empty");
        }

        let input_path = PathBuf::from(&parsed.path);
        let target = match resolve_workspace_target(&self.workspace_root, &input_path) {
            Ok(path) => path,
            Err(err) => return tool_err(err),
        };

        let before = match std::fs::read_to_string(&target) {
            Ok(content) => content,
            Err(err) => return tool_err(format!("failed to read file: {err}")),
        };

        let matches = before.matches(&parsed.old_string).count();
        if matches == 0 {
            return tool_err("old_string not found in file");
        }
        if !parsed.replace_all && matches > 1 {
            return tool_err(
                "old_string is not unique; set replace_all=true to replace all matches",
            );
        }

        let after = if parsed.replace_all {
            before.replace(&parsed.old_string, &parsed.new_string)
        } else {
            before.replacen(&parsed.old_string, &parsed.new_string, 1)
        };

        let applied = before != after;

        if applied && let Err(err) = std::fs::write(&target, &after) {
            return tool_err(format!("failed to write file: {err}"));
        }

        let diff = TextDiff::from_lines(&before, &after);
        let mut added_lines = 0;
        let mut removed_lines = 0;
        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Insert => added_lines += 1,
                ChangeTag::Delete => removed_lines += 1,
                ChangeTag::Equal => {}
            }
        }

        let unified = diff
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{}", parsed.path), &format!("b/{}", parsed.path))
            .to_string();

        let output = EditOutput {
            path: parsed.path,
            applied,
            summary: EditSummary {
                added_lines,
                removed_lines,
            },
            diff: unified,
        };

        tool_ok_json(&output)
    }
}

fn tool_ok_json(output: &impl Serialize) -> ToolResult {
    match serde_json::to_value(output) {
        Ok(value) => ToolResult::ok_json("ok", value),
        Err(err) => tool_err(format!("failed to serialize output: {err}")),
    }
}

fn tool_err(err: impl ToString) -> ToolResult {
    ToolResult::err_text("error", err.to_string())
}
