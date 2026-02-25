pub mod bash;
pub mod edit;
pub mod fs;
pub mod registry;
pub mod schema;
pub mod todo;
pub mod web;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use schema::ToolSchema;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub is_error: bool,
    pub summary: String,
    pub content_type: String,
    pub payload: Value,
    pub output: String,
}

impl ToolResult {
    pub fn ok_text(summary: impl Into<String>, text: impl Into<String>) -> Self {
        let output = text.into();
        Self {
            is_error: false,
            summary: summary.into(),
            content_type: "text/plain".to_string(),
            payload: Value::String(output.clone()),
            output,
        }
    }

    pub fn err_text(summary: impl Into<String>, text: impl Into<String>) -> Self {
        let output = text.into();
        Self {
            is_error: true,
            summary: summary.into(),
            content_type: "text/plain".to_string(),
            payload: Value::String(output.clone()),
            output,
        }
    }

    pub fn ok_json(summary: impl Into<String>, payload: Value) -> Self {
        let output = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        Self {
            is_error: false,
            summary: summary.into(),
            content_type: "application/json".to_string(),
            payload,
            output,
        }
    }

    pub fn ok_json_typed(
        summary: impl Into<String>,
        content_type: impl Into<String>,
        payload: Value,
    ) -> Self {
        let output = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        Self {
            is_error: false,
            summary: summary.into(),
            content_type: content_type.into(),
            payload,
            output,
        }
    }

    pub fn err_json(summary: impl Into<String>, payload: Value) -> Self {
        let output = serde_json::to_string(&payload).unwrap_or_else(|_| json!({}).to_string());
        Self {
            is_error: true,
            summary: summary.into(),
            content_type: "application/json".to_string(),
            payload,
            output,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: Value) -> ToolResult;
}
