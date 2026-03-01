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
    fn serialization_error(err: serde_json::Error) -> Self {
        Self::err_text(
            "serialization_error",
            format!("failed to serialize output: {err}"),
        )
    }

    fn text_result(is_error: bool, summary: impl Into<String>, text: impl Into<String>) -> Self {
        let output = text.into();
        Self {
            is_error,
            summary: summary.into(),
            content_type: "text/plain".to_string(),
            payload: Value::String(output.clone()),
            output,
        }
    }

    fn json_result(
        is_error: bool,
        summary: impl Into<String>,
        content_type: impl Into<String>,
        payload: Value,
        fallback: impl Into<String>,
    ) -> Self {
        let fallback = fallback.into();
        let output = serde_json::to_string(&payload).unwrap_or_else(|_| fallback.to_string());
        Self {
            is_error,
            summary: summary.into(),
            content_type: content_type.into(),
            payload,
            output,
        }
    }

    pub fn ok_text(summary: impl Into<String>, text: impl Into<String>) -> Self {
        Self::text_result(false, summary, text)
    }

    pub fn err_text(summary: impl Into<String>, text: impl Into<String>) -> Self {
        Self::text_result(true, summary, text)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::err_text("error", text)
    }

    pub fn ok_json(summary: impl Into<String>, payload: Value) -> Self {
        Self::json_result(false, summary, "application/json", payload, "{}")
    }

    pub fn ok_json_typed(
        summary: impl Into<String>,
        content_type: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self::json_result(false, summary, content_type, payload, "{}")
    }

    pub fn err_json(summary: impl Into<String>, payload: Value) -> Self {
        Self::json_result(
            true,
            summary,
            "application/json",
            payload,
            json!({}).to_string(),
        )
    }

    pub fn ok_json_serializable(summary: impl Into<String>, output: &impl Serialize) -> Self {
        match serde_json::to_value(output) {
            Ok(value) => Self::ok_json(summary, value),
            Err(err) => Self::serialization_error(err),
        }
    }

    pub fn ok_json_typed_serializable(
        summary: impl Into<String>,
        content_type: impl Into<String>,
        output: &impl Serialize,
    ) -> Self {
        match serde_json::to_value(output) {
            Ok(value) => Self::ok_json_typed(summary, content_type, value),
            Err(err) => Self::serialization_error(err),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: Value) -> ToolResult;
}
