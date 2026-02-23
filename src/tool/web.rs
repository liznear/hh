use crate::tool::{Tool, ToolResult, ToolSchema};
use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::{Value, json};

pub struct WebFetchTool {
    client: reqwest::Client,
}

pub struct WebSearchTool {
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
            return ToolResult {
                is_error: true,
                output: "query is required".to_string(),
            };
        }

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = self.client.get(&url).send().await;

        match response {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return ToolResult {
                        is_error: true,
                        output: format!("search failed: status={}", resp.status()),
                    };
                }

                match resp.text().await {
                    Ok(html) => {
                        let results = parse_ddg_results(&html);
                        if results.is_empty() {
                            ToolResult {
                                is_error: false,
                                output: "No results found".to_string(),
                            }
                        } else {
                            ToolResult {
                                is_error: false,
                                output: results.join("\n\n"),
                            }
                        }
                    }
                    Err(err) => ToolResult {
                        is_error: true,
                        output: format!("failed to read response: {}", err),
                    },
                }
            }
            Err(err) => ToolResult {
                is_error: true,
                output: format!("search request failed: {}", err),
            },
        }
    }
}

fn parse_ddg_results(html: &str) -> Vec<String> {
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
            .and_then(|h| extract_ddg_url(h))
            .unwrap_or_default();

        let snippet = result
            .select(&snippet_selector)
            .next()
            .map(|el| el.text().collect::<String>())
            .unwrap_or_default()
            .trim()
            .to_string();

        if !title.is_empty() {
            let mut entry = format!("Title: {}", title);
            if !url.is_empty() {
                entry.push_str(&format!("\nURL: {}", url));
            }
            if !snippet.is_empty() {
                entry.push_str(&format!("\nSnippet: {}", snippet));
            }
            results.push(entry);
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
