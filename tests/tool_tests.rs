use hh::tool::Tool;
use hh::tool::bash::BashTool;
use hh::tool::fs::{FsRead, FsWrite};
use serde_json::json;

#[tokio::test]
async fn fs_write_respects_workspace_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let write_tool = FsWrite::new(temp.path().to_path_buf());

    let ok = write_tool
        .execute(json!({"path": "a.txt", "content": "hello"}))
        .await;
    assert!(!ok.is_error);

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
    assert_eq!(res.output, "content");
}

#[tokio::test]
async fn bash_tool_blocks_denylisted_command() {
    let bash = BashTool::new();
    let result = bash.execute(json!({"command": "rm -rf /"})).await;
    assert!(result.is_error);
    assert!(result.output.contains("blocked"));
}
