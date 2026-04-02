//! Parallel tool executor with concurrency control and error isolation.
//!
//! Validates input, enforces a maximum concurrency limit using a semaphore,
//! tracks execution duration, and surfaces any execution errors as ToolResult
//! objects rather than panics.

use crate::tool::framework::ToolRegistry;
use crate::types::{ToolResult, ToolUseContext};
use crate::utils::semaphore::Semaphore;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Options for the tool executor.
pub struct ToolExecutorOptions {
    /// Maximum number of tool calls that may run in parallel. Defaults to 4.
    pub max_concurrency: usize,
}

impl Default for ToolExecutorOptions {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
        }
    }
}

/// Describes one call in a batch.
pub struct BatchToolCall {
    /// Caller-assigned ID used as the key in the result map.
    pub id: String,
    /// Registered tool name.
    pub name: String,
    /// Raw (unparsed) input object from the LLM.
    pub input: serde_json::Value,
}

/// Executes tools from a [`ToolRegistry`], enforcing a concurrency limit for
/// batch execution.
///
/// All errors — including unknown tool names and execution exceptions — are
/// caught and returned as `ToolResult` objects with `is_error: true`.
pub struct ToolExecutor {
    registry: Arc<ToolRegistry>,
    semaphore: Semaphore,
}

impl ToolExecutor {
    pub fn new(registry: Arc<ToolRegistry>, options: Option<ToolExecutorOptions>) -> Self {
        let opts = options.unwrap_or_default();
        Self {
            registry,
            semaphore: Semaphore::new(opts.max_concurrency),
        }
    }

    /// Execute a single tool by name.
    ///
    /// Errors are caught and returned as a [`ToolResult`] with `is_error: true`.
    pub async fn execute(
        &self,
        tool_name: &str,
        input: serde_json::Value,
        context: &ToolUseContext,
    ) -> ToolResult {
        let tool = match self.registry.get(tool_name) {
            Some(t) => t,
            None => {
                return ToolResult {
                    data: format!(
                        "Tool \"{}\" is not registered in the ToolRegistry.",
                        tool_name
                    ),
                    is_error: true,
                };
            }
        };

        let start = Instant::now();
        let result = tool.execute(input, context).await;
        let _duration = start.elapsed();
        result
    }

    /// Execute multiple tool calls in parallel, honouring the concurrency limit.
    ///
    /// Returns a `HashMap` from call ID to result.
    pub async fn execute_batch(
        &self,
        calls: Vec<BatchToolCall>,
        context: &ToolUseContext,
    ) -> HashMap<String, ToolResult> {
        let mut handles = Vec::new();

        for call in calls {
            let sem = self.semaphore.clone();
            let registry = self.registry.clone();
            let ctx = context.clone();

            let handle = tokio::spawn(async move {
                sem.acquire().await;
                let tool = registry.get(&call.name);
                let result = match tool {
                    Some(t) => t.execute(call.input, &ctx).await,
                    None => ToolResult {
                        data: format!(
                            "Tool \"{}\" is not registered in the ToolRegistry.",
                            call.name
                        ),
                        is_error: true,
                    },
                };
                sem.release().await;
                (call.id, result)
            });
            handles.push(handle);
        }

        let mut results = HashMap::new();
        for handle in handles {
            if let Ok((id, result)) = handle.await {
                results.insert(id, result);
            }
        }
        results
    }
}
