use hh::config::settings::Settings;
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
    assert_eq!(matcher.decision_for_tool("bash"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("web_fetch"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("unknown"), Decision::Deny);
}
