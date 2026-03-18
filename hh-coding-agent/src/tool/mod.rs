pub mod bash;
pub mod diff;
pub mod edit;
pub mod fs;
pub mod question;
pub mod registry;
pub mod schema;
pub mod skill;
pub mod task;
pub mod todo;
pub mod web;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use hh_agent::ToolResult;
pub use registry::ToolRegistry;
pub use schema::ToolSchema;

use crate::core::agent::types::StatePatch;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub result: ToolResult,
    pub patch: StatePatch,
}

impl ToolExecution {
    pub fn from_result(result: ToolResult) -> Self {
        Self {
            result,
            patch: StatePatch::none(),
        }
    }

    pub fn new(result: ToolResult, patch: StatePatch) -> Self {
        Self { result, patch }
    }
}

pub fn parse_tool_args<T: DeserializeOwned>(args: Value, tool_name: &str) -> Result<T, ToolResult> {
    serde_json::from_value(args)
        .map_err(|err| ToolResult::error(format!("invalid {tool_name} args: {err}")))
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: Value) -> ToolResult;
    fn state_patch(&self, _args: &Value, _result: &ToolResult) -> StatePatch {
        StatePatch::none()
    }
}
