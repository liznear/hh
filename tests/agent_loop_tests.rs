use async_trait::async_trait;
use hh::agent::AgentLoop;
use hh::config::settings::Settings;
use hh::permission::PermissionMatcher;
use hh::provider::{Message, Provider, ProviderRequest, ProviderResponse, Role};
use hh::session::SessionStore;
use hh::tool::registry::ToolRegistry;

#[derive(Clone)]
struct MockProvider {
    responses: std::sync::Arc<std::sync::Mutex<Vec<ProviderResponse>>>,
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
}

#[tokio::test]
async fn agent_loop_stops_on_final_answer() {
    let provider = MockProvider {
        responses: std::sync::Arc::new(std::sync::Mutex::new(vec![ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: "done".to_string(),
                tool_call_id: None,
            },
            tool_calls: vec![],
            done: true,
        }])),
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let cwd = temp.path().join("ws");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    let settings = Settings::default();
    let session = SessionStore::for_workspace(temp.path(), &cwd).expect("session");

    let agent = AgentLoop {
        provider,
        tool_registry: ToolRegistry::new(&settings, &cwd),
        permissions: PermissionMatcher::new(settings.clone()),
        max_steps: 3,
        model: settings.provider.model,
        session,
    };

    let out = agent
        .run("hello".to_string(), |_tool| Ok(true))
        .await
        .expect("run");

    assert_eq!(out, "done");
}
