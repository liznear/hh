use async_trait::async_trait;
use hh::config::settings::Settings;
use hh::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use hh::permission::PermissionMatcher;
use hh::provider::{
    Message, Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent, Role, ToolCall,
};
use hh::session::{SessionEvent, SessionStore};
use hh::tool::registry::ToolRegistry;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct MockProvider {
    responses: Arc<Mutex<Vec<ProviderResponse>>>,
    stream_events: Vec<ProviderStreamEvent>,
    requests: Arc<Mutex<Vec<ProviderRequest>>>,
}

#[async_trait]
impl Provider for MockProvider {
    async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
        self.requests.lock().expect("requests").push(_req);
        let mut lock = self.responses.lock().expect("lock");
        if lock.is_empty() {
            anyhow::bail!("no response queued")
        }
        Ok(lock.remove(0))
    }

    async fn complete_stream<F>(
        &self,
        req: ProviderRequest,
        mut on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send,
    {
        for event in &self.stream_events {
            on_event(event.clone());
        }
        self.complete(req).await
    }
}

#[derive(Clone, Default)]
struct RecordingEvents {
    log: Arc<Mutex<Vec<String>>>,
}

impl RecordingEvents {
    fn entries(&self) -> Vec<String> {
        self.log.lock().expect("log").clone()
    }
}

impl AgentEvents for RecordingEvents {
    fn on_thinking(&self, text: &str) {
        self.log
            .lock()
            .expect("log")
            .push(format!("thinking:{text}"));
    }

    fn on_tool_start(&self, name: &str, _args: &serde_json::Value) {
        self.log
            .lock()
            .expect("log")
            .push(format!("tool_start:{name}"));
    }

    fn on_tool_end(&self, name: &str, result: &hh::tool::ToolResult) {
        self.log
            .lock()
            .expect("log")
            .push(format!("tool_end:{name}:{}", result.is_error));
    }

    fn on_assistant_delta(&self, delta: &str) {
        self.log.lock().expect("log").push(format!("delta:{delta}"));
    }

    fn on_assistant_done(&self) {
        self.log.lock().expect("log").push("done".to_string());
    }
}

#[tokio::test]
async fn agent_loop_stops_on_final_answer() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: "done".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            tool_calls: vec![],
            done: true,
            thinking: None,
            context_tokens: None,
        }])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 3,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let out = agent
        .run(
            Message {
                role: Role::User,
                content: "hello".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect("run");

    assert_eq!(out, "done");
}

#[tokio::test]
async fn agent_loop_emits_stream_and_tool_events() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "hello world".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: json!({"path":"Cargo.toml"}),
                }],
                done: false,
                thinking: Some("considering".to_string()),
                context_tokens: None,
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "final".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: true,
                thinking: None,
                context_tokens: None,
            },
        ])),
        stream_events: vec![
            ProviderStreamEvent::ThinkingDelta("thinking ".to_string()),
            ProviderStreamEvent::AssistantDelta("hello ".to_string()),
            ProviderStreamEvent::AssistantDelta("world".to_string()),
        ],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let events = RecordingEvents::default();
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: events.clone(),
    };

    let _ = agent
        .run(
            Message {
                role: Role::User,
                content: "hello".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect("run");

    let entries = events.entries();
    assert!(entries.iter().any(|e| e == "thinking:thinking "));
    assert!(entries.iter().any(|e| e == "delta:hello "));
    assert!(entries.iter().any(|e| e == "delta:world"));
    assert!(entries.iter().any(|e| e == "tool_start:read"));
    assert!(entries.iter().any(|e| e == "tool_end:read:false"));
    assert!(entries.iter().any(|e| e == "done"));
}

#[tokio::test]
async fn agent_loop_persists_thinking_before_assistant_message() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: "done".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            tool_calls: vec![],
            done: true,
            thinking: Some("plan first".to_string()),
            context_tokens: None,
        }])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let session_for_assert = session.clone();
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 3,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let _ = agent
        .run(
            Message {
                role: Role::User,
                content: "hello".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect("run");

    let events = session_for_assert.replay_events().expect("replay events");
    let mut thinking_idx = None;
    let mut assistant_idx = None;

    for (idx, event) in events.iter().enumerate() {
        match event {
            SessionEvent::Thinking { content, .. } if content == "plan first" => {
                thinking_idx = Some(idx);
            }
            SessionEvent::Message { message, .. }
                if message.role == Role::Assistant && message.content == "done" =>
            {
                assistant_idx = Some(idx);
            }
            _ => {}
        }
    }

    let thinking_idx = thinking_idx.expect("missing thinking event");
    let assistant_idx = assistant_idx.expect("missing assistant message");
    assert!(
        thinking_idx < assistant_idx,
        "thinking must be persisted before assistant output"
    );
}

#[tokio::test]
async fn agent_loop_zero_max_steps_is_unbounded() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "working".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: false,
                thinking: None,
                context_tokens: None,
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "final".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: true,
                thinking: None,
                context_tokens: None,
            },
        ])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 0,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let out = agent
        .run(
            Message {
                role: Role::User,
                content: "hello".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect("run");

    assert_eq!(out, "final");
}

#[tokio::test]
async fn agent_loop_respects_max_steps_when_set() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: "not done yet".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            tool_calls: vec![],
            done: false,
            thinking: None,
            context_tokens: None,
        }])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 1,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let err = agent
        .run(
            Message {
                role: Role::User,
                content: "hello".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect_err("should hit max steps");

    assert!(err.to_string().contains("Reached max steps"));
}

#[tokio::test]
async fn agent_loop_injects_runtime_todo_state_message() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "todo_write".to_string(),
                    arguments: json!({
                        "todos": [
                            {"content": "Ship feature", "status": "pending", "priority": "high"}
                        ]
                    }),
                }],
                done: false,
                thinking: None,
                context_tokens: None,
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "done".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: true,
                thinking: None,
                context_tokens: None,
            },
        ])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let captured_requests = provider.requests.clone();

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let out = agent
        .run(
            Message {
                role: Role::User,
                content: "plan and execute".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect("run");

    assert_eq!(out, "done");

    let requests = captured_requests.lock().expect("requests");
    assert_eq!(requests.len(), 2);
    let state_message = requests[1]
        .messages
        .iter()
        .find(|msg| msg.role == Role::System && msg.content.contains("Runtime TODO state"))
        .expect("runtime todo state message");
    assert!(state_message.content.contains("Ship feature"));
}

#[tokio::test]
async fn agent_loop_todo_read_returns_current_runtime_snapshot() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "todo_write".to_string(),
                    arguments: json!({
                        "todos": [
                            {"content": "Ship feature", "status": "pending", "priority": "high"}
                        ]
                    }),
                }],
                done: false,
                thinking: None,
                context_tokens: None,
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![ToolCall {
                    id: "call-2".to_string(),
                    name: "todo_read".to_string(),
                    arguments: json!({}),
                }],
                done: false,
                thinking: None,
                context_tokens: None,
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "done".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: true,
                thinking: None,
                context_tokens: None,
            },
        ])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let session_reader = session.clone();
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let out = agent
        .run(
            Message {
                role: Role::User,
                content: "manage todos".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
        )
        .await
        .expect("run");

    assert_eq!(out, "done");

    let events = session_reader.replay_events().expect("replay events");
    let snapshot = events
        .into_iter()
        .find_map(|event| match event {
            SessionEvent::ToolResult {
                id,
                result: Some(result),
                ..
            } if id == "call-2" => Some(result.payload),
            _ => None,
        })
        .expect("todo_read result payload");

    assert_eq!(snapshot["counts"]["total"], 1);
    assert_eq!(snapshot["todos"][0]["content"], "Ship feature");
}

#[tokio::test]
async fn agent_loop_question_tool_uses_question_handler_answers() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: String::new(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "question".to_string(),
                    arguments: json!({
                        "questions": [
                            {
                                "question": "Which strategy?",
                                "header": "Strategy",
                                "options": [
                                    {"label": "A", "description": "First"},
                                    {"label": "B", "description": "Second"}
                                ]
                            }
                        ]
                    }),
                }],
                done: false,
                thinking: None,
                context_tokens: None,
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "done".to_string(),
                    attachments: Vec::new(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: true,
                thinking: None,
                context_tokens: None,
            },
        ])),
        stream_events: vec![],
        requests: Arc::new(Mutex::new(Vec::new())),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let session_reader = session.clone();
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentLoop {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
        events: NoopEvents,
    };

    let out = agent
        .run_with_question_tool(
            Message {
                role: Role::User,
                content: "ask me".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
            },
            |_tool| Ok(true),
            |_questions| async { Ok(vec![vec!["B".to_string()]]) },
        )
        .await
        .expect("run");

    assert_eq!(out, "done");

    let events = session_reader.replay_events().expect("replay events");
    let payload = events
        .into_iter()
        .find_map(|event| match event {
            SessionEvent::ToolResult {
                id,
                result: Some(result),
                ..
            } if id == "call-1" => Some(result.payload),
            _ => None,
        })
        .expect("question result payload");

    assert_eq!(payload["answers"][0][0], "B");
}
