//! Anthropic Messages content blocks: model responses (standard scaffolding
//! and Claude Code alike) and Claude Code's tool results.
//!
//! | Block | Bench part |
//! | --- | --- |
//! | `text` | `text` (dropped when empty) |
//! | `thinking` | `reasoning` (signature dropped) |
//! | `redacted_thinking` | `reasoning_opaque` (unless scrubbed) |
//! | `tool_use` | `tool_call`, client-executed |
//! | `server_tool_use` | `tool_call`, server-executed |
//! | `*_tool_result` | `server_tool_result` with the result as JSON text |
//! | `tool_result` (in a user turn) | `tool_result`: text contents kept, images dropped, `is_error` → `error` |
//! | anything else | `unknown` |

use a2a_bench_corpus::world::StopReason;
use a2a_bench_format::message::{
    AssistantPart, ResultContent, ToolCall, ToolExecution, ToolOutcome, ToolResult,
};
use serde_json::Value;

use super::{Response, arguments, opaque, text, visible};

/// A whole Anthropic message object.
pub fn convert(raw: &Value) -> Response {
    let parts: Vec<AssistantPart> = raw
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter_map(block).collect())
        .unwrap_or_default();
    let stop = stop_reason(raw.get("stop_reason").and_then(Value::as_str), &parts);
    Response { parts, stop }
}

/// The stop reason, from `stop_reason` or, when it is missing, from whether
/// the message calls a tool.
pub fn stop_reason(reason: Option<&str>, parts: &[AssistantPart]) -> StopReason {
    match reason {
        Some("end_turn") => StopReason::EndTurn,
        Some("tool_use") => StopReason::ToolUse,
        Some("max_tokens") => StopReason::MaxTokens,
        Some("stop_sequence") => StopReason::StopSequence,
        Some("refusal") => StopReason::Refusal,
        Some(_) => StopReason::Other,
        None => super::stop_for(parts, StopReason::EndTurn),
    }
}

/// One assistant content block.
pub fn block(raw: &Value) -> Option<AssistantPart> {
    let kind = raw.get("type").and_then(Value::as_str).unwrap_or("");
    let string = |name: &str| raw.get(name).and_then(Value::as_str).unwrap_or("");
    match kind {
        "text" => text(string("text")),
        "thinking" => visible(string("thinking")),
        "redacted_thinking" => opaque(string("data")),
        "tool_use" | "server_tool_use" => Some(AssistantPart::ToolCall(ToolCall {
            call_id: string("id").to_owned(),
            name: string("name").to_owned(),
            arguments: arguments(raw.get("input").unwrap_or(&Value::Null)),
            execution: if kind == "tool_use" {
                ToolExecution::Client
            } else {
                ToolExecution::Server
            },
        })),
        kind if kind.ends_with("_tool_result") => {
            let content = raw.get("content").unwrap_or(&Value::Null).to_string();
            Some(AssistantPart::ServerToolResult(ToolResult {
                call_id: string("tool_use_id").to_owned(),
                content: vec![ResultContent::Text { text: content }],
                outcome: ToolOutcome::Success,
            }))
        }
        "image" => None,
        _ => Some(AssistantPart::Unknown),
    }
}

/// A `tool_result` block of a user turn.
pub fn tool_result(raw: &Value) -> ToolResult {
    let call_id = raw
        .get("tool_use_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let content = match raw.get("content") {
        Some(Value::String(text)) => vec![ResultContent::Text { text: text.clone() }],
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| ResultContent::Text {
                            text: text.to_owned(),
                        })
                }
                // Images are scrubbed or held elsewhere: dropped.
                Some("image") => None,
                Some(_) => Some(ResultContent::Unknown),
                None => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let outcome = if raw.get("is_error").and_then(Value::as_bool) == Some(true) {
        ToolOutcome::Error
    } else {
        ToolOutcome::Success
    };
    ToolResult {
        call_id,
        content,
        outcome,
    }
}
