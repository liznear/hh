use hh::config::settings::{PermissionSettings, Settings};
use hh::permission::{Decision, PermissionMatcher};

#[test]
fn default_permission_matrix_matches_policy() {
    let settings = Settings::default();
    let matcher = PermissionMatcher::new(settings);

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
    let parsed: PermissionSettings = toml::from_str(
        r#"
read = "allow"
list = "allow"
glob = "allow"
grep = "allow"
write = "ask"
bash = "ask"
web = "ask"
"#,
    )
    .expect("parse permission settings");

    assert_eq!(parsed.edit, "ask");
    assert_eq!(parsed.todo_write, "allow");
}
