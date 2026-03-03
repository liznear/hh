use hh_cli::config::{
    claude_local_config_path, claude_project_config_path, global_config_path, load_settings,
    local_config_path, project_config_path, upsert_local_permission_rule,
    write_default_project_config,
};

#[test]
fn config_paths_use_settings_json_locations() {
    let home = dirs::home_dir().expect("home");
    assert_eq!(global_config_path(), home.join(".config/hh/config.json"));

    let cwd = std::path::Path::new("/tmp/my-project");
    assert_eq!(
        claude_project_config_path(cwd),
        cwd.join(".claude/settings.json")
    );
    assert_eq!(project_config_path(cwd), cwd.join(".hh/config.json"));
    assert_eq!(
        claude_local_config_path(cwd),
        cwd.join(".claude/settings.local.json")
    );
    assert_eq!(local_config_path(cwd), cwd.join(".hh/config.local.json"));
}

#[test]
fn write_default_project_config_uses_config_json_filename() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = write_default_project_config(temp.path()).expect("write default config");
    assert!(path.ends_with(".hh/config.json"));
    assert!(path.exists());
}

#[test]
fn load_settings_merges_project_and_local_permissions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(cwd.join(".hh")).expect("create .hh");

    std::fs::write(
        cwd.join(".hh/config.json"),
        r#"{
  "permissions": {
    "allow": ["Bash(git status)"]
  }
}"#,
    )
    .expect("write shared settings");

    std::fs::write(
        cwd.join(".hh/config.local.json"),
        r#"{
  "permissions": {
    "deny": ["Bash(git status)"]
  }
}"#,
    )
    .expect("write local settings");

    let loaded = load_settings(&cwd, None).expect("load settings");
    assert!(
        loaded
            .permissions
            .allow
            .contains(&"Bash(git status)".to_string())
    );
    assert!(
        loaded
            .permissions
            .deny
            .contains(&"Bash(git status)".to_string())
    );
}

#[test]
fn load_settings_prioritizes_hh_over_claude_for_project_and_local() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(cwd.join(".claude")).expect("create .claude");
    std::fs::create_dir_all(cwd.join(".hh")).expect("create .hh");

    std::fs::write(
        cwd.join(".claude/settings.json"),
        r#"{
  "permissions": {
    "bash": "allow"
  }
}"#,
    )
    .expect("write claude project settings");

    std::fs::write(
        cwd.join(".hh/config.json"),
        r#"{
  "permissions": {
    "bash": "ask"
  }
}"#,
    )
    .expect("write hh project settings");

    std::fs::write(
        cwd.join(".claude/settings.local.json"),
        r#"{
  "permissions": {
    "bash": "allow"
  }
}"#,
    )
    .expect("write claude local settings");

    std::fs::write(
        cwd.join(".hh/config.local.json"),
        r#"{
  "permissions": {
    "bash": "deny"
  }
}"#,
    )
    .expect("write hh local settings");

    let loaded = load_settings(&cwd, None).expect("load settings");
    assert_eq!(loaded.permissions.bash, "deny");
}

#[test]
fn upsert_local_permission_rule_writes_hh_local_config() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");

    upsert_local_permission_rule(&cwd, "allow", "Bash(git status*)").expect("upsert allow");
    upsert_local_permission_rule(&cwd, "deny", "Bash(git push*)").expect("upsert deny");

    let raw = std::fs::read_to_string(cwd.join(".hh/config.local.json")).expect("read config");
    assert!(raw.contains("Bash(git status*)"));
    assert!(raw.contains("Bash(git push*)"));
}
