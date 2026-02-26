use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use reqwest::StatusCode;
use scraper::{Html, Selector};
use serde::Serialize;
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

#[derive(Debug, Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Serialize)]
struct WebSearchOutput {
    query: String,
    count: usize,
    results: Vec<SearchResult>,
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
        let url = args.get("url").and_then(|v| v.as_str()).unwrap_or_default();
        let (status, body) = match send_and_read_text(self.client.get(url)).await {
            Ok(result) => result,
            Err(WebRequestError::Request(err)) => {
                return ToolResult::err_text("request_error", err.to_string());
            }
            Err(WebRequestError::ReadBody(err)) => {
                return ToolResult::err_text("read_body_error", err.to_string());
            }
        };

        let output = WebFetchOutput {
            url: url.to_string(),
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
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if query.is_empty() {
            return ToolResult::err_text("invalid_input", "query is required");
        }

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let (status, html) = match send_and_read_text(self.client.get(&url)).await {
            Ok(result) => result,
            Err(WebRequestError::Request(err)) => {
                return ToolResult::err_text(
                    "request_error",
                    format!("search request failed: {}", err),
                );
            }
            Err(WebRequestError::ReadBody(err)) => {
                return ToolResult::err_text(
                    "read_body_error",
                    format!("failed to read response: {}", err),
                );
            }
        };

        if !status.is_success() {
            return ToolResult::err_text(
                "search_failed",
                format!("search failed: status={status}"),
            );
        }

        let results = parse_ddg_results(&html);
        let output = WebSearchOutput {
            query: query.to_string(),
            count: results.len(),
            results,
        };
        ToolResult::ok_json_serializable("ok", &output)
    }
}

fn parse_ddg_results(html: &str) -> Vec<SearchResult> {
    let document = Html::parse_document(html);
    let result_selector = match Selector::parse(".result") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let title_selector = match Selector::parse(".result__a") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let snippet_selector = match Selector::parse(".result__snippet") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();

    for result in document.select(&result_selector) {
        let title_el = result.select(&title_selector).next();
        let title = title_el
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default()
            .trim()
            .to_string();

        let url = title_el
            .and_then(|el| el.value().attr("href"))
            .and_then(extract_ddg_url)
            .unwrap_or_default();

        let snippet = result
            .select(&snippet_selector)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default()
            .trim()
            .to_string();

        if !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }

        if results.len() >= 5 {
            break;
        }
    }

    results
}

fn extract_ddg_url(redirect_url: &str) -> Option<String> {
    // DuckDuckGo redirect URLs are like: /l/?uddg=URL&rut=...
    let prefix = "/l/?uddg=";
    if let Some(start) = redirect_url.find(prefix) {
        let encoded = &redirect_url[start + prefix.len()..];
        let encoded = if let Some(end) = encoded.find('&') {
            &encoded[..end]
        } else {
            encoded
        };
        // URL decode the result
        return Some(urlencoding_decode(encoded));
    }
    None
}

fn urlencoding_decode(s: &str) -> String {
    // Simple URL decoding - replace + with space and decode %xx sequences
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '+' {
            result.push(' ');
        } else if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}
