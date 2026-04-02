//! Shared OpenAI wire-format conversion helpers.
//!
//! Both the OpenAI and Copilot adapters use the OpenAI Chat Completions API
//! format. This module contains the common conversion logic.

use crate::types::{ContentBlock, LlmMessage, LlmResponse, LlmToolDef, Role, TextBlock, ToolUseBlock};

/// Convert a framework [`LlmToolDef`] to an OpenAI tool JSON object.
pub fn to_openai_tool(tool: &LlmToolDef) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

/// Convert framework messages into OpenAI message format.
pub fn to_openai_messages(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
    let mut result = Vec::new();

    for msg in messages {
        match msg.role {
            Role::Assistant => {
                result.push(to_openai_assistant_message(msg));
            }
            Role::User => {
                let has_tool_results = msg.content.iter().any(|b| matches!(b, ContentBlock::ToolResult(_)));

                if !has_tool_results {
                    result.push(to_openai_user_message(msg));
                } else {
                    let non_tool_blocks: Vec<&ContentBlock> = msg
                        .content
                        .iter()
                        .filter(|b| !matches!(b, ContentBlock::ToolResult(_)))
                        .collect();

                    if !non_tool_blocks.is_empty() {
                        let text = non_tool_blocks
                            .iter()
                            .filter_map(|b| {
                                if let ContentBlock::Text(t) = b {
                                    Some(t.text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        if !text.is_empty() {
                            result.push(serde_json::json!({
                                "role": "user",
                                "content": text,
                            }));
                        }
                    }

                    for block in &msg.content {
                        if let ContentBlock::ToolResult(tr) = block {
                            result.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tr.tool_use_id,
                                "content": tr.content,
                            }));
                        }
                    }
                }
            }
        }
    }

    result
}

fn to_openai_user_message(msg: &LlmMessage) -> serde_json::Value {
    if msg.content.len() == 1 {
        if let ContentBlock::Text(t) = &msg.content[0] {
            return serde_json::json!({
                "role": "user",
                "content": t.text,
            });
        }
    }

    let parts: Vec<serde_json::Value> = msg
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(t) => Some(serde_json::json!({
                "type": "text",
                "text": t.text,
            })),
            ContentBlock::Image(img) => Some(serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", img.source.media_type, img.source.data),
                }
            })),
            _ => None,
        })
        .collect();

    serde_json::json!({
        "role": "user",
        "content": parts,
    })
}

fn to_openai_assistant_message(msg: &LlmMessage) -> serde_json::Value {
    let mut tool_calls = Vec::new();
    let mut text_parts = Vec::new();

    for block in &msg.content {
        match block {
            ContentBlock::ToolUse(tu) => {
                tool_calls.push(serde_json::json!({
                    "id": tu.id,
                    "type": "function",
                    "function": {
                        "name": tu.name,
                        "arguments": serde_json::to_string(&tu.input).unwrap_or_default(),
                    }
                }));
            }
            ContentBlock::Text(t) => {
                text_parts.push(t.text.clone());
            }
            _ => {}
        }
    }

    let mut assistant_msg = serde_json::json!({
        "role": "assistant",
        "content": if text_parts.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(text_parts.join("")) },
    });

    if !tool_calls.is_empty() {
        assistant_msg["tool_calls"] = serde_json::Value::Array(tool_calls);
    }

    assistant_msg
}

/// Convert an OpenAI completion response into a framework [`LlmResponse`].
pub fn from_openai_completion(completion: &serde_json::Value) -> Result<LlmResponse, String> {
    let choice = completion
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| "OpenAI returned a completion with no choices".to_string())?;

    let message = choice
        .get("message")
        .ok_or_else(|| "No message in choice".to_string())?;

    let mut content = Vec::new();

    if let Some(text) = message.get("content").and_then(|c| c.as_str()) {
        content.push(ContentBlock::Text(TextBlock {
            text: text.to_string(),
        }));
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let func = tc.get("function").unwrap_or(&serde_json::Value::Null);
            let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let args_str = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
            let input = serde_json::from_str(args_str).unwrap_or(serde_json::Value::Object(Default::default()));

            content.push(ContentBlock::ToolUse(ToolUseBlock { id, name, input }));
        }
    }

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|fr| fr.as_str())
        .unwrap_or("stop");

    let usage = completion.get("usage").unwrap_or(&serde_json::Value::Null);
    let input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    Ok(LlmResponse {
        id: completion.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        content,
        model: completion.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        stop_reason: normalize_finish_reason(finish_reason),
        usage: crate::types::TokenUsage {
            input_tokens,
            output_tokens,
        },
    })
}

/// Normalize an OpenAI `finish_reason` string to the framework's canonical vocabulary.
pub fn normalize_finish_reason(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "length" => "max_tokens".to_string(),
        "content_filter" => "content_filter".to_string(),
        other => other.to_string(),
    }
}

/// Prepend a system message when `system_prompt` is provided.
pub fn build_openai_message_list(
    messages: &[LlmMessage],
    system_prompt: Option<&str>,
) -> Vec<serde_json::Value> {
    let mut result = Vec::new();

    if let Some(prompt) = system_prompt {
        if !prompt.is_empty() {
            result.push(serde_json::json!({
                "role": "system",
                "content": prompt,
            }));
        }
    }

    result.extend(to_openai_messages(messages));
    result
}
