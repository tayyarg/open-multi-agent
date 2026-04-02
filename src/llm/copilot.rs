//! GitHub Copilot adapter implementing [`LlmAdapter`].
//!
//! Uses the OpenAI-compatible Copilot Chat Completions endpoint at
//! `https://api.githubcopilot.com`.

use crate::llm::adapter::LlmAdapter;
use crate::llm::openai_common::{build_openai_message_list, from_openai_completion, to_openai_tool};
use crate::types::*;
use async_trait::async_trait;
use tokio::sync::Mutex;

const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

/// LLM adapter backed by the GitHub Copilot Chat Completions API.
pub struct CopilotAdapter {
    github_token: Mutex<Option<String>>,
    cached_token: Mutex<Option<String>>,
    token_expires_at: Mutex<u64>,
    client: reqwest::Client,
}

impl CopilotAdapter {
    pub fn new(api_key: Option<&str>) -> Self {
        let token = api_key
            .map(String::from)
            .or_else(|| std::env::var("GITHUB_COPILOT_TOKEN").ok())
            .or_else(|| std::env::var("GITHUB_TOKEN").ok());

        Self {
            github_token: Mutex::new(token),
            cached_token: Mutex::new(None),
            token_expires_at: Mutex::new(0),
            client: reqwest::Client::new(),
        }
    }

    async fn get_session_token(&self) -> Result<String, String> {
        let now = chrono::Utc::now().timestamp() as u64;
        {
            let cached = self.cached_token.lock().await;
            let expires = *self.token_expires_at.lock().await;
            if let Some(ref token) = *cached {
                if expires > now + 60 {
                    return Ok(token.clone());
                }
            }
        }

        self.do_refresh().await
    }

    async fn do_refresh(&self) -> Result<String, String> {
        let github_token = self.github_token.lock().await;
        let token = github_token
            .as_ref()
            .ok_or_else(|| {
                "No GitHub token available. Set GITHUB_COPILOT_TOKEN or GITHUB_TOKEN.".to_string()
            })?
            .clone();
        drop(github_token);

        let response = self
            .client
            .get(COPILOT_TOKEN_URL)
            .header("Authorization", format!("token {}", token))
            .header("Accept", "application/json")
            .header("User-Agent", "GitHubCopilotChat/0.28.0")
            .send()
            .await
            .map_err(|e| format!("Copilot token exchange failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Copilot token exchange failed ({}): {}",
                status, body
            ));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Copilot token response: {}", e))?;

        let session_token = body
            .get("token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| "No token in Copilot response".to_string())?
            .to_string();

        let expires_at = body
            .get("expires_at")
            .and_then(|e| e.as_u64())
            .unwrap_or(0);

        *self.cached_token.lock().await = Some(session_token.clone());
        *self.token_expires_at.lock().await = expires_at;

        Ok(session_token)
    }
}

#[async_trait]
impl LlmAdapter for CopilotAdapter {
    fn name(&self) -> &str {
        "copilot"
    }

    async fn chat(
        &self,
        messages: &[LlmMessage],
        options: &LlmChatOptions,
    ) -> Result<LlmResponse, String> {
        let session_token = self.get_session_token().await?;
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
            .post(format!("{}/chat/completions", COPILOT_BASE_URL))
            .header("Authorization", format!("Bearer {}", session_token))
            .header("Content-Type", "application/json")
            .header("Copilot-Integration-Id", "vscode-chat")
            .header("Editor-Version", "vscode/1.100.0")
            .header("Editor-Plugin-Version", "copilot-chat/0.42.2")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Copilot API request failed: {}", e))?;

        let status = response.status();
        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Copilot response: {}", e))?;

        if !status.is_success() {
            let error_msg = response_body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(format!("Copilot API error ({}): {}", status, error_msg));
        }

        from_openai_completion(&response_body)
    }
}
