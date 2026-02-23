use crate::core::types::{ProviderRequest, ProviderResponse, ProviderStreamEvent};
use async_trait::async_trait;
use serde_json::Value;

pub trait AgentEvents: Send + Sync {
    fn on_thinking(&self, _text: &str) {}
    fn on_tool_start(&self, _name: &str, _args: &Value) {}
    fn on_tool_end(&self, _name: &str, _is_error: bool, _output_preview: &str) {}
    fn on_assistant_delta(&self, _delta: &str) {}
    fn on_assistant_done(&self) {}
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopEvents;

impl AgentEvents for NoopEvents {}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse>;

    async fn complete_stream<F>(
        &self,
        req: ProviderRequest,
        mut on_event: F,
    ) -> anyhow::Result<ProviderResponse>
    where
        F: FnMut(ProviderStreamEvent) + Send,
    {
        let response = self.complete(req).await?;
        if let Some(thinking) = &response.thinking {
            on_event(ProviderStreamEvent::ThinkingDelta(thinking.clone()));
        }
        if !response.assistant_message.content.is_empty() {
            on_event(ProviderStreamEvent::AssistantDelta(
                response.assistant_message.content.clone(),
            ));
        }
        Ok(response)
    }
}
