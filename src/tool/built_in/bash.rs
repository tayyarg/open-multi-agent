//! Built-in bash tool.
//!
//! Executes a shell command and returns its stdout + stderr. Supports an
//! optional timeout and a custom working directory.

use crate::tool::framework::ToolDefinition;
use crate::types::{ToolResult, ToolUseContext};
use async_trait::async_trait;
use std::time::Duration;
use tokio::process::Command;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;

pub struct BashTool;

#[async_trait]
impl ToolDefinition for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command and return its stdout and stderr. \
         Use this for file system operations, running scripts, installing packages, \
         and any task that requires shell access. \
         The command runs in a non-interactive shell (bash -c). \
         Long-running commands should use the timeout parameter."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute."
                },
                "timeout": {
                    "type": "number",
                    "description": format!("Timeout in milliseconds before the command is forcibly killed. Defaults to {} ms.", DEFAULT_TIMEOUT_MS)
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory in which to run the command."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolUseContext,
    ) -> ToolResult {
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => {
                return ToolResult {
                    data: "Missing required parameter: command".to_string(),
                    is_error: true,
                };
            }
        };

        let timeout_ms = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_MS);

        let cwd = input.get("cwd").and_then(|v| v.as_str()).map(String::from);

        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&command);

        if let Some(dir) = &cwd {
            cmd.current_dir(dir);
        }

        let result = tokio::time::timeout(Duration::from_millis(timeout_ms), cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(1);

                let combined = build_output(&stdout, &stderr, exit_code);
                ToolResult {
                    data: combined,
                    is_error: exit_code != 0,
                }
            }
            Ok(Err(e)) => ToolResult {
                data: format!("Failed to execute command: {}", e),
                is_error: true,
            },
            Err(_) => ToolResult {
                data: format!("Command timed out after {}ms", timeout_ms),
                is_error: true,
            },
        }
    }
}

fn build_output(stdout: &str, stderr: &str, exit_code: i32) -> String {
    let mut parts = Vec::new();

    if !stdout.is_empty() {
        parts.push(stdout.to_string());
    }

    if !stderr.is_empty() {
        if !stdout.is_empty() {
            parts.push(format!("--- stderr ---\n{}", stderr));
        } else {
            parts.push(stderr.to_string());
        }
    }

    if parts.is_empty() {
        return if exit_code == 0 {
            "(command completed with no output)".to_string()
        } else {
            format!("(command exited with code {}, no output)", exit_code)
        };
    }

    if exit_code != 0 {
        parts.push(format!("\n(exit code: {})", exit_code));
    }

    parts.join("\n")
}
