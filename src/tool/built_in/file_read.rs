//! Built-in file-read tool.
//!
//! Reads a file from disk and returns its contents with 1-based line numbers.
//! Supports reading a slice of lines via `offset` and `limit` for large files.

use crate::tool::framework::ToolDefinition;
use crate::types::{ToolResult, ToolUseContext};
use async_trait::async_trait;

pub struct FileReadTool;

#[async_trait]
impl ToolDefinition for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from disk. \
         Returns the file contents with line numbers prefixed in the format \"N\\t<line>\". \
         Use `offset` and `limit` to read large files in chunks."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to read."
                },
                "offset": {
                    "type": "integer",
                    "description": "1-based line number to start reading from."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return."
                }
            },
            "required": ["path"]
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

        let raw = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(e) => {
                return ToolResult {
                    data: format!("Could not read file \"{}\": {}", path, e),
                    is_error: true,
                };
            }
        };

        let mut lines: Vec<&str> = raw.split('\n').collect();
        // Remove trailing empty string from trailing newline
        if lines.last() == Some(&"") {
            lines.pop();
        }

        let total_lines = lines.len();
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|o| (o as usize).saturating_sub(1))
            .unwrap_or(0);

        if offset >= total_lines && total_lines > 0 {
            return ToolResult {
                data: format!(
                    "File \"{}\" has {} line{} but offset {} is beyond the end.",
                    path,
                    total_lines,
                    if total_lines == 1 { "" } else { "s" },
                    offset + 1
                ),
                is_error: true,
            };
        }

        let end_index = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|l| (offset + l as usize).min(total_lines))
            .unwrap_or(total_lines);

        let slice = &lines[offset..end_index];

        let numbered: String = slice
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}\t{}", offset + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        let meta = if end_index < total_lines {
            format!(
                "\n\n(showing lines {}–{} of {})",
                offset + 1,
                end_index,
                total_lines
            )
        } else {
            String::new()
        };

        ToolResult {
            data: format!("{}{}", numbered, meta),
            is_error: false,
        }
    }
}
