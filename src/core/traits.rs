use crate::core::types::{Message, ProviderRequest, ProviderResponse, ProviderStreamEvent};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow,
    Ask,
    Deny,
}

pub trait AgentEvents: Send + Sync {
    fn on_thinking(&self, _text: &str) {}
    fn on_tool_start(&self, _name: &str, _args: &Value) {}
    fn on_tool_end(&self, _name: &str, _result: &crate::tool::ToolResult) {}
    fn on_assistant_delta(&self, _delta: &str) {}
    fn on_context_usage(&self, _tokens: usize) {}
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

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn schemas(&self) -> Vec<crate::tool::schema::ToolSchema>;
    async fn execute(&self, name: &str, args: Value) -> crate::tool::ToolResult;
}

pub trait ApprovalPolicy: Send + Sync {
    fn decision_for_tool(&self, tool_name: &str) -> ApprovalDecision;
}

pub trait SessionSink: Send + Sync {
    fn append(&self, event: &crate::session::SessionEvent) -> anyhow::Result<()>;
}

pub trait SessionReader: Send + Sync {
    fn replay_messages(&self) -> anyhow::Result<Vec<Message>>;
}
