// Copyright 2024-2026 Reflective Labs

//! Token streaming and tool-call detection for `StreamGenerate`.
//!
//! The server generates text token-by-token and detects tool-call patterns
//! in the output. Tool execution is client-side: the client calls the tool,
//! appends the result as a `role=tool` message, and sends a new request.

use super::proto;

/// Formats chat messages into a Llama 3 chat template.
pub fn format_chat_template(
    messages: &[proto::ChatMessage],
    system_prompt: Option<&str>,
) -> String {
    let mut prompt = String::new();

    prompt.push_str("<|begin_of_text|>");

    // System prompt
    if let Some(sys) = system_prompt {
        prompt.push_str("<|start_header_id|>system<|end_header_id|>\n\n");
        prompt.push_str(sys);
        prompt.push_str("<|eot_id|>");
    }

    // Chat messages
    for msg in messages {
        let role = match proto::ChatRole::try_from(msg.role) {
            Ok(proto::ChatRole::System) => "system",
            Ok(proto::ChatRole::User) => "user",
            Ok(proto::ChatRole::Assistant) => "assistant",
            Ok(proto::ChatRole::Tool) => "tool",
            _ => "user",
        };

        prompt.push_str("<|start_header_id|>");
        prompt.push_str(role);
        prompt.push_str("<|end_header_id|>\n\n");
        prompt.push_str(&msg.content);
        prompt.push_str("<|eot_id|>");
    }

    // Assistant turn start
    prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");

    prompt
}

/// Detects tool-call patterns in generated text.
///
/// Looks for patterns like:
/// ```text
/// <tool_call>
/// {"name": "tool_name", "arguments": {...}}
/// </tool_call>
/// ```
///
/// Returns parsed tool calls if found.
pub fn detect_tool_calls(text: &str) -> Vec<DetectedToolCall> {
    let mut calls = Vec::new();

    let mut search_from = 0;
    while let Some(start) = text[search_from..].find("<tool_call>") {
        let abs_start = search_from + start;
        let after_tag = abs_start + "<tool_call>".len();

        if let Some(end) = text[after_tag..].find("</tool_call>") {
            let json_str = text[after_tag..after_tag + end].trim();

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                let name = parsed
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = parsed
                    .get("arguments")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".to_string());

                calls.push(DetectedToolCall {
                    call_id: format!("call_{}", calls.len()),
                    tool_name: name,
                    arguments_json: arguments,
                });
            }

            search_from = after_tag + end + "</tool_call>".len();
        } else {
            break;
        }
    }

    calls
}

/// A tool call detected in generated text.
pub struct DetectedToolCall {
    pub call_id: String,
    pub tool_name: String,
    pub arguments_json: String,
}

impl From<DetectedToolCall> for proto::ToolCallChunk {
    fn from(tc: DetectedToolCall) -> Self {
        Self {
            call_id: tc.call_id,
            tool_name: tc.tool_name,
            arguments_json: tc.arguments_json,
        }
    }
}

/// Builds the tool instruction block for the system prompt.
pub fn build_tool_instructions(tools: &[proto::ToolDefinition]) -> Option<String> {
    if tools.is_empty() {
        return None;
    }

    let mut instructions = String::from(
        "You have access to the following tools. To use a tool, output a tool_call block:\n\n\
         <tool_call>\n\
         {\"name\": \"tool_name\", \"arguments\": {\"arg\": \"value\"}}\n\
         </tool_call>\n\n\
         Available tools:\n\n",
    );

    for tool in tools {
        instructions.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
        if let Some(schema) = &tool.parameters_schema {
            let json = super::convert::prost_struct_to_json(schema);
            instructions.push_str(&format!("  Parameters: {}\n", json));
        }
    }

    Some(instructions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_chat_template_basic() {
        let messages = vec![proto::ChatMessage {
            role: proto::ChatRole::User.into(),
            content: "Hello".to_string(),
            tool_call_id: None,
        }];

        let result = format_chat_template(&messages, Some("You are helpful."));
        assert!(result.contains("<|begin_of_text|>"));
        assert!(result.contains("system"));
        assert!(result.contains("You are helpful."));
        assert!(result.contains("user"));
        assert!(result.contains("Hello"));
        assert!(result.ends_with("assistant<|end_header_id|>\n\n"));
    }

    #[test]
    fn test_detect_tool_calls() {
        let text = r#"Let me help you with that.
<tool_call>
{"name": "read_file", "arguments": {"path": "/tmp/test.rs"}}
</tool_call>"#;

        let calls = detect_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "read_file");
        assert!(calls[0].arguments_json.contains("/tmp/test.rs"));
    }

    #[test]
    fn test_detect_multiple_tool_calls() {
        let text = r#"<tool_call>
{"name": "tool_a", "arguments": {}}
</tool_call>
Some text
<tool_call>
{"name": "tool_b", "arguments": {"key": "val"}}
</tool_call>"#;

        let calls = detect_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].tool_name, "tool_a");
        assert_eq!(calls[1].tool_name, "tool_b");
    }

    #[test]
    fn test_no_tool_calls() {
        let text = "Just regular text without any tool calls.";
        let calls = detect_tool_calls(text);
        assert!(calls.is_empty());
    }
}
