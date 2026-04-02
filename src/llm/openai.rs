//! OpenAI adapter implementing [`LlmAdapter`].

use crate::llm::adapter::LlmAdapter;
use crate::llm::openai_common::{build_openai_message_list, from_openai_completion, to_openai_tool};
use crate::types::*;
use async_trait::async_trait;

/// LLM adapter backed by the OpenAI Chat Completions API.
pub struct OpenAIAdapter {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIAdapter {
    pub fn new(api_key: Option<&str>, base_url: Option<&str>) -> Self {
        let key = api_key
            .map(String::from)
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_default();

        Self {
            api_key: key,
            base_url: base_url
                .unwrap_or("https://api.openai.com/v1")
                .to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmAdapter for OpenAIAdapter {
    fn name(&self) -> &str {
        "openai"
    }

    async fn chat(
        &self,
        messages: &[LlmMessage],
        options: &LlmChatOptions,
    ) -> Result<LlmResponse, String> {
        let openai_messages = build_openai_message_list(messages, options.system_prompt.as_deref());

        let mut body = serde_json::json!({
            "model": options.model,
            "messages": openai_messages,
            "stream": false,
        });

        if let Some(max_tokens) = options.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        if let Some(temp) = options.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(ref tools) = options.tools {
            if !tools.is_empty() {
                let openai_tools: Vec<serde_json::Value> =
                    tools.iter().map(|t| to_openai_tool(t)).collect();
                body["tools"] = serde_json::Value::Array(openai_tools);
            }
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenAI API request failed: {}", e))?;

        let status = response.status();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse OpenAI response: {}", e))?;

        if !status.is_success() {
            let error_msg = response_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(format!("OpenAI API error ({}): {}", status, error_msg));
        }

        from_openai_completion(&response_body)
    }
}
