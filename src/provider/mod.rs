pub mod openai_compatible;
pub mod types;

use async_trait::async_trait;

pub use types::{Message, ProviderRequest, ProviderResponse, Role, ToolCall};

#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse>;
}
