//! Provider-neutral messages, stored once per world and named by the digest
//! of their canonical JSON.

mod part;
mod text;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ids::{Digest, MessageId};
use crate::json::Json;

pub use part::{
    AssistantPart, ResultContent, SystemPart, ToolArguments, ToolCall, ToolExecution, ToolOutcome,
    ToolPart, ToolResult, UserPart,
};
pub use text::{NoPartText, TOOL_RESULT_SEPARATOR};

/// The BLAKE3 derive-key context of a [`MessageId`].
pub const MESSAGE_ID_CONTEXT: &str = "a2a-bench/1 message";

/// A message body: its role and its parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "role",
    content = "parts",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Body {
    System(Vec<SystemPart>),
    User(Vec<UserPart>),
    Assistant(Vec<AssistantPart>),
    /// One or more tool results.
    Tool(Vec<ToolPart>),
}

/// Why a body is not a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidMessage {
    #[error("a tool message holds no result")]
    EmptyTool,
    #[error("a message has more than {max} parts", max = u16::MAX)]
    TooManyParts,
    #[error("the body does not serialize: {0}")]
    Encode(String),
    #[error("message id {stored} is not its body's ({computed})")]
    IdMismatch {
        stored: MessageId,
        computed: MessageId,
    },
}

/// A message: a body and the id its canonical JSON hashes to. Built only by
/// [`Message::new`], and checked on deserialization, so the id is always
/// the body's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    id: MessageId,
    body: Body,
}

impl Message {
    pub fn new(body: Body) -> Result<Self, InvalidMessage> {
        let parts = match &body {
            Body::System(parts) => parts.len(),
            Body::User(parts) => parts.len(),
            Body::Assistant(parts) => parts.len(),
            Body::Tool(parts) => parts.len(),
        };
        if matches!(&body, Body::Tool(parts) if parts.is_empty()) {
            return Err(InvalidMessage::EmptyTool);
        }
        if parts > usize::from(u16::MAX) {
            return Err(InvalidMessage::TooManyParts);
        }
        let id = id_of(&body)?;
        Ok(Self { id, body })
    }

    pub fn id(&self) -> MessageId {
        self.id
    }

    pub fn body(&self) -> &Body {
        &self.body
    }
}

/// The id of `body`: the keyed BLAKE3 of its canonical JSON
/// (`{"parts":[…],"role":"…"}` with members sorted).
pub fn id_of(body: &Body) -> Result<MessageId, InvalidMessage> {
    let text =
        serde_json::to_string(body).map_err(|error| InvalidMessage::Encode(error.to_string()))?;
    let canonical = Json::parse(&text)
        .map_err(|error| InvalidMessage::Encode(error.to_string()))?
        .canonical();
    Ok(MessageId::from_digest(Digest::keyed(
        MESSAGE_ID_CONTEXT,
        canonical.as_str().as_bytes(),
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMessage {
    id: MessageId,
    body: Body,
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawMessage::deserialize(deserializer)?;
        let message = Self::new(raw.body).map_err(serde::de::Error::custom)?;
        if message.id != raw.id {
            return Err(serde::de::Error::custom(InvalidMessage::IdMismatch {
                stored: raw.id,
                computed: message.id,
            }));
        }
        Ok(message)
    }
}
