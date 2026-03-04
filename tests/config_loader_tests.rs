use hh_cli::config::{
    claude_local_config_path, claude_project_config_path, global_config_path,
    global_instructions_path_for_home, load_settings, local_config_path, project_config_path,
    project_instructions_path, upsert_local_permission_rule, write_default_project_config,
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
fn instruction_paths_use_priority_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    std::fs::create_dir_all(home.join(".config/hh")).expect("create hh config dir");
    std::fs::create_dir_all(home.join(".agents")).expect("create agents dir");
    std::fs::create_dir_all(home.join(".config/claude")).expect("create claude config dir");

    std::fs::write(home.join(".agents/AGENTS.md"), "fallback").expect("write fallback");
    std::fs::write(home.join(".config/claude/CLAUDE.md"), "claude").expect("write claude");
    assert_eq!(
        global_instructions_path_for_home(&home),
        Some(home.join(".agents/AGENTS.md"))
    );

    std::fs::write(home.join(".config/hh/AGENTS.md"), "primary").expect("write primary");
    assert_eq!(
        global_instructions_path_for_home(&home),
        Some(home.join(".config/hh/AGENTS.md"))
    );

    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    std::fs::write(cwd.join("CLAUDE.md"), "project fallback").expect("write project fallback");
    assert_eq!(project_instructions_path(&cwd), Some(cwd.join("CLAUDE.md")));

    std::fs::write(cwd.join("AGENTS.md"), "project primary").expect("write project primary");
    assert_eq!(project_instructions_path(&cwd), Some(cwd.join("AGENTS.md")));
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
fn load_settings_appends_project_instructions_to_system_prompt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");
    std::fs::write(cwd.join("AGENTS.md"), "project instruction body").expect("write AGENTS.md");

    let loaded = load_settings(&cwd, None).expect("load settings");
    let prompt = loaded.agent.resolved_system_prompt();

    assert!(prompt.contains("project instruction body"));
    assert!(prompt.contains("<Project instructions path=\""));
    assert!(prompt.contains(&cwd.join("AGENTS.md").display().to_string()));
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
