//! Core conversation loop engine for open-multi-agent.
//!
//! [`AgentRunner`] handles:
//!  - Sending messages to the LLM adapter
//!  - Extracting tool-use blocks from the response
//!  - Executing tool calls in parallel via [`ToolExecutor`]
//!  - Appending tool results and looping back until `end_turn`

use crate::llm::adapter::LlmAdapter;
use crate::tool::executor::ToolExecutor;
use crate::tool::framework::ToolRegistry;
use crate::types::*;
use std::sync::Arc;
use std::time::Instant;

/// Static configuration for an [`AgentRunner`] instance.
pub struct RunnerOptions {
    pub model: String,
    pub system_prompt: Option<String>,
    pub max_turns: Option<usize>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub allowed_tools: Option<Vec<String>>,
    pub agent_name: Option<String>,
    pub agent_role: Option<String>,
}

/// The aggregated result returned when a full run completes.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub messages: Vec<LlmMessage>,
    pub output: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub token_usage: TokenUsage,
    pub turns: usize,
}

/// Drives a full agentic conversation: LLM calls, tool execution, and looping.
pub struct AgentRunner {
    adapter: Box<dyn LlmAdapter>,
    registry: Arc<ToolRegistry>,
    executor: Arc<ToolExecutor>,
    options: RunnerOptions,
    max_turns: usize,
}

impl AgentRunner {
    pub fn new(
        adapter: Box<dyn LlmAdapter>,
        registry: Arc<ToolRegistry>,
        executor: Arc<ToolExecutor>,
        options: RunnerOptions,
    ) -> Self {
        let max_turns = options.max_turns.unwrap_or(10);
        Self {
            adapter,
            registry,
            executor,
            options,
            max_turns,
        }
    }

    /// Run a complete conversation starting from `messages`.
    pub async fn run(&self, messages: Vec<LlmMessage>) -> Result<RunResult, String> {
        let mut conversation_messages = messages;
        let mut total_usage = TokenUsage::default();
        let mut all_tool_calls: Vec<ToolCallRecord> = Vec::new();
        let mut final_output = String::new();
        let mut turns = 0;

        // Build tool defs once
        let all_defs = self.registry.to_tool_defs();
        let tool_defs: Vec<LlmToolDef> = match &self.options.allowed_tools {
            Some(allowed) => all_defs
                .into_iter()
                .filter(|d| allowed.contains(&d.name))
                .collect(),
            None => all_defs,
        };

        let chat_options = LlmChatOptions {
            model: self.options.model.clone(),
            tools: if tool_defs.is_empty() {
                None
            } else {
                Some(tool_defs)
            },
            max_tokens: self.options.max_tokens,
            temperature: self.options.temperature,
            system_prompt: self.options.system_prompt.clone(),
        };

        loop {
            if turns >= self.max_turns {
                break;
            }
            turns += 1;

            // Step 1: Call the LLM
            let response = self
                .adapter
                .chat(&conversation_messages, &chat_options)
                .await?;

            total_usage = total_usage.add(&response.usage);

            // Step 2: Build assistant message
            let assistant_message = LlmMessage {
                role: Role::Assistant,
                content: response.content.clone(),
            };
            conversation_messages.push(assistant_message);

            // Extract text and tool-use blocks
            let turn_text = extract_text(&response.content);
            let tool_use_blocks = extract_tool_use_blocks(&response.content);

            // Step 3: Check if we should continue
            if tool_use_blocks.is_empty() {
                final_output = turn_text;
                break;
            }

            // Step 4: Execute all tool calls in parallel
            let tool_context = self.build_tool_context();
            let mut tool_result_blocks: Vec<ContentBlock> = Vec::new();

            for block in &tool_use_blocks {
                let start = Instant::now();
                let result = self
                    .executor
                    .execute(&block.name, block.input.clone(), &tool_context)
                    .await;
                let duration = start.elapsed().as_millis() as u64;

                all_tool_calls.push(ToolCallRecord {
                    tool_name: block.name.clone(),
                    input: block.input.clone(),
                    output: result.data.clone(),
                    duration,
                });

                tool_result_blocks.push(ContentBlock::ToolResult(ToolResultBlock {
                    tool_use_id: block.id.clone(),
                    content: result.data,
                    is_error: result.is_error,
                }));
            }

            // Step 5: Append tool results as user message
            let tool_result_message = LlmMessage {
                role: Role::User,
                content: tool_result_blocks,
            };
            conversation_messages.push(tool_result_message);
        }

        // If loop exited due to max_turns, extract last output
        if final_output.is_empty() {
            if let Some(last_assistant) = conversation_messages
                .iter()
                .rev()
                .find(|m| m.role == Role::Assistant)
            {
                final_output = extract_text(&last_assistant.content);
            }
        }

        let initial_len = 1; // The initial user message
        let new_messages = if conversation_messages.len() > initial_len {
            conversation_messages[initial_len..].to_vec()
        } else {
            Vec::new()
        };

        Ok(RunResult {
            messages: new_messages,
            output: final_output,
            tool_calls: all_tool_calls,
            token_usage: total_usage,
            turns,
        })
    }

    fn build_tool_context(&self) -> ToolUseContext {
        ToolUseContext {
            agent: AgentInfo {
                name: self.options.agent_name.clone().unwrap_or_else(|| "runner".to_string()),
                role: self.options.agent_role.clone().unwrap_or_else(|| "assistant".to_string()),
                model: self.options.model.clone(),
            },
            cwd: None,
            metadata: None,
        }
    }
}

fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text(t) = b {
                Some(t.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn extract_tool_use_blocks(content: &[ContentBlock]) -> Vec<&ToolUseBlock> {
    content
        .iter()
        .filter_map(|b| {
            if let ContentBlock::ToolUse(tu) = b {
                Some(tu)
            } else {
                None
            }
        })
        .collect()
}
