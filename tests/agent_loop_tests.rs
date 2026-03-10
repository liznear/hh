use async_trait::async_trait;
use hh_cli::config::settings::Settings;
use hh_cli::core::RunnerOutputObserver;
use hh_cli::core::agent::AgentCore;
use hh_cli::core::{ApprovalChoice, Message, Role};
use hh_cli::permission::PermissionMatcher;
use hh_cli::provider::{
    Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent, ToolCall,
};
use hh_cli::session::{SessionEvent, SessionStore};
use hh_cli::tool::registry::ToolRegistry;
use serde_json::json;
use std::path::{Path, PathBuf};
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

impl RunnerOutputObserver for RecordingEvents {
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

    fn on_tool_end(&self, name: &str, result: &hh_cli::tool::ToolResult) {
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

struct TestContext {
    _temp_dir: tempfile::TempDir,
    workspace_dir: PathBuf,
    settings: Settings,
    session: SessionStore,
}

impl TestContext {
    fn new() -> Self {
        Self::with_settings(Settings::default())
    }

    fn with_settings(settings: Settings) -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workspace_dir = temp_dir.path().join("ws");
        std::fs::create_dir_all(&workspace_dir).expect("mkdir");
        let session =
            SessionStore::new(temp_dir.path(), &workspace_dir, None, None).expect("session");
        Self {
            _temp_dir: temp_dir,
            workspace_dir,
            settings,
            session,
        }
    }

    fn seed_workspace_file(&self, relative_path: &str, content: &str) {
        std::fs::write(self.workspace_dir.join(relative_path), content)
            .expect("seed workspace file");
    }

    fn make_agent(
        &self,
        provider: MockProvider,
        max_steps: usize,
    ) -> AgentCore<MockProvider, ToolRegistry, PermissionMatcher, SessionStore> {
        let tools = ToolRegistry::new(&self.settings, &self.workspace_dir);
        let schemas = tools.schemas();
        AgentCore {
            provider,
            tools,
            approvals: PermissionMatcher::new(self.settings.clone(), &schemas, &self.workspace_dir),
            max_steps,
            system_prompt: self.settings.agent.resolved_system_prompt(),
            model: self.settings.selected_model_ref().to_string(),
            session: self.session.clone(),
        }
    }
}

async fn run_agent_with_custom_approve<F, Fut>(
    agent: &hh_cli::core::agent::AgentCore<
        MockProvider,
        hh_cli::tool::registry::ToolRegistry,
        hh_cli::permission::PermissionMatcher,
        hh_cli::session::SessionStore,
    >,
    messages: Vec<hh_cli::core::Message>,
    approve_handler: F,
) -> anyhow::Result<String>
where
    F: FnMut(hh_cli::core::ApprovalRequest) -> Fut + Send + 'static,
    Fut:
        std::future::Future<Output = anyhow::Result<hh_cli::core::ApprovalChoice>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    for msg in messages {
        tx.send(hh_cli::core::agent::RunnerInput::Message(msg))
            .await
            .unwrap();
    }

    let tx_clone = tx.clone();
    let session_for_emit = agent.session.clone();
    let mut last_emitted_todo_items = Vec::new();

    let handler = std::sync::Arc::new(tokio::sync::Mutex::new(approve_handler));

    let mut emit_output = move |output: hh_cli::core::agent::RunnerOutput| {
        if let hh_cli::core::agent::RunnerOutput::ApprovalRequired { call_id, request } = &output {
            let tx = tx_clone.clone();
            let call_id = call_id.clone();
            let handler = std::sync::Arc::clone(&handler);
            let req = request.clone();
            tokio::spawn(async move {
                let mut h = handler.lock().await;
                let choice_res = h(req).await;
                if let Ok(choice) = choice_res {
                    let _ = tx
                        .send(hh_cli::core::agent::RunnerInput::ApprovalDecision {
                            call_id,
                            choice,
                        })
                        .await;
                }
            });
        }
        hh_cli::core::agent::apply_runner_output_to_observer(
            &(),
            &session_for_emit,
            output,
            &mut last_emitted_todo_items,
        )
    };

    agent
        .run_with_runner_output_sink_cancellable(rx, &mut emit_output, &mut || Vec::new())
        .await
}

struct TestData;

impl TestData {
    fn user_message(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    fn assistant_message(content: &str, tool_calls: Vec<ToolCall>) -> Message {
        Message {
            role: Role::Assistant,
            content: content.to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls,
        }
    }

    fn provider_response(
        content: &str,
        tool_calls: Vec<ToolCall>,
        done: bool,
        thinking: Option<&str>,
    ) -> ProviderResponse {
        ProviderResponse {
            assistant_message: Self::assistant_message(content, tool_calls.clone()),
            tool_calls,
            done,
            thinking: thinking.map(str::to_string),
            context_tokens: None,
        }
    }
}

fn mock_provider_with_responses(responses: Vec<ProviderResponse>) -> MockProvider {
    MockProvider {
        responses: Arc::new(Mutex::new(responses)),
        stream_events: Vec::new(),
        requests: Arc::new(Mutex::new(Vec::new())),
    }
}

fn mock_provider_with_responses_and_stream(
    responses: Vec<ProviderResponse>,
    stream_events: Vec<ProviderStreamEvent>,
) -> MockProvider {
    MockProvider {
        responses: Arc::new(Mutex::new(responses)),
        stream_events,
        requests: Arc::new(Mutex::new(Vec::new())),
    }
}

fn test_assert_file_content(path: &Path, expected: &str) {
    assert_eq!(std::fs::read_to_string(path).expect("read file"), expected);
}

#[tokio::test]
async fn agent_loop_stops_on_final_answer() {
    let provider = mock_provider_with_responses(vec![TestData::provider_response(
        "done",
        vec![],
        true,
        None,
    )]);
    let context = TestContext::new();
    context.seed_workspace_file("Cargo.toml", "[package]\nname='ws'\n");
    let agent = context.make_agent(provider, 3);

    let out = run_agent_with_custom_approve(
        &agent,
        vec![TestData::user_message("hello")],
        |_request| async { Ok::<ApprovalChoice, anyhow::Error>(ApprovalChoice::AllowSession) },
    )
    .await
    .expect("run");

    assert_eq!(out, "done");
}

#[tokio::test]
async fn agent_loop_emits_stream_and_tool_events() {
    let provider = mock_provider_with_responses_and_stream(
        vec![
            TestData::provider_response(
                "hello world",
                vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: json!({"path":"Cargo.toml"}),
                }],
                false,
                Some("considering"),
            ),
            TestData::provider_response("final", vec![], true, None),
        ],
        vec![
            ProviderStreamEvent::ThinkingDelta("thinking ".to_string()),
            ProviderStreamEvent::AssistantDelta("hello ".to_string()),
            ProviderStreamEvent::AssistantDelta("world".to_string()),
        ],
    );

    let context = TestContext::new();
    let events = RecordingEvents::default();
    let agent = context.make_agent(provider, 4);

    let mut last_emitted_todo_items = Vec::new();
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tx.send(hh_cli::core::agent::RunnerInput::Message(
        TestData::user_message("hello"),
    ))
    .await
    .unwrap();
    let tx_clone = tx.clone();
    let _ = agent
        .run_with_runner_output_sink_cancellable(
            rx,
            &mut |output| {
                if let hh_cli::core::agent::RunnerOutput::ApprovalRequired { call_id, .. } = &output
                {
                    let tx = tx_clone.clone();
                    let call_id = call_id.clone();
                    tokio::spawn(async move {
                        let _ = tx
                            .send(hh_cli::core::agent::RunnerInput::ApprovalDecision {
                                call_id,
                                choice: ApprovalChoice::AllowSession,
                            })
                            .await;
                    });
                }
                hh_cli::core::agent::apply_runner_output_to_observer(
                    &events,
                    &agent.session,
                    output,
                    &mut last_emitted_todo_items,
                )
            },
            &mut Vec::new,
        )
        .await
        .expect("run");

    let entries = events.entries();
    assert!(entries.iter().any(|e| e == "thinking:thinking "));
    assert!(entries.iter().any(|e| e == "delta:hello "));
    assert!(entries.iter().any(|e| e == "delta:world"));
    assert!(entries.iter().any(|e| e == "tool_start:read"));
    assert!(
        entries
            .iter()
            .any(|e| e == "tool_end:read:false" || e == "tool_end:read:true")
    );
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
                tool_calls: Vec::new(),
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

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 3,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let _ = run_agent_with_custom_approve(
        &agent,
        vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        |_request| async {
            Ok::<hh_cli::core::ApprovalChoice, anyhow::Error>(
                hh_cli::core::ApprovalChoice::AllowSession,
            )
        },
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
                    tool_calls: Vec::new(),
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
                    tool_calls: Vec::new(),
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

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 0,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let out = run_agent_with_custom_approve(
        &agent,
        vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        |_request| async {
            Ok::<hh_cli::core::ApprovalChoice, anyhow::Error>(
                hh_cli::core::ApprovalChoice::AllowSession,
            )
        },
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
                tool_calls: Vec::new(),
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

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 1,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let err = run_agent_with_custom_approve(
        &agent,
        vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        |_request| async {
            Ok::<hh_cli::core::ApprovalChoice, anyhow::Error>(
                hh_cli::core::ApprovalChoice::AllowSession,
            )
        },
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
                    tool_calls: Vec::new(),
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
                    tool_calls: Vec::new(),
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

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let out = run_agent_with_custom_approve(
        &agent,
        vec![Message {
            role: Role::User,
            content: "plan and execute".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        |_request| async {
            Ok::<hh_cli::core::ApprovalChoice, anyhow::Error>(
                hh_cli::core::ApprovalChoice::AllowSession,
            )
        },
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
                    tool_calls: Vec::new(),
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
                    tool_calls: Vec::new(),
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
                    tool_calls: Vec::new(),
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

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let out = run_agent_with_custom_approve(
        &agent,
        vec![Message {
            role: Role::User,
            content: "manage todos".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        |_request| async {
            Ok::<hh_cli::core::ApprovalChoice, anyhow::Error>(
                hh_cli::core::ApprovalChoice::AllowSession,
            )
        },
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
                    tool_calls: Vec::new(),
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
                    tool_calls: Vec::new(),
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

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let mut last_emitted_todo_items = Vec::new();
    let (tx, rx) = tokio::sync::mpsc::channel(100);
    tx.send(hh_cli::core::agent::RunnerInput::Message(Message {
        role: Role::User,
        content: "ask me".to_string(),
        attachments: Vec::new(),
        tool_call_id: None,
        tool_calls: Vec::new(),
    }))
    .await
    .unwrap();
    let tx_clone = tx.clone();
    let out = agent
        .run_with_runner_output_sink_cancellable(
            rx,
            &mut |output| {
                if let hh_cli::core::agent::RunnerOutput::ApprovalRequired { call_id, .. } = &output
                {
                    let tx = tx_clone.clone();
                    let call_id = call_id.clone();
                    tokio::spawn(async move {
                        let _ = tx
                            .send(hh_cli::core::agent::RunnerInput::ApprovalDecision {
                                call_id,
                                choice: hh_cli::core::ApprovalChoice::AllowSession,
                            })
                            .await;
                    });
                }
                if let hh_cli::core::agent::RunnerOutput::QuestionRequired { call_id, .. } = &output
                {
                    let tx = tx_clone.clone();
                    let call_id = call_id.clone();
                    tokio::spawn(async move {
                        let _ = tx
                            .send(hh_cli::core::agent::RunnerInput::QuestionAnswered {
                                call_id,
                                answers: vec![vec!["B".to_string()]],
                            })
                            .await;
                    });
                }
                hh_cli::core::agent::apply_runner_output_to_observer(
                    &(),
                    &agent.session,
                    output,
                    &mut last_emitted_todo_items,
                )
            },
            &mut || Vec::new(),
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

#[tokio::test]
async fn approval_choice_is_remembered_for_repeated_ask_policy_tool_calls() {
    let cases = [
        ("allow-session", ApprovalChoice::AllowSession),
        ("allow-always", ApprovalChoice::AllowAlways),
    ];

    for (case_name, approval_choice) in cases {
        let provider = mock_provider_with_responses(vec![
            TestData::provider_response(
                "writing files",
                vec![
                    ToolCall {
                        id: "call-1".to_string(),
                        name: "write".to_string(),
                        arguments: json!({"path":"a.txt","content":"one"}),
                    },
                    ToolCall {
                        id: "call-2".to_string(),
                        name: "write".to_string(),
                        arguments: json!({"path":"b.txt","content":"two"}),
                    },
                ],
                false,
                None,
            ),
            TestData::provider_response("done", vec![], true, None),
        ]);
        let context = TestContext::new();
        let agent = context.make_agent(provider, 4);

        let approval_count = Arc::new(Mutex::new(0usize));
        let approval_count_for_handler = Arc::clone(&approval_count);

        let out = run_agent_with_custom_approve(
            &agent,
            vec![TestData::user_message("write two files")],
            move |_request| {
                let approval_count = Arc::clone(&approval_count_for_handler);
                async move {
                    *approval_count.lock().expect("approval count") += 1;
                    Ok::<ApprovalChoice, anyhow::Error>(approval_choice)
                }
            },
        )
        .await
        .expect("run");

        assert_eq!(out, "done", "{case_name}");
        assert_eq!(
            *approval_count.lock().expect("approval count"),
            1,
            "{case_name}"
        );
        test_assert_file_content(&context.workspace_dir.join("a.txt"), "one");
        test_assert_file_content(&context.workspace_dir.join("b.txt"), "two");
    }
}

#[tokio::test]
async fn bash_approval_request_includes_llm_stated_purpose() {
    let provider = MockProvider {
        responses: Arc::new(Mutex::new(vec![ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: "checking changes".to_string(),
                attachments: Vec::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "bash".to_string(),
                arguments: json!({
                    "command": "git diff --name-only",
                    "description": "inspect modified files"
                }),
            }],
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

    let mut settings = Settings::default();
    settings.permissions.bash = "ask".to_string();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let tools = ToolRegistry::new(&settings, &cwd);
    let schemas = tools.schemas();

    let agent = AgentCore {
        provider,
        tools,
        approvals: PermissionMatcher::new(settings.clone(), &schemas, &cwd),
        max_steps: 1,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.selected_model_ref().to_string(),
        session,
    };

    let captured = Arc::new(Mutex::new(String::new()));
    let captured_for_handler = Arc::clone(&captured);
    let _ = run_agent_with_custom_approve(
        &agent,
        vec![Message {
            role: Role::User,
            content: "show changed files".to_string(),
            attachments: Vec::new(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }],
        move |request| {
            let captured_for_handler = Arc::clone(&captured_for_handler);
            async move {
                *captured_for_handler.lock().expect("capture") = request.body;
                Ok::<hh_cli::core::ApprovalChoice, anyhow::Error>(
                    hh_cli::core::ApprovalChoice::Deny,
                )
            }
        },
    )
    .await;

    let body = captured.lock().expect("captured body").clone();
    assert!(body.contains("Allow `git diff --name-only` to inspect modified files"));
}
