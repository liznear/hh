use crate::types::{ProviderRequest, ProviderResponse, ProviderStreamEvent, ToolSchema};
use async_trait::async_trait;

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

#[async_trait]
pub trait ToolRegistry: Send + Sync {
    fn schemas(&self) -> Vec<ToolSchema>;
    fn is_blocking(&self, tool_name: &str) -> bool;
}
