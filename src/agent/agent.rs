//! High-level Agent class for open-multi-agent.
//!
//! [`Agent`] wraps [`AgentRunner`] with:
//!  - Persistent conversation history (`prompt()`)
//!  - Fresh-conversation semantics (`run()`)
//!  - Full lifecycle state tracking

use crate::agent::runner::{AgentRunner, RunnerOptions};
use crate::llm::adapter::create_adapter;
use crate::tool::executor::ToolExecutor;
use crate::tool::framework::{ToolDefinition, ToolRegistry};
use crate::types::*;
use std::sync::Arc;
use tokio::sync::Mutex;

/// High-level wrapper around [`AgentRunner`] that manages conversation
/// history, state transitions, and tool lifecycle.
pub struct Agent {
    pub name: String,
    pub config: AgentConfig,
    runner: Mutex<Option<AgentRunner>>,
    state: Mutex<AgentState>,
    message_history: Mutex<Vec<LlmMessage>>,
    tool_registry: Arc<ToolRegistry>,
    tool_executor: Arc<ToolExecutor>,
}

impl Agent {
    pub fn new(
        config: AgentConfig,
        tool_registry: Arc<ToolRegistry>,
        tool_executor: Arc<ToolExecutor>,
    ) -> Self {
        let name = config.name.clone();
        Self {
            name,
            config,
            runner: Mutex::new(None),
            state: Mutex::new(AgentState {
                status: AgentStatus::Idle,
                messages: Vec::new(),
                token_usage: TokenUsage::default(),
                error: None,
            }),
            message_history: Mutex::new(Vec::new()),
            tool_registry,
            tool_executor,
        }
    }

    /// Lazily create the [`AgentRunner`].
    async fn get_or_create_runner(&self) -> Result<(), String> {
        let mut runner_guard = self.runner.lock().await;
        if runner_guard.is_some() {
            return Ok(());
        }

        let provider = self.config.provider.unwrap_or(Provider::Anthropic);
        let adapter = create_adapter(
            provider,
            self.config.api_key.as_deref(),
            self.config.base_url.as_deref(),
        );

        let runner_options = RunnerOptions {
            model: self.config.model.clone(),
            system_prompt: self.config.system_prompt.clone(),
            max_turns: self.config.max_turns,
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
            allowed_tools: if self.config.tools.is_empty() {
                None
            } else {
                Some(self.config.tools.clone())
            },
            agent_name: Some(self.name.clone()),
            agent_role: self
                .config
                .system_prompt
                .as_ref()
                .map(|s| s.chars().take(50).collect::<String>())
                .or_else(|| Some("assistant".to_string())),
        };

        *runner_guard = Some(AgentRunner::new(
            adapter,
            self.tool_registry.clone(),
            self.tool_executor.clone(),
            runner_options,
        ));

        Ok(())
    }

    /// Run `prompt` in a fresh conversation (history is NOT used).
    pub async fn run(&self, prompt: &str) -> AgentRunResult {
        let messages = vec![LlmMessage {
            role: Role::User,
            content: vec![ContentBlock::Text(TextBlock {
                text: prompt.to_string(),
            })],
        }];

        self.execute_run(messages).await
    }

    /// Run `prompt` as part of the ongoing conversation.
    pub async fn prompt(&self, message: &str) -> AgentRunResult {
        let user_message = LlmMessage {
            role: Role::User,
            content: vec![ContentBlock::Text(TextBlock {
                text: message.to_string(),
            })],
        };

        {
            let mut history = self.message_history.lock().await;
            history.push(user_message);
        }

        let messages = self.message_history.lock().await.clone();
        let result = self.execute_run(messages).await;

        // Persist new messages into history
        {
            let mut history = self.message_history.lock().await;
            for msg in &result.messages {
                history.push(msg.clone());
            }
        }

        result
    }

    /// Return a snapshot of the current agent state.
    pub async fn get_state(&self) -> AgentState {
        self.state.lock().await.clone()
    }

    /// Return a copy of the persistent message history.
    pub async fn get_history(&self) -> Vec<LlmMessage> {
        self.message_history.lock().await.clone()
    }

    /// Clear the persistent conversation history and reset state to idle.
    pub async fn reset(&self) {
        *self.message_history.lock().await = Vec::new();
        *self.state.lock().await = AgentState {
            status: AgentStatus::Idle,
            messages: Vec::new(),
            token_usage: TokenUsage::default(),
            error: None,
        };
    }

    /// Register a new tool at runtime.
    pub fn add_tool(&self, _tool: Arc<dyn ToolDefinition>) {
        // Note: In Rust, the registry is shared via Arc, so runtime modification
        // would require interior mutability. This is a placeholder for the API.
    }

    /// Build a [`ToolUseContext`] that identifies this agent.
    pub fn build_tool_context(&self) -> ToolUseContext {
        ToolUseContext {
            agent: AgentInfo {
                name: self.name.clone(),
                role: self
                    .config
                    .system_prompt
                    .as_ref()
                    .map(|s| s.chars().take(60).collect())
                    .unwrap_or_else(|| "assistant".to_string()),
                model: self.config.model.clone(),
            },
            cwd: None,
            metadata: None,
        }
    }

    // --- Private execution core ---

    async fn execute_run(&self, messages: Vec<LlmMessage>) -> AgentRunResult {
        self.transition_to(AgentStatus::Running).await;

        if let Err(e) = self.get_or_create_runner().await {
            self.transition_to_error(&e).await;
            return AgentRunResult {
                success: false,
                output: e,
                messages: Vec::new(),
                token_usage: TokenUsage::default(),
                tool_calls: Vec::new(),
            };
        }

        let runner_guard = self.runner.lock().await;
        let runner = runner_guard.as_ref().unwrap();

        match runner.run(messages).await {
            Ok(result) => {
                {
                    let mut state = self.state.lock().await;
                    state.token_usage = state.token_usage.add(&result.token_usage);
                }
                self.transition_to(AgentStatus::Completed).await;

                AgentRunResult {
                    success: true,
                    output: result.output,
                    messages: result.messages,
                    token_usage: result.token_usage,
                    tool_calls: result.tool_calls,
                }
            }
            Err(e) => {
                self.transition_to_error(&e).await;
                AgentRunResult {
                    success: false,
                    output: e,
                    messages: Vec::new(),
                    token_usage: TokenUsage::default(),
                    tool_calls: Vec::new(),
                }
            }
        }
    }

    async fn transition_to(&self, status: AgentStatus) {
        self.state.lock().await.status = status;
    }

    async fn transition_to_error(&self, error: &str) {
        let mut state = self.state.lock().await;
        state.status = AgentStatus::Error;
        state.error = Some(error.to_string());
    }
}
