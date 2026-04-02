//! Built-in file-write tool.
//!
//! Creates or overwrites a file with the supplied content. Parent directories
//! are created automatically (equivalent to `mkdir -p`).

use crate::tool::framework::ToolDefinition;
use crate::types::{ToolResult, ToolUseContext};
use async_trait::async_trait;
use std::path::Path;

pub struct FileWriteTool;

#[async_trait]
impl ToolDefinition for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file, creating it (and any missing parent directories) if it \
         does not already exist, or overwriting it if it does. \
         Prefer this tool for creating new files; use file_edit for targeted in-place edits \
         of existing files."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to write."
                },
                "content": {
                    "type": "string",
                    "description": "The full content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolUseContext,
    ) -> ToolResult {
        let path = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult {
                    data: "Missing required parameter: path".to_string(),
                    is_error: true,
                };
            }
        };

        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return ToolResult {
                    data: "Missing required parameter: content".to_string(),
                    is_error: true,
                };
            }
        };

        let existed = Path::new(path).exists();

        // Ensure parent directory exists
        if let Some(parent) = Path::new(path).parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult {
                    data: format!("Failed to create parent directory: {}", e),
                    is_error: true,
                };
            }
        }

        if let Err(e) = tokio::fs::write(path, content).await {
            return ToolResult {
                data: format!("Failed to write file \"{}\": {}", path, e),
                is_error: true,
            };
        }

        let line_count = content.split('\n').count();
        let byte_count = content.len();
        let action = if existed { "Updated" } else { "Created" };

        ToolResult {
            data: format!(
                "{} \"{}\" ({} line{}, {} bytes).",
                action,
                path,
                line_count,
                if line_count == 1 { "" } else { "s" },
                byte_count
            ),
            is_error: false,
        }
    }
}
