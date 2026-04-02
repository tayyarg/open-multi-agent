//! Core type definitions for the open-multi-agent orchestration framework.
//!
//! All public types are exported from this single module. Downstream modules
//! import only what they need, keeping the dependency graph acyclic.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Content blocks
// ---------------------------------------------------------------------------

/// Plain-text content produced by a model or supplied by the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

/// A request by the model to invoke a named tool with a structured input.
/// The `id` is unique per turn and is referenced by [`ToolResultBlock`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// The result of executing a tool, keyed back to the originating
/// [`ToolUseBlock`] via `tool_use_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

/// A base64-encoded image passed to or returned from a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageBlock {
    pub source: ImageSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    pub media_type: String,
    pub data: String,
}

/// Union of all content block variants that may appear in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text(TextBlock),
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseBlock),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultBlock),
    #[serde(rename = "image")]
    Image(ImageBlock),
}

// ---------------------------------------------------------------------------
// LLM messages & responses
// ---------------------------------------------------------------------------

/// Role in a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// A single message in a conversation thread.
/// System messages are passed separately via [`LlmChatOptions::system_prompt`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

/// Token accounting for a single API call.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl TokenUsage {
    pub fn add(&self, other: &TokenUsage) -> TokenUsage {
        TokenUsage {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
        }
    }
}

/// Normalised response returned by every [`LlmAdapter`] implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub id: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: String,
    pub usage: TokenUsage,
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

/// A discrete event emitted during streaming generation.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Incremental text delta.
    Text(String),
    /// The model has begun or completed a tool-use block.
    ToolUse(ToolUseBlock),
    /// A tool result has been appended to the stream.
    ToolResult(ToolResultBlock),
    /// The stream has ended; data is the final [`LlmResponse`].
    Done(Box<LlmResponse>),
    /// An unrecoverable error occurred.
    Error(String),
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

/// The serialisable tool schema sent to the LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing the tool's `input` parameter.
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// Minimal descriptor for the agent that is invoking a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub role: String,
    pub model: String,
}

/// Context injected into every tool execution.
#[derive(Debug, Clone)]
pub struct ToolUseContext {
    /// High-level description of the agent invoking this tool.
    pub agent: AgentInfo,
    /// Working directory hint for file-system tools.
    pub cwd: Option<String>,
    /// Arbitrary caller-supplied metadata (session ID, request ID, etc.).
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Value returned by a tool's `execute` function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub data: String,
    #[serde(default)]
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

/// The set of LLM providers supported out of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Copilot,
    #[serde(rename = "openai")]
    OpenAI,
}

impl Default for Provider {
    fn default() -> Self {
        Provider::Anthropic
    }
}

/// Static configuration for a single agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub provider: Option<Provider>,
    /// Custom base URL for OpenAI-compatible APIs (Ollama, vLLM, LM Studio, etc.).
    pub base_url: Option<String>,
    /// API key override; falls back to the provider's standard env var.
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    /// Names of tools (from the tool registry) available to this agent.
    #[serde(default)]
    pub tools: Vec<String>,
    pub max_turns: Option<usize>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
}

/// Lifecycle state tracked during an agent run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Completed,
    Error,
}

/// The full state of an agent.
#[derive(Debug, Clone)]
pub struct AgentState {
    pub status: AgentStatus,
    pub messages: Vec<LlmMessage>,
    pub token_usage: TokenUsage,
    pub error: Option<String>,
}

/// A single recorded tool invocation within a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: String,
    /// Wall-clock duration in milliseconds.
    pub duration: u64,
}

/// The final result produced when an agent run completes (or fails).
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub success: bool,
    pub output: String,
    pub messages: Vec<LlmMessage>,
    pub token_usage: TokenUsage,
    pub tool_calls: Vec<ToolCallRecord>,
}

// ---------------------------------------------------------------------------
// Team
// ---------------------------------------------------------------------------

/// Static configuration for a team of cooperating agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    pub name: String,
    pub agents: Vec<AgentConfig>,
    #[serde(default)]
    pub shared_memory: bool,
    pub max_concurrency: Option<usize>,
}

/// Aggregated result for a full team run.
#[derive(Debug, Clone)]
pub struct TeamRunResult {
    pub success: bool,
    /// Keyed by agent name.
    pub agent_results: HashMap<String, AgentRunResult>,
    pub total_token_usage: TokenUsage,
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// Valid states for a [`Task`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

/// A discrete unit of work tracked by the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    /// Agent name responsible for executing this task.
    pub assignee: Option<String>,
    /// IDs of tasks that must complete before this one can start.
    pub depends_on: Option<Vec<String>>,
    pub result: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

/// Progress event emitted by the orchestrator during a run.
#[derive(Debug, Clone)]
pub struct OrchestratorEvent {
    pub event_type: OrchestratorEventType,
    pub agent: Option<String>,
    pub task: Option<String>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestratorEventType {
    AgentStart,
    AgentComplete,
    TaskStart,
    TaskComplete,
    Message,
    Error,
}

/// Top-level configuration for the orchestrator.
pub struct OrchestratorConfig {
    pub max_concurrency: Option<usize>,
    pub default_model: Option<String>,
    pub default_provider: Option<Provider>,
    pub default_base_url: Option<String>,
    pub default_api_key: Option<String>,
    pub on_progress: Option<Box<dyn Fn(OrchestratorEvent) + Send + Sync>>,
}

impl std::fmt::Debug for OrchestratorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrchestratorConfig")
            .field("max_concurrency", &self.max_concurrency)
            .field("default_model", &self.default_model)
            .field("default_provider", &self.default_provider)
            .field("default_base_url", &self.default_base_url)
            .field("default_api_key", &self.default_api_key)
            .field("on_progress", &self.on_progress.as_ref().map(|_| "<callback>"))
            .finish()
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: None,
            default_model: None,
            default_provider: None,
            default_base_url: None,
            default_api_key: None,
            on_progress: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// A single key-value record stored in a [`MemoryStore`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// LLM adapter options
// ---------------------------------------------------------------------------

/// Options shared by both chat and streaming calls.
#[derive(Debug, Clone)]
pub struct LlmChatOptions {
    pub model: String,
    pub tools: Option<Vec<LlmToolDef>>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub system_prompt: Option<String>,
}
