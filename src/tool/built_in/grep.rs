//! Built-in grep tool.
//!
//! Searches for a regex pattern in files. Prefers the `rg` (ripgrep) binary
//! when available for performance; falls back to a pure Rust recursive
//! implementation.

use crate::tool::framework::ToolDefinition;
use crate::types::{ToolResult, ToolUseContext};
use async_trait::async_trait;
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const DEFAULT_MAX_RESULTS: usize = 100;
const SKIP_DIRS: &[&str] = &[".git", ".svn", ".hg", "node_modules", ".next", "dist", "build", "target"];

pub struct GrepTool;

#[async_trait]
impl ToolDefinition for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a regular-expression pattern in one or more files. \
         Returns matching lines with their file paths and 1-based line numbers. \
         Use the `glob` parameter to restrict the search to specific file types. \
         Results are capped by `maxResults`."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file path to search in. Defaults to cwd."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.rs\")."
                },
                "maxResults": {
                    "type": "integer",
                    "description": format!("Maximum matching lines to return. Defaults to {}.", DEFAULT_MAX_RESULTS)
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolUseContext,
    ) -> ToolResult {
        let pattern = match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => p.to_string(),
            None => {
                return ToolResult {
                    data: "Missing required parameter: pattern".to_string(),
                    is_error: true,
                };
            }
        };

        let search_path = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let max_results = input
            .get("maxResults")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS);

        let glob_pattern = input.get("glob").and_then(|v| v.as_str()).map(String::from);

        // Try ripgrep first
        if is_ripgrep_available().await {
            return run_ripgrep(&pattern, search_path, glob_pattern.as_deref(), max_results).await;
        }

        // Fallback: pure Rust search
        let regex = match Regex::new(&pattern) {
            Ok(r) => r,
            Err(_) => {
                return ToolResult {
                    data: format!("Invalid regular expression: \"{}\"", pattern),
                    is_error: true,
                };
            }
        };

        run_rust_search(&regex, search_path, glob_pattern.as_deref(), max_results).await
    }
}

async fn is_ripgrep_available() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn run_ripgrep(
    pattern: &str,
    search_path: &str,
    glob: Option<&str>,
    max_results: usize,
) -> ToolResult {
    let mut args = vec![
        "--line-number".to_string(),
        "--no-heading".to_string(),
        "--color=never".to_string(),
        format!("--max-count={}", max_results),
    ];
    if let Some(g) = glob {
        args.push("--glob".to_string());
        args.push(g.to_string());
    }
    args.push("--".to_string());
    args.push(pattern.to_string());
    args.push(search_path.to_string());

    match Command::new("rg").args(&args).output().await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
            let code = output.status.code().unwrap_or(2);

            if code != 0 && code != 1 {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return ToolResult {
                    data: format!("ripgrep failed (exit {}): {}", code, stderr),
                    is_error: true,
                };
            }

            if stdout.is_empty() {
                return ToolResult {
                    data: "No matches found.".to_string(),
                    is_error: false,
                };
            }

            ToolResult {
                data: stdout,
                is_error: false,
            }
        }
        Err(e) => ToolResult {
            data: format!("ripgrep process error: {}", e),
            is_error: true,
        },
    }
}

async fn run_rust_search(
    regex: &Regex,
    search_path: &str,
    glob: Option<&str>,
    max_results: usize,
) -> ToolResult {
    let path = Path::new(search_path);
    let files = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        let mut collected = Vec::new();
        collect_files(path, glob, &mut collected).await;
        collected
    };

    let mut matches = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_default();

    for file in files {
        if matches.len() >= max_results {
            break;
        }

        let content = match tokio::fs::read_to_string(&file).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        for (i, line) in content.lines().enumerate() {
            if matches.len() >= max_results {
                break;
            }
            if regex.is_match(line) {
                let rel = file.strip_prefix(&cwd).unwrap_or(&file);
                matches.push(format!("{}:{}:{}", rel.display(), i + 1, line));
            }
        }
    }

    if matches.is_empty() {
        return ToolResult {
            data: "No matches found.".to_string(),
            is_error: false,
        };
    }

    let mut result = matches.join("\n");
    if matches.len() >= max_results {
        result.push_str(&format!(
            "\n\n(results capped at {}; use maxResults to raise the limit)",
            max_results
        ));
    }

    ToolResult {
        data: result,
        is_error: false,
    }
}

async fn collect_files(dir: &Path, glob: Option<&str>, results: &mut Vec<PathBuf>) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                Box::pin(collect_files(&path, glob, results)).await;
            }
        } else if path.is_file() {
            if glob.is_none() || matches_glob(&name, glob.unwrap()) {
                results.push(path);
            }
        }
    }
}

fn matches_glob(filename: &str, glob: &str) -> bool {
    let pattern = if let Some(stripped) = glob.strip_prefix("**/") {
        stripped
    } else {
        glob
    };
    let regex_source = pattern
        .replace('.', "\\.")
        .replace('*', ".*")
        .replace('?', ".");
    Regex::new(&format!("^{}$", regex_source))
        .map(|re| re.is_match(filename))
        .unwrap_or(false)
}
