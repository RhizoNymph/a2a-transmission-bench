//! Gemini `generateContent` responses: the first candidate's parts.
//!
//! | Part | Bench part |
//! | --- | --- |
//! | `text` with `"thought": true` | `reasoning` |
//! | `text` | `text` |
//! | `functionCall` | `tool_call` (its `id`, or `<fallback>-<n>` when it has none) |
//! | `thoughtSignature` | `reasoning_opaque`, unless scrubbed |
//!
//! `finishReason` `MAX_TOKENS` is `max_tokens`, `SAFETY` / `RECITATION`
//! `refusal`; otherwise the stop is `tool_use` when a function is called.

use a2a_bench_corpus::world::StopReason;
use a2a_bench_format::message::{AssistantPart, ToolCall, ToolExecution};
use serde_json::Value;

use super::{Response, arguments, opaque, stop_for, text, visible};

pub fn convert(raw: &Value, fallback_id: &str) -> Response {
    let candidate = raw
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .unwrap_or(&Value::Null);
    let mut parts = Vec::new();
    let mut unnamed = 0usize;
    for part in candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(call) = part.get("functionCall") {
            let id = match call.get("id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => id.to_owned(),
                _ => {
                    unnamed += 1;
                    format!("{fallback_id}-{unnamed}")
                }
            };
            parts.push(AssistantPart::ToolCall(ToolCall {
                call_id: id,
                name: call
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                arguments: arguments(call.get("args").unwrap_or(&Value::Null)),
                execution: ToolExecution::Client,
            }));
        } else if let Some(content) = part.get("text").and_then(Value::as_str) {
            if part.get("thought").and_then(Value::as_bool) == Some(true) {
                parts.extend(visible(content));
            } else {
                parts.extend(text(content));
            }
        }
        if let Some(signature) = part.get("thoughtSignature").and_then(Value::as_str) {
            parts.extend(opaque(signature));
        }
    }
    let stop = match candidate.get("finishReason").and_then(Value::as_str) {
        Some("MAX_TOKENS") => StopReason::MaxTokens,
        Some("SAFETY") | Some("RECITATION") => StopReason::Refusal,
        _ => stop_for(&parts, StopReason::EndTurn),
    };
    Response { parts, stop }
}
