//! Built-in tool collection.
//!
//! Re-exports every built-in tool and provides a convenience function to
//! register them all with a [`ToolRegistry`] in one call.

pub mod bash;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod grep;

use crate::tool::framework::{ToolDefinition, ToolRegistry};
use std::sync::Arc;

/// Register all built-in tools with the given registry.
pub fn register_built_in_tools(registry: &mut ToolRegistry) {
    let tools: Vec<Arc<dyn ToolDefinition>> = vec![
        Arc::new(bash::BashTool),
        Arc::new(file_read::FileReadTool),
        Arc::new(file_write::FileWriteTool),
        Arc::new(file_edit::FileEditTool),
        Arc::new(grep::GrepTool),
    ];
    for tool in tools {
        registry.register(tool);
    }
}
