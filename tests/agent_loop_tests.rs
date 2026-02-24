use async_trait::async_trait;
use hh::config::settings::Settings;
use hh::core::agent::{AgentEvents, AgentLoop, NoopEvents};
use hh::permission::PermissionMatcher;
use hh::provider::{
    Message, Provider, ProviderRequest, ProviderResponse, ProviderStreamEvent, Role, ToolCall,
};
use hh::session::SessionStore;
use hh::tool::registry::ToolRegistry;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct MockProvider {
    responses: Arc<Mutex<Vec<ProviderResponse>>>,
    stream_events: Vec<ProviderStreamEvent>,
}

#[async_trait]
impl Provider for MockProvider {
    async fn complete(&self, _req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
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

    fn on_tool_end(&self, name: &str, is_error: bool, _output_preview: &str) {
        self.log
            .lock()
            .expect("log")
            .push(format!("tool_end:{name}:{is_error}"));
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
                tool_call_id: None,
            },
            tool_calls: vec![],
            done: true,
            thinking: None,
        }])),
        stream_events: vec![],
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");

    let agent = AgentLoop {
        provider,
        tool_registry: ToolRegistry::new(&settings, &cwd),
        permissions: PermissionMatcher::new(settings.clone()),
        max_steps: 3,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.provider.model,
        session,
        events: NoopEvents,
    };

    let out = agent
        .run("hello".to_string(), |_tool| Ok(true))
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
                    tool_call_id: None,
                },
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: json!({"path":"Cargo.toml"}),
                }],
                done: false,
                thinking: Some("considering".to_string()),
            },
            ProviderResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: "final".to_string(),
                    tool_call_id: None,
                },
                tool_calls: vec![],
                done: true,
                thinking: None,
            },
        ])),
        stream_events: vec![
            ProviderStreamEvent::ThinkingDelta("thinking ".to_string()),
            ProviderStreamEvent::AssistantDelta("hello ".to_string()),
            ProviderStreamEvent::AssistantDelta("world".to_string()),
        ],
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::new(temp.path(), &cwd, None, None).expect("session");
    let events = RecordingEvents::default();

    let agent = AgentLoop {
        provider,
        tool_registry: ToolRegistry::new(&settings, &cwd),
        permissions: PermissionMatcher::new(settings.clone()),
        max_steps: 4,
        system_prompt: settings.agent.resolved_system_prompt(),
        model: settings.provider.model,
        session,
        events: events.clone(),
    };

    let _ = agent
        .run("hello".to_string(), |_tool| Ok(true))
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
