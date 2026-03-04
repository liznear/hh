use hh_cli::core::{Message, Role};
use hh_cli::session::{SessionEvent, SessionStore};

#[test]
fn appends_and_replays_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");

    let store = SessionStore::new(temp.path(), &cwd, None, Some("test session".to_string()))
        .expect("store");

    let msg1 = Message {
        role: Role::User,
        content: "hello".to_string(),
        attachments: Vec::new(),
        tool_call_id: None,
        tool_calls: Vec::new(),
    };
    store
        .append(&SessionEvent::Message {
            id: "1".to_string(),
            message: msg1.clone(),
        })
        .expect("append user");

    let msg2 = Message {
        role: Role::Assistant,
        content: "hi".to_string(),
        attachments: Vec::new(),
        tool_call_id: None,
        tool_calls: Vec::new(),
    };
    store
        .append(&SessionEvent::Message {
            id: "2".to_string(),
            message: msg2.clone(),
        })
        .expect("append assistant");

    store
        .append(&SessionEvent::Thinking {
            id: "3".to_string(),
            content: "hidden reasoning".to_string(),
        })
        .expect("append thinking");

    let replayed = store.replay_messages().expect("replay");
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].content, "hello");
    assert_eq!(replayed[1].content, "hi");

    // Test listing and resuming
    let sessions = SessionStore::list(temp.path(), &cwd).expect("list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "test session");

    let resume_store =
        SessionStore::new(temp.path(), &cwd, Some(&sessions[0].id), None).expect("resume store");
    let resumed_msgs = resume_store.replay_messages().expect("resume replay");
    assert_eq!(resumed_msgs.len(), 2);
    assert_eq!(resumed_msgs[0].content, "hello");
}

#[test]
fn replays_legacy_message_event_shape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");

    // Create legacy file manually
    // workspace_key for cwd
    let key = cwd.display().to_string().replace('/', "_");
    let legacy_file = temp.path().join(format!("{}.jsonl", key));
    let legacy_content = "{\"event\":\"message\",\"id\":\"1\",\"role\":\"user\",\"content\":\"legacy\",\"tool_call_id\":null}\n";
    std::fs::write(&legacy_file, legacy_content).expect("write legacy file");

    // New store should detect migration. We call new() with no ID to trigger the check/migration logic.
    let _ = SessionStore::new(temp.path(), &cwd, None, None).expect("store migration");

    // Check that legacy file is gone
    assert!(!legacy_file.exists());

    // Check metadata exists and we can find the migrated session
    let sessions = SessionStore::list(temp.path(), &cwd).expect("list migrated");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "Migrated Session");

    // Load the migrated session
    let store =
        SessionStore::new(temp.path(), &cwd, Some(&sessions[0].id), None).expect("load migrated");

    let replayed = store.replay_messages().expect("replay");
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].role, Role::User);
    assert_eq!(replayed[0].content, "legacy");
}

#[test]
fn replay_messages_uses_latest_compact_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");

    let store = SessionStore::new(temp.path(), &cwd, None, Some("compact session".to_string()))
        .expect("store");

    store
        .append(&SessionEvent::Message {
            id: "1".to_string(),
            message: Message {
                role: Role::User,
                content: "before compact".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
        })
        .expect("append before compact");

    store
        .append(&SessionEvent::Compact {
            id: "2".to_string(),
            summary: "compacted summary".to_string(),
        })
        .expect("append compact marker");

    store
        .append(&SessionEvent::Message {
            id: "3".to_string(),
            message: Message {
                role: Role::Assistant,
                content: "after compact".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
        })
        .expect("append after compact");

    let replayed = store.replay_messages().expect("replay");
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].content, "compacted summary");
    assert_eq!(replayed[1].content, "after compact");
}
