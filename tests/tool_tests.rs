use hh::tool::Tool;
use hh::tool::bash::BashTool;
use hh::tool::edit::EditTool;
use hh::tool::fs::{FsRead, FsWrite};
use hh::tool::todo::TodoWriteTool;
use serde_json::json;

#[tokio::test]
async fn fs_write_respects_workspace_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let write_tool = FsWrite::new(temp.path().to_path_buf());

    let ok = write_tool
        .execute(json!({"path": "a.txt", "content": "hello"}))
        .await;
    assert!(!ok.is_error);
    let ok_output: serde_json::Value = serde_json::from_str(&ok.output).expect("write json output");
    assert_eq!(ok_output["path"], "a.txt");
    assert!(
        ok_output["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+hello")
    );

    let overwrite = write_tool
        .execute(json!({"path": "a.txt", "content": "hello world"}))
        .await;
    assert!(!overwrite.is_error);
    let overwrite_output: serde_json::Value =
        serde_json::from_str(&overwrite.output).expect("overwrite json output");
    assert!(
        overwrite_output["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("-hello")
    );
    assert!(
        overwrite_output["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+hello world")
    );

    let blocked = write_tool
        .execute(json!({"path": "/tmp/outside.txt", "content": "bad"}))
        .await;
    assert!(blocked.is_error);
}

#[tokio::test]
async fn fs_read_returns_file_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("note.txt");
    std::fs::write(&path, "content").expect("write file");

    let read_tool = FsRead;
    let res = read_tool
        .execute(json!({"path": path.display().to_string()}))
        .await;

    assert!(!res.is_error);
    let output: serde_json::Value = serde_json::from_str(&res.output).expect("json output");
    assert_eq!(output["content"], "content");
    assert_eq!(output["bytes"], 7);
    assert_eq!(output["lines"], 1);
}

#[tokio::test]
async fn bash_tool_blocks_denylisted_command() {
    let bash = BashTool::new();
    let result = bash.execute(json!({"command": "rm -rf /"})).await;
    assert!(result.is_error);
    let output: serde_json::Value = serde_json::from_str(&result.output).expect("json output");
    assert_eq!(output["status"], "blocked");
    assert_eq!(output["ok"], false);
    assert!(output["error"].as_str().unwrap_or_default().contains("blocked"));
}

#[tokio::test]
async fn bash_tool_reports_exit_code_and_streams() {
    let bash = BashTool::new();
    let result = bash
        .execute(json!({"command": "printf 'ok'; printf 'warn' 1>&2"}))
        .await;

    assert!(!result.is_error);
    let output: serde_json::Value = serde_json::from_str(&result.output).expect("json output");
    assert_eq!(output["status"], "success");
    assert_eq!(output["ok"], true);
    assert_eq!(output["exit_code"], 0);
    assert_eq!(output["stdout"], "ok");
    assert_eq!(output["stderr"], "warn");
}

#[tokio::test]
async fn todo_write_set_updates_list() {
    let todo = TodoWriteTool;
    let result = todo
        .execute(json!({
            "todos": [
                {"content": "Ship feature", "status": "in_progress", "priority": "high"},
                {"content": "Write tests", "status": "pending", "priority": "medium"}
            ]
        }))
        .await;

    assert!(!result.is_error);
    let output: serde_json::Value = serde_json::from_str(&result.output).expect("json output");
    assert_eq!(output["counts"]["total"], 2);
    assert_eq!(output["counts"]["in_progress"], 1);
}

#[tokio::test]
async fn todo_write_rejects_invalid_args() {
    let todo = TodoWriteTool;
    let result = todo
        .execute(json!({
            "todos": [
                {"content": "", "status": "active", "priority": "critical"}
            ]
        }))
        .await;

    assert!(result.is_error);
}

#[tokio::test]
async fn edit_applies_single_replacement() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("note.txt");
    std::fs::write(&path, "hello world\n").expect("seed file");
    let edit = EditTool::new(temp.path().to_path_buf());

    let result = edit
        .execute(json!({
            "path": "note.txt",
            "old_string": "world",
            "new_string": "rust"
        }))
        .await;

    assert!(!result.is_error);
    let updated = std::fs::read_to_string(&path).expect("read updated");
    assert_eq!(updated, "hello rust\n");

    let output: serde_json::Value = serde_json::from_str(&result.output).expect("json output");
    assert_eq!(output["applied"], true);
    assert!(
        output["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+hello rust")
    );
}

#[tokio::test]
async fn edit_replace_all() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("repeat.txt");
    std::fs::write(&path, "a\na\n").expect("seed file");
    let edit = EditTool::new(temp.path().to_path_buf());

    let result = edit
        .execute(json!({
            "path": "repeat.txt",
            "old_string": "a",
            "new_string": "b",
            "replace_all": true
        }))
        .await;

    assert!(!result.is_error);
    let updated = std::fs::read_to_string(&path).expect("read updated");
    assert_eq!(updated, "b\nb\n");
}

#[tokio::test]
async fn edit_errors_when_old_string_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("missing.txt");
    std::fs::write(&path, "hello\n").expect("seed file");
    let edit = EditTool::new(temp.path().to_path_buf());

    let result = edit
        .execute(json!({
            "path": "missing.txt",
            "old_string": "world",
            "new_string": "rust"
        }))
        .await;

    assert!(result.is_error);
    assert!(result.output.contains("not found"));
}

#[tokio::test]
async fn edit_errors_on_non_unique_match_when_replace_all_false() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("non_unique.txt");
    std::fs::write(&path, "x\nx\n").expect("seed file");
    let edit = EditTool::new(temp.path().to_path_buf());

    let result = edit
        .execute(json!({
            "path": "non_unique.txt",
            "old_string": "x",
            "new_string": "y"
        }))
        .await;

    assert!(result.is_error);
    assert!(result.output.contains("not unique"));
}

#[tokio::test]
async fn edit_respects_workspace_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::NamedTempFile::new().expect("outside file");
    let edit = EditTool::new(temp.path().to_path_buf());

    let result = edit
        .execute(json!({
            "path": outside.path().display().to_string(),
            "old_string": "x",
            "new_string": "y"
        }))
        .await;

    assert!(result.is_error);
    assert!(result.output.contains("outside workspace"));
}

#[tokio::test]
async fn edit_rejects_parent_traversal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let edit = EditTool::new(temp.path().to_path_buf());

    let result = edit
        .execute(json!({
            "path": "../escape.txt",
            "old_string": "x",
            "new_string": "y"
        }))
        .await;

    assert!(result.is_error);
    assert!(result.output.contains("parent directory traversal"));
}

#[cfg(unix)]
#[tokio::test]
async fn edit_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside_root = tempfile::tempdir().expect("outside tempdir");
    let outside_file = outside_root.path().join("outside.txt");
    std::fs::write(&outside_file, "hello\n").expect("seed outside");

    let symlink_path = temp.path().join("link.txt");
    symlink(&outside_file, &symlink_path).expect("create symlink");

    let edit = EditTool::new(temp.path().to_path_buf());
    let result = edit
        .execute(json!({
            "path": "link.txt",
            "old_string": "hello",
            "new_string": "bye"
        }))
        .await;

    assert!(result.is_error);
    assert!(result.output.contains("outside workspace"));
}
