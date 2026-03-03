use crate::tool::{Tool, ToolResult, ToolSchema, parse_tool_args};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub struct WebFetchTool {
    client: reqwest::Client,
}

pub struct WebSearchTool {
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct WebFetchOutput {
    url: String,
    status_code: u16,
    ok: bool,
    body: String,
}

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
}

#[derive(Debug, Deserialize)]
struct WebSearchArgs {
    query: String,
}

#[derive(Debug, Serialize)]
struct McpRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: McpParams,
}

#[derive(Debug, Serialize)]
struct McpParams {
    name: String,
    arguments: McpArguments,
}

#[derive(Debug, Serialize)]
struct McpArguments {
    query: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "numResults")]
    num_results: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    livecrawl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    type_: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "contextMaxCharacters"
    )]
    context_max_characters: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct McpResponse {
    #[serde(default)]
    result: Option<McpResult>,
}

#[derive(Debug, Deserialize)]
struct McpResult {
    content: Vec<McpContent>,
}

#[derive(Debug, Deserialize)]
struct McpContent {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    content_type: String,
    text: String,
}

enum WebRequestError {
    Request(reqwest::Error),
    ReadBody(reqwest::Error),
}

async fn send_and_read_text(
    request: reqwest::RequestBuilder,
) -> Result<(StatusCode, String), WebRequestError> {
    let response = request.send().await.map_err(WebRequestError::Request)?;
    let status = response.status();
    let body = response.text().await.map_err(WebRequestError::ReadBody)?;
    Ok((status, body))
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
            capability: Some("web".to_string()),
            mutating: Some(false),
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
        let parsed: WebFetchArgs = match parse_tool_args(args, "web_fetch") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let url = parsed.url;
        let (status, body) = match send_and_read_text(self.client.get(&url)).await {
            Ok(result) => result,
            Err(WebRequestError::Request(err)) => {
                return ToolResult::err_text("request_error", err.to_string());
            }
            Err(WebRequestError::ReadBody(err)) => {
                return ToolResult::err_text("read_body_error", err.to_string());
            }
        };

        let output = WebFetchOutput {
            url,
            status_code: status.as_u16(),
            ok: status.is_success(),
            body,
        };

        if status.is_success() {
            ToolResult::ok_json_serializable("ok", &output)
        } else {
            let payload = serde_json::to_value(&output)
                .unwrap_or_else(|_| json!({"status_code": status.as_u16()}));
            ToolResult::err_json("request_failed", payload)
        }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (compatible; hh-agent/1.0)")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".to_string(),
            description: "Search the web for information. Returns search results with titles, snippets, and URLs.".to_string(),
            capability: Some("web".to_string()),
            mutating: Some(false),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let parsed: WebSearchArgs = match parse_tool_args(args, "web_search") {
            Ok(value) => value,
            Err(err) => return err,
        };
        let query = parsed.query;

        if query.is_empty() {
            return ToolResult::err_text("invalid_input", "query is required");
        }

        let api_key = std::env::var("HH_EXA_API_KEY").ok();

        let mcp_request = McpRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tools/call",
            params: McpParams {
                name: "web_search_exa".to_string(),
                arguments: McpArguments {
                    query: query.clone(),
                    num_results: Some(8),
                    livecrawl: Some("fallback".to_string()),
                    type_: Some("auto".to_string()),
                    context_max_characters: Some(10000),
                },
            },
        };

        let mut http_request = self
            .client
            .post("https://mcp.exa.ai/mcp")
            .json(&mcp_request)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        if let Some(ref key) = api_key {
            http_request = http_request.header("x-api-key", key);
        }

        let response = match http_request.send().await {
            Ok(r) => r,
            Err(err) => {
                return ToolResult::err_text(
                    "request_error",
                    format!("search request failed: {}", err),
                );
            }
        };

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return ToolResult::err_text(
                "search_failed",
                format!("search failed: status={}, body={}", status, error_body),
            );
        }

        let response_text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                return ToolResult::err_text(
                    "read_body_error",
                    format!("failed to read response: {}", err),
                );
            }
        };

        // Parse SSE response
        for line in response_text.lines() {
            if let Some(json_str) = line.strip_prefix("data: ") {
                match serde_json::from_str::<McpResponse>(json_str) {
                    Ok(mcp_response) => {
                        if let Some(result) = mcp_response.result
                            && let Some(content) = result.content.first()
                        {
                            return ToolResult::ok_text("ok", content.text.clone());
                        }
                    }
                    Err(err) => {
                        return ToolResult::err_text(
                            "parse_error",
                            format!("failed to parse MCP response: {}", err),
                        );
                    }
                }
            }
        }

        ToolResult::err_text(
            "no_results",
            "No search results found. Please try a different query.",
        )
    }
}
