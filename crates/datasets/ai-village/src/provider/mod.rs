//! Provider-shaped model responses as bench assistant messages.
//!
//! `computer_use_turns.agent_messages` and `events.data.output` hold the raw
//! response of whichever provider the agent ran on. [`response`] tells the
//! shape apart and converts it:
//!
//! | Shape | Detected by | Module |
//! | --- | --- | --- |
//! | Anthropic Messages object | an object with a `content` array of typed blocks | [`anthropic`] |
//! | Anthropic content blocks | an array holding a `text`, `thinking`, `redacted_thinking`, `tool_use` or `server_tool_use` block (events' `data.output`) | [`anthropic`] |
//! | OpenAI Responses item list | an array | [`openai::responses`] |
//! | OpenAI chat completion message | an object with `role` (and no block array) | [`openai::chat`] |
//! | Gemini `generateContent` | an object with `candidates` | [`gemini`] |
//!
//! Scrub markers are not data: a `[BLOB_REMOVED]` signature is dropped
//! (no `reasoning_opaque`), and an image block is dropped. Encrypted
//! reasoning that survived the scrub is `reasoning_opaque`; signatures are
//! never stored. Empty text parts are dropped. Unknown blocks are kept as
//! `unknown` parts (no bytes).

pub mod anthropic;
pub mod gemini;
pub mod openai;

use a2a_bench_corpus::world::StopReason;
use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{AssistantPart, Body, ToolArguments};
use serde_json::Value;

/// The scrub marker for removed blobs (signatures, long base64).
pub const BLOB_REMOVED: &str = "[BLOB_REMOVED]";

/// One converted response.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub parts: Vec<AssistantPart>,
    pub stop: StopReason,
}

impl Response {
    pub fn body(&self) -> Body {
        Body::Assistant(self.parts.clone())
    }

    pub fn into_body(self) -> Body {
        Body::Assistant(self.parts)
    }

    /// The ids of the tool calls it makes, in order.
    pub fn call_ids(&self) -> Vec<String> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                AssistantPart::ToolCall(call) => Some(call.call_id.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Converts a raw response. `fallback_id` names tool calls the provider gave
/// no id (Gemini): the `n`th becomes `<fallback_id>-<n>`.
pub fn response(raw: &Value, fallback_id: &str) -> Response {
    match raw {
        Value::Array(items) if items.iter().any(anthropic_block) => {
            anthropic::convert(&serde_json::json!({ "content": items }))
        }
        Value::Array(items) => openai::responses::convert(items),
        Value::Object(members) if members.contains_key("candidates") => {
            gemini::convert(raw, fallback_id)
        }
        Value::Object(members)
            if members
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| blocks.iter().all(|block| block.get("type").is_some())) =>
        {
            anthropic::convert(raw)
        }
        Value::Object(members) if members.contains_key("role") => openai::chat::convert(raw),
        Value::Null => Response {
            parts: Vec::new(),
            stop: StopReason::Other,
        },
        _ => Response {
            parts: vec![AssistantPart::Unknown],
            stop: StopReason::Other,
        },
    }
}

/// Whether `item` is an Anthropic content block (no OpenAI Responses item
/// has these types).
fn anthropic_block(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("text" | "thinking" | "redacted_thinking" | "tool_use" | "server_tool_use")
    )
}

/// Tool-call arguments: a JSON string or a JSON value, as canonical JSON
/// when they parse, else kept verbatim.
pub fn arguments(raw: &Value) -> ToolArguments {
    let text = match raw {
        Value::String(text) => text.clone(),
        Value::Null => "{}".to_owned(),
        other => other.to_string(),
    };
    match CanonicalJson::canonicalize(&text) {
        Ok(json) => ToolArguments::Json(json),
        Err(_) => ToolArguments::Invalid(text),
    }
}

/// Visible reasoning, dropped when empty. Its signature is never stored.
pub fn visible(text: &str) -> Option<AssistantPart> {
    (!text.is_empty()).then(|| AssistantPart::Reasoning {
        text: text.to_owned(),
    })
}

/// Opaque reasoning, dropped when empty or scrubbed.
pub fn opaque(payload: &str) -> Option<AssistantPart> {
    (!payload.is_empty() && payload != BLOB_REMOVED).then_some(AssistantPart::ReasoningOpaque)
}

/// A text part, dropped when empty.
pub fn text(text: &str) -> Option<AssistantPart> {
    (!text.is_empty()).then(|| AssistantPart::Text {
        text: text.to_owned(),
    })
}

/// `ToolUse` when the parts call a tool, else `fallback`.
pub fn stop_for(parts: &[AssistantPart], fallback: StopReason) -> StopReason {
    if parts
        .iter()
        .any(|part| matches!(part, AssistantPart::ToolCall(_)))
    {
        StopReason::ToolUse
    } else {
        fallback
    }
}
