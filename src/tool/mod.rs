pub mod bash;
pub mod fs;
pub mod registry;
pub mod schema;
pub mod web;

use async_trait::async_trait;
use serde_json::Value;

pub use schema::ToolSchema;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub is_error: bool,
    pub output: String,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: Value) -> ToolResult;
}
