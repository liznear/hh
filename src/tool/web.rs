use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct WebFetchTool {
    client: reqwest::Client,
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_fetch".to_string(),
            description: "Fetch content from a URL".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"}
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let url = args.get("url").and_then(|v| v.as_str()).unwrap_or_default();
        let response = self.client.get(url).send().await;
        match response {
            Ok(resp) => {
                let status = resp.status();
                match resp.text().await {
                    Ok(body) => ToolResult {
                        is_error: !status.is_success(),
                        output: format!("status={}\n{}", status.as_u16(), body),
                    },
                    Err(err) => ToolResult {
                        is_error: true,
                        output: err.to_string(),
                    },
                }
            }
            Err(err) => ToolResult {
                is_error: true,
                output: err.to_string(),
            },
        }
    }
}
