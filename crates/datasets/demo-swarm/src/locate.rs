//! Finding the truth's tool uses in captured exchanges.
//!
//! - **Reader.** A request's tool result for a call id: the `tool` message
//!   holding it, its position among that message's results (the part
//!   index of a location), and its text (`Message::part_text`, the
//!   result's text contents joined; one text content for a page read).
//! - **Writer.** A response's `PUT` tool call for a call id, and the page
//!   body inside its JSON arguments (`input.body`).

use std::collections::BTreeMap;

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::ids::MessageId;
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{
    AssistantPart, Body, Message, NoPartText, ToolArguments, ToolPart,
};

use crate::schema::HexDigest;

/// The capture's messages by id.
pub type MessageIndex = BTreeMap<MessageId, Message>;

/// A tool result found in a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundResult {
    /// The whole result's text, in the exchange it was found in.
    pub at: Location,
    pub text: String,
}

impl FoundResult {
    pub fn digest(&self) -> HexDigest {
        HexDigest::blake3_of(self.text.as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocateError {
    #[error("the exchange names message {0}, which the capture does not hold")]
    MissingMessage(MessageId),
    #[error("the tool result has no text: {0}")]
    NoText(NoPartText),
    #[error("the tool result's text is empty")]
    EmptyText,
    #[error("the tool result's text is longer than u32::MAX bytes")]
    TooLong,
    #[error("a tool message holds more than {} results", u16::MAX)]
    TooManyParts,
}

fn message(messages: &MessageIndex, id: MessageId) -> Result<&Message, LocateError> {
    messages.get(&id).ok_or(LocateError::MissingMessage(id))
}

/// The tool result for `call_id` in `exchange`'s request; the last one if
/// the request repeats it.
pub fn tool_result(
    exchange: &Exchange,
    call_id: &str,
    messages: &MessageIndex,
) -> Result<Option<FoundResult>, LocateError> {
    let mut found = None;
    for id in &exchange.request.messages {
        let held = message(messages, *id)?;
        let Body::Tool(results) = held.body() else {
            continue;
        };
        for (index, ToolPart::ToolResult(result)) in results.iter().enumerate() {
            if result.call_id != call_id {
                continue;
            }
            let part = u16::try_from(index).map_err(|_| LocateError::TooManyParts)?;
            let text = held
                .part_text(part)
                .map_err(LocateError::NoText)?
                .into_owned();
            let end = u32::try_from(text.len()).map_err(|_| LocateError::TooLong)?;
            let range = ByteRange::new(0, end).map_err(|_| LocateError::EmptyText)?;
            let at = Location {
                exchange: exchange.id,
                message: *id,
                part,
                range,
            };
            found = Some(FoundResult { at, text });
        }
    }
    Ok(found)
}

/// A write tool call found in a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoundCall {
    /// A `PUT` whose arguments carry a string `body`.
    Put { url: String, body: String },
    /// The call exists but is not a `PUT` with a string body.
    NotAPut,
}

/// The tool call `call_id` in `exchange`'s response (the partial response
/// of a failed exchange, when the capture holds one), if any: the first
/// one, response messages in order.
pub fn write_call(
    exchange: &Exchange,
    call_id: &str,
    messages: &MessageIndex,
) -> Result<Option<FoundCall>, LocateError> {
    for id in &exchange.response.messages {
        let Body::Assistant(parts) = message(messages, *id)?.body() else {
            continue;
        };
        for part in parts {
            let AssistantPart::ToolCall(call) = part else {
                continue;
            };
            if call.call_id != call_id {
                continue;
            }
            let ToolArguments::Json(json) = &call.arguments else {
                return Ok(Some(FoundCall::NotAPut));
            };
            return Ok(Some(put_of(json.as_str())));
        }
    }
    Ok(None)
}

fn put_of(arguments: &str) -> FoundCall {
    let Ok(serde_json::Value::Object(members)) = serde_json::from_str(arguments) else {
        return FoundCall::NotAPut;
    };
    let text = |name: &str| match members.get(name) {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        _ => None,
    };
    match (text("method"), text("url"), text("body")) {
        (Some(method), Some(url), Some(body)) if method.eq_ignore_ascii_case("PUT") => {
            FoundCall::Put { url, body }
        }
        _ => FoundCall::NotAPut,
    }
}
