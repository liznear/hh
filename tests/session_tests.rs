use hh::provider::Role;
use hh::session::{SessionEvent, SessionStore};

#[test]
fn appends_and_replays_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("workspace");
    std::fs::create_dir_all(&cwd).expect("create workspace");

    let store = SessionStore::for_workspace(temp.path(), &cwd).expect("store");

    store
        .append(&SessionEvent::Message {
            id: "1".to_string(),
            role: Role::User,
            content: "hello".to_string(),
            tool_call_id: None,
        })
        .expect("append user");

    store
        .append(&SessionEvent::Message {
            id: "2".to_string(),
            role: Role::Assistant,
            content: "hi".to_string(),
            tool_call_id: None,
        })
        .expect("append assistant");

    let replayed = store.replay_messages().expect("replay");
    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].content, "hello");
    assert_eq!(replayed[1].content, "hi");
}
