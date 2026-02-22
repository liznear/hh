use crate::provider::{Message, Provider, ProviderRequest, ProviderResponse, Role, ToolCall};
use anyhow::{Context, bail};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use std::env;

pub struct OpenAiCompatibleProvider {
    base_url: String,
    model: String,
    api_key_env: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, model: String, api_key_env: String) -> Self {
        Self {
            base_url,
            model,
            api_key_env,
            client: reqwest::Client::new(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    fn auth_headers(&self) -> anyhow::Result<HeaderMap> {
        let api_key = env::var(&self.api_key_env)
            .with_context(|| format!("missing API key env var {}", self.api_key_env))?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", api_key))?,
        );
        Ok(headers)
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn complete(&self, req: ProviderRequest) -> anyhow::Result<ProviderResponse> {
        let tools = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect::<Vec<_>>();

        let messages = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                let mut obj = json!({
                    "role": role,
                    "content": m.content,
                });
                if let Some(id) = &m.tool_call_id {
                    obj["tool_call_id"] = json!(id);
                }
                obj
            })
            .collect::<Vec<_>>();

        let body = json!({
            "model": if req.model.is_empty() { &self.model } else { &req.model },
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
        });

        let response = self
            .client
            .post(self.endpoint())
            .headers(self.auth_headers()?)
            .json(&body)
            .send()
            .await
            .context("provider request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("provider error {}: {}", status, body);
        }

        let value: Value = response.json().await.context("invalid provider JSON")?;
        let choice = value
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .context("provider response missing choices[0]")?;

        let message = choice
            .get("message")
            .and_then(|m| m.as_object())
            .context("provider response missing message")?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();

        let mut tool_calls = Vec::new();
        if let Some(calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
            for call in calls {
                let id = call
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let function = call
                    .get("function")
                    .and_then(|v| v.as_object())
                    .context("tool call missing function")?;
                let name = function
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let args_raw = function
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let arguments: Value = serde_json::from_str(args_raw).unwrap_or_else(|_| json!({}));
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
        }

        Ok(ProviderResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content,
                tool_call_id: None,
            },
            done: tool_calls.is_empty(),
            tool_calls,
        })
    }
}
