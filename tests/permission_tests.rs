use hh_cli::config::settings::{PermissionSettings, Settings};
use hh_cli::permission::{Decision, PermissionMatcher};
use hh_cli::tool::registry::ToolRegistry;
use serde_json::json;

#[test]
fn default_permission_matrix_matches_policy() {
    let settings = Settings::default();
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &cwd);

    assert_eq!(matcher.decision_for_tool("read"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("list"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("glob"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("grep"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("write"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("edit"), Decision::Ask);
    assert_eq!(matcher.decision_for_tool("todo_read"), Decision::Allow);
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
    assert_eq!(parsed.todo_read, "allow");
    assert_eq!(parsed.todo_write, "allow");
    assert!(parsed.capabilities.is_empty());
    assert!(parsed.allow.is_empty());
    assert!(parsed.ask.is_empty());
    assert!(parsed.deny.is_empty());
}

#[test]
fn capability_override_applies_to_all_matching_tools() {
    let mut settings = Settings::default();
    settings
        .permissions
        .capabilities
        .insert("web".to_string(), "allow".to_string());

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &cwd);

    assert_eq!(matcher.decision_for_tool("web_fetch"), Decision::Allow);
    assert_eq!(matcher.decision_for_tool("web_search"), Decision::Allow);
}

#[test]
fn rule_syntax_applies_deny_then_ask_then_allow() {
    let mut settings = Settings::default();
    settings.permissions.allow = vec!["Bash(git *)".to_string()];
    settings.permissions.ask = vec!["Bash(git push *)".to_string()];
    settings.permissions.deny = vec!["Bash(git push origin main)".to_string()];

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &cwd);

    assert_eq!(
        matcher.decision_for_tool_call("bash", &json!({"command": "git push origin main"})),
        Decision::Deny
    );
    assert_eq!(
        matcher.decision_for_tool_call("bash", &json!({"command": "git push origin dev"})),
        Decision::Ask
    );
    assert_eq!(
        matcher.decision_for_tool_call("bash", &json!({"command": "git status"})),
        Decision::Allow
    );
}

#[test]
fn bash_wildcard_rule_matches_expected_commands() {
    let mut settings = Settings::default();
    settings.permissions.allow = vec!["Bash(ls *)".to_string()];
    settings.permissions.bash = "deny".to_string();

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &cwd);

    assert_eq!(
        matcher.decision_for_tool_call("bash", &json!({"command": "ls -la"})),
        Decision::Allow
    );
    assert_eq!(
        matcher.decision_for_tool_call("bash", &json!({"command": "lsof"})),
        Decision::Deny
    );
}

#[test]
fn persisted_local_bash_allow_rule_in_parent_workspace_applies_from_subdirectory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    let nested = workspace.join("nested/project");
    std::fs::create_dir_all(workspace.join(".hh")).expect("create config dir");
    std::fs::create_dir_all(&nested).expect("create nested cwd");

    std::fs::write(
        workspace.join(".hh/config.local.json"),
        r#"{
  "permissions": {
    "allow": ["Bash(ls -al*)"]
  }
}
"#,
    )
    .expect("write local config");

    let settings = hh_cli::config::load_settings(&nested, None).expect("load settings");
    let registry = ToolRegistry::new(&settings, &nested);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &nested);

    assert_eq!(
        matcher.decision_for_tool_call("bash", &json!({"command": "ls -alh"})),
        Decision::Allow
    );
}

#[test]
fn read_edit_rules_support_project_and_absolute_patterns() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(cwd.join("src")).expect("create src");
    std::fs::write(cwd.join("src/main.rs"), "fn main() {}\n").expect("seed file");

    let absolute_file = temp.path().join("outside.txt");
    std::fs::write(&absolute_file, "outside\n").expect("seed outside file");

    let mut settings = Settings::default();
    settings.permissions.read = "deny".to_string();
    settings.permissions.edit = "deny".to_string();
    settings.permissions.allow = vec![
        "Read(/src/**)".to_string(),
        format!("Read(//{})", absolute_file.display()),
        "Edit(/src/**)".to_string(),
    ];

    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &cwd);

    assert_eq!(
        matcher.decision_for_tool_call(
            "read",
            &json!({"path": cwd.join("src/main.rs").display().to_string()})
        ),
        Decision::Allow
    );
    assert_eq!(
        matcher.decision_for_tool_call(
            "read",
            &json!({"path": absolute_file.display().to_string()})
        ),
        Decision::Allow
    );
    assert_eq!(
        matcher.decision_for_tool_call(
            "edit",
            &json!({
                "path": cwd.join("src/main.rs").display().to_string(),
                "old_string": "fn main() {}",
                "new_string": "fn main() { println!(\"hi\"); }"
            })
        ),
        Decision::Allow
    );
}

#[test]
fn webfetch_domain_rule_matches_base_and_subdomain() {
    let mut settings = Settings::default();
    settings.permissions.web = "deny".to_string();
    settings.permissions.allow = vec!["WebFetch(domain:example.com)".to_string()];

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    let registry = ToolRegistry::new(&settings, &cwd);
    let schemas = registry.schemas();
    let matcher = PermissionMatcher::new(settings, &schemas, &cwd);

    assert_eq!(
        matcher.decision_for_tool_call("web_fetch", &json!({"url": "https://example.com"})),
        Decision::Allow
    );
    assert_eq!(
        matcher.decision_for_tool_call("web_fetch", &json!({"url": "https://api.example.com"})),
        Decision::Allow
    );
    assert_eq!(
        matcher.decision_for_tool_call("web_fetch", &json!({"url": "https://other.com"})),
        Decision::Deny
    );
}
