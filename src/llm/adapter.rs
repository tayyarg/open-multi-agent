//! LLM adapter factory.
//!
//! Provides a [`create_adapter`] factory that returns the correct concrete
//! implementation based on the requested provider.

use crate::types::{LlmChatOptions, LlmMessage, LlmResponse, Provider, StreamEvent};
use async_trait::async_trait;

/// Provider-agnostic interface that every LLM backend must implement.
#[async_trait]
pub trait LlmAdapter: Send + Sync {
    /// Human-readable provider name, e.g. `"anthropic"` or `"openai"`.
    fn name(&self) -> &str;

    /// Send a chat request and return the complete response.
    async fn chat(
        &self,
        messages: &[LlmMessage],
        options: &LlmChatOptions,
    ) -> Result<LlmResponse, String>;

    /// Send a chat request and return stream events.
    /// Default implementation falls back to non-streaming chat.
    async fn stream(
        &self,
        messages: &[LlmMessage],
        options: &LlmChatOptions,
    ) -> Result<Vec<StreamEvent>, String> {
        match self.chat(messages, options).await {
            Ok(response) => {
                let mut events = Vec::new();
                for block in &response.content {
                    if let crate::types::ContentBlock::Text(text) = block {
                        events.push(StreamEvent::Text(text.text.clone()));
                    }
                }
                events.push(StreamEvent::Done(Box::new(response)));
                Ok(events)
            }
            Err(e) => Ok(vec![StreamEvent::Error(e)]),
        }
    }
}

/// Instantiate the appropriate [`LlmAdapter`] for the given provider.
///
/// API keys fall back to the standard environment variables when not supplied.
pub fn create_adapter(
    provider: Provider,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Box<dyn LlmAdapter> {
    match provider {
        Provider::Anthropic => {
            Box::new(super::anthropic::AnthropicAdapter::new(api_key, base_url))
        }
        Provider::OpenAI => {
            Box::new(super::openai::OpenAIAdapter::new(api_key, base_url))
        }
        Provider::Copilot => {
            if base_url.is_some() {
                eprintln!("[open-multi-agent] baseURL is not supported for the copilot provider and will be ignored.");
            }
            Box::new(super::copilot::CopilotAdapter::new(api_key))
        }
    }
}
