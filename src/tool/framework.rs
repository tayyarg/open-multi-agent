//! Tool definition framework for open-multi-agent.
//!
//! Provides the core primitives for declaring, registering, and converting
//! tools to the JSON Schema format that LLM APIs expect.

use crate::types::{LlmToolDef, ToolResult, ToolUseContext};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

/// A tool registered with the framework.
///
/// Implement this trait to create custom tools. The `input_schema` method
/// returns a JSON Schema object used for documentation/validation. The `execute`
/// method performs the actual work.
#[async_trait]
pub trait ToolDefinition: Send + Sync {
    /// The unique name of this tool.
    fn name(&self) -> &str;

    /// A human-readable description of what this tool does.
    fn description(&self) -> &str;

    /// JSON Schema object describing the tool's input parameter.
    fn input_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given input and context.
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolUseContext,
    ) -> ToolResult;
}

/// Registry that holds a set of named tools and can produce the JSON Schema
/// representation expected by LLM APIs (Anthropic, OpenAI, etc.).
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolDefinition>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Add a tool to the registry. Panics if a tool with the same name has
    /// already been registered.
    pub fn register(&mut self, tool: Arc<dyn ToolDefinition>) {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            panic!(
                "ToolRegistry: a tool named \"{}\" is already registered. \
                 Use a unique name or deregister the existing one first.",
                name
            );
        }
        self.tools.insert(name, tool);
    }

    /// Return a tool by name, or `None` if not found.
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolDefinition>> {
        self.tools.get(name).cloned()
    }

    /// Return all registered tool definitions as a vec.
    pub fn list(&self) -> Vec<Arc<dyn ToolDefinition>> {
        self.tools.values().cloned().collect()
    }

    /// Return true when a tool with the given name is registered.
    pub fn has(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Remove a tool by name. No-op if the tool was not registered.
    pub fn deregister(&mut self, name: &str) {
        self.tools.remove(name);
    }

    /// Convert all registered tools to the [`LlmToolDef`] format used by LLM
    /// adapters.
    pub fn to_tool_defs(&self) -> Vec<LlmToolDef> {
        self.tools
            .values()
            .map(|tool| LlmToolDef {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
