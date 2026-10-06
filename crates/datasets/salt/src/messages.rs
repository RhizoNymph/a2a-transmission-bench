//! SALT's OpenAI-chat messages as bench messages.
//!
//! | SALT | Bench |
//! | --- | --- |
//! | `system` | `System([Text])` |
//! | `user` | `User([Text])` |
//! | `assistant` | `Assistant`: reasoning parts, then the text (when not empty), then tool calls |
//! | `tool` | `Tool([ToolResult])`, outcome `error` when the result is a JSON object with `"success": false`, else `success` |
//!
//! Assistant reasoning, in crosstalk-eval's order: each `thinking_blocks`
//! entry with thinking text is `reasoning` with that text (a signature, if
//! any, is dropped: signed visible reasoning is not opaque); one without
//! text but with `data` or a signature is `reasoning_opaque`. Without
//! thinking blocks, `reasoning_content` (or `reasoning`) is `reasoning`.
//! Then each `reasoning_items` entry's `encrypted_content` and each
//! `provider_specific_fields.thought_signatures` entry is
//! `reasoning_opaque`. Opaque parts have no text, so no location can name
//! them. Tool-call ids are kept whole as `call_id`, Gemini's embedded
//! base64 thought signatures included; a call id is never part text.

use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};
use serde_json::Value;

use crate::SaltError;
use crate::schema::RawMessage;

/// The text of a message's `content`: a string, `null`, or a list of parts
/// whose `text` members are joined with a line break.
pub fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// One SALT message as a bench body.
pub fn body(raw: &RawMessage) -> Result<Body, SaltError> {
    let text = content_text(&raw.content);
    match raw.role.as_str() {
        "system" => Ok(Body::System(vec![SystemPart::Text { text }])),
        "user" => Ok(Body::User(vec![UserPart::Text { text }])),
        "assistant" => Ok(Body::Assistant(assistant_parts(raw, text))),
        "tool" => {
            let outcome = match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(members))
                    if members.get("success") == Some(&Value::Bool(false)) =>
                {
                    ToolOutcome::Error
                }
                _ => ToolOutcome::Success,
            };
            Ok(Body::Tool(vec![ToolPart::ToolResult(ToolResult {
                call_id: raw.tool_call_id.clone().unwrap_or_default(),
                content: vec![ResultContent::Text { text }],
                outcome,
            })]))
        }
        other => Err(SaltError::UnknownRole(other.to_owned())),
    }
}

/// One SALT message as a bench message.
pub fn convert(raw: &RawMessage) -> Result<Message, SaltError> {
    Ok(Message::new(body(raw)?)?)
}

fn assistant_parts(raw: &RawMessage, text: String) -> Vec<AssistantPart> {
    let mut parts = Vec::new();
    match &raw.thinking_blocks {
        Some(blocks) if !blocks.is_empty() => {
            for block in blocks {
                let signed = block
                    .get("signature")
                    .and_then(Value::as_str)
                    .is_some_and(|signature| !signature.is_empty());
                match block.get("thinking").and_then(Value::as_str) {
                    Some(thinking) if !thinking.is_empty() => {
                        parts.push(AssistantPart::Reasoning {
                            text: thinking.to_owned(),
                        });
                    }
                    _ => {
                        // Redacted thinking: only the opaque payload, or
                        // nothing when there is none.
                        let data = block.get("data").and_then(Value::as_str).is_some();
                        if data || signed {
                            parts.push(AssistantPart::ReasoningOpaque);
                        }
                    }
                }
            }
        }
        _ => {
            let visible = raw
                .reasoning_content
                .as_ref()
                .or(raw.reasoning.as_ref())
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty());
            if let Some(visible) = visible {
                parts.push(AssistantPart::Reasoning {
                    text: visible.to_owned(),
                });
            }
        }
    }
    for item in raw.reasoning_items.iter().flatten() {
        if item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_some()
        {
            parts.push(AssistantPart::ReasoningOpaque);
        }
    }
    if let Some(Value::Array(signatures)) = raw
        .provider_specific_fields
        .as_ref()
        .and_then(|fields| fields.get("thought_signatures"))
    {
        parts.extend(
            signatures
                .iter()
                .filter(|signature| signature.is_string())
                .map(|_| AssistantPart::ReasoningOpaque),
        );
    }
    if !text.is_empty() {
        parts.push(AssistantPart::Text { text });
    }
    for call in raw.tool_calls.iter().flatten() {
        parts.push(AssistantPart::ToolCall(ToolCall {
            call_id: call.id.clone(),
            name: call.function.name.clone(),
            arguments: arguments(&call.function.arguments),
            execution: ToolExecution::Client,
        }));
    }
    parts
}

/// Arguments as canonical JSON when they parse, else kept verbatim; `null`
/// is empty invalid arguments; an object is canonicalised from its
/// serialisation.
pub fn arguments(raw: &Value) -> ToolArguments {
    match raw {
        Value::String(text) => match CanonicalJson::canonicalize(text) {
            Ok(json) => ToolArguments::Json(json),
            Err(_) => ToolArguments::Invalid(text.clone()),
        },
        Value::Null => ToolArguments::Invalid(String::new()),
        other => {
            let text = other.to_string();
            match CanonicalJson::canonicalize(&text) {
                Ok(json) => ToolArguments::Json(json),
                Err(_) => ToolArguments::Invalid(text),
            }
        }
    }
}

/// The string member `name` of a tool call's arguments.
pub fn argument(raw: &Value, name: &str) -> Option<String> {
    let parsed = match raw {
        Value::String(text) => serde_json::from_str::<Value>(text).ok()?,
        other => other.clone(),
    };
    parsed.get(name).and_then(Value::as_str).map(str::to_owned)
}
