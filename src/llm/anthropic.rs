//! Anthropic Claude adapter implementing [`LlmAdapter`].
//!
//! Converts between the framework's internal [`ContentBlock`] types and the
//! Anthropic SDK's wire format.

use crate::llm::adapter::LlmAdapter;
use crate::types::*;
use async_trait::async_trait;

/// LLM adapter backed by the Anthropic Claude API.
pub struct AnthropicAdapter {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicAdapter {
    pub fn new(api_key: Option<&str>, base_url: Option<&str>) -> Self {
        let key = api_key
            .map(String::from)
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .unwrap_or_default();

        Self {
            api_key: key,
            base_url: base_url
                .unwrap_or("https://api.anthropic.com")
                .to_string(),
            client: reqwest::Client::new(),
        }
    }

    fn to_anthropic_content_block(block: &ContentBlock) -> serde_json::Value {
        match block {
            ContentBlock::Text(t) => serde_json::json!({
                "type": "text",
                "text": t.text,
            }),
            ContentBlock::ToolUse(tu) => serde_json::json!({
                "type": "tool_use",
                "id": tu.id,
                "name": tu.name,
                "input": tu.input,
            }),
            ContentBlock::ToolResult(tr) => serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tr.tool_use_id,
                "content": tr.content,
                "is_error": tr.is_error,
            }),
            ContentBlock::Image(img) => serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": img.source.media_type,
                    "data": img.source.data,
                }
            }),
        }
    }

    fn to_anthropic_messages(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|msg| {
                let role = match msg.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                serde_json::json!({
                    "role": role,
                    "content": msg.content.iter().map(Self::to_anthropic_content_block).collect::<Vec<_>>(),
                })
            })
            .collect()
    }

    fn to_anthropic_tools(tools: &[LlmToolDef]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                let mut schema = t.input_schema.clone();
                if let Some(obj) = schema.as_object_mut() {
                    obj.entry("type").or_insert(serde_json::json!("object"));
                }
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": schema,
                })
            })
            .collect()
    }

    fn from_anthropic_content_block(block: &serde_json::Value) -> ContentBlock {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => ContentBlock::Text(TextBlock {
                text: block.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(),
            }),
            Some("tool_use") => ContentBlock::ToolUse(ToolUseBlock {
                id: block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                name: block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                input: block.get("input").cloned().unwrap_or(serde_json::Value::Object(Default::default())),
            }),
            _ => ContentBlock::Text(TextBlock {
                text: format!("[unsupported block type: {}]", block.get("type").and_then(|t| t.as_str()).unwrap_or("unknown")),
            }),
        }
    }
}

#[async_trait]
impl LlmAdapter for AnthropicAdapter {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn chat(
        &self,
        messages: &[LlmMessage],
        options: &LlmChatOptions,
    ) -> Result<LlmResponse, String> {
        let anthropic_messages = Self::to_anthropic_messages(messages);

        let mut body = serde_json::json!({
            "model": options.model,
            "max_tokens": options.max_tokens.unwrap_or(4096),
            "messages": anthropic_messages,
        });

        if let Some(ref prompt) = options.system_prompt {
            body["system"] = serde_json::Value::String(prompt.clone());
        }

        if let Some(ref tools) = options.tools {
            if !tools.is_empty() {
                body["tools"] = serde_json::Value::Array(Self::to_anthropic_tools(tools));
            }
        }

        if let Some(temp) = options.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Anthropic API request failed: {}", e))?;

        let status = response.status();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Anthropic response: {}", e))?;

        if !status.is_success() {
            let error_msg = response_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(format!("Anthropic API error ({}): {}", status, error_msg));
        }

        let content = response_body
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| blocks.iter().map(Self::from_anthropic_content_block).collect())
            .unwrap_or_default();

        let usage = response_body.get("usage").unwrap_or(&serde_json::Value::Null);

        Ok(LlmResponse {
            id: response_body.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            content,
            model: response_body.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            stop_reason: response_body.get("stop_reason").and_then(|v| v.as_str()).unwrap_or("end_turn").to_string(),
            usage: TokenUsage {
                input_tokens: usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                output_tokens: usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            },
        })
    }
}
