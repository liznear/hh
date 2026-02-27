use hh::config::settings::{PermissionSettings, Settings};
use hh::permission::{Decision, PermissionMatcher};
use hh::tool::registry::ToolRegistry;

#[test]
fn default_permission_matrix_matches_policy() {
    let settings = Settings::default();
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas);

    assert_eq!(matcher.decision_for_tool("read"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("list"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("glob"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("grep"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("write"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("edit"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("todo_write"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("bash"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("web_fetch"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("web_search"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("unknown"), Decision::Deny);
}

#[test]
fn permission_settings_backfill_new_tool_defaults() {
    let parsed: PermissionSettings = serde_json::from_str(
        r#"{
            "read": "allow",
            "list": "allow",
            "glob": "allow",
            "grep": "allow",
            "write": "ask",
            "bash": "ask",
            "web": "ask"
        }"#,
    )
    .expect("parse permission settings");

    assert_eq!(parsed.edit, "ask");
    assert_eq!(parsed.todo_write, "allow");
    assert!(parsed.capabilities.is_empty());
}

#[test]
fn capability_override_applies_to_all_matching_tools() {
    let mut settings = Settings::default();
    settings
        .permission
        .capabilities
        .insert("web".to_string(), "allow".to_string());

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas);

    assert_eq!(matcher.decision_for_tool("web_fetch"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("web_search"), Decision::Allow);
}
