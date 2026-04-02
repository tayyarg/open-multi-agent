//! Built-in file-edit tool.
//!
//! Performs a targeted string replacement inside an existing file.
//! The uniqueness invariant (one match unless replace_all is set) prevents the
//! common class of bugs where a generic pattern matches the wrong occurrence.

use crate::tool::framework::ToolDefinition;
use crate::types::{ToolResult, ToolUseContext};
use async_trait::async_trait;

pub struct FileEditTool;

#[async_trait]
impl ToolDefinition for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing a specific string with new content. \
         The `old_string` must appear verbatim in the file. \
         By default the tool errors if `old_string` appears more than once — \
         use `replace_all: true` to replace every occurrence."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to edit."
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to find and replace."
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement string."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "When true, replace every occurrence. Defaults to false."
                }
            },
            "required": ["path", "old_string", "new_string"]
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

        let old_string = match input.get("old_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult {
                    data: "Missing required parameter: old_string".to_string(),
                    is_error: true,
                };
            }
        };

        let new_string = match input.get("new_string").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolResult {
                    data: "Missing required parameter: new_string".to_string(),
                    is_error: true,
                };
            }
        };

        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let original = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(e) => {
                return ToolResult {
                    data: format!("Could not read \"{}\": {}", path, e),
                    is_error: true,
                };
            }
        };

        let occurrences = count_occurrences(&original, old_string);

        if occurrences == 0 {
            return ToolResult {
                data: format!(
                    "The string to replace was not found in \"{}\".\n\
                     Make sure `old_string` matches the file contents exactly, \
                     including indentation and line endings.",
                    path
                ),
                is_error: true,
            };
        }

        if occurrences > 1 && !replace_all {
            return ToolResult {
                data: format!(
                    "`old_string` appears {} times in \"{}\". \
                     Provide a more specific string or set `replace_all: true`.",
                    occurrences, path
                ),
                is_error: true,
            };
        }

        let updated = if replace_all {
            original.replace(old_string, new_string)
        } else {
            original.replacen(old_string, new_string, 1)
        };

        if let Err(e) = tokio::fs::write(path, &updated).await {
            return ToolResult {
                data: format!("Failed to write \"{}\": {}", path, e),
                is_error: true,
            };
        }

        let replaced_count = if replace_all { occurrences } else { 1 };
        ToolResult {
            data: format!(
                "Replaced {} occurrence{} in \"{}\".",
                replaced_count,
                if replaced_count == 1 { "" } else { "s" },
                path
            ),
            is_error: false,
        }
    }
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.matches(needle).count()
}
