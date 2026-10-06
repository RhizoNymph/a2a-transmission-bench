//! Message parts, per role, mirroring crosstalk-spec's so that a part's
//! index is the same on both sides. Opaque provider material (signatures,
//! encrypted reasoning, media bytes) is not stored.

use serde::{Deserialize, Serialize};

use crate::json::CanonicalJson;

/// A system message's part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SystemPart {
    Text { text: String },
    Unknown,
}

/// A user message's part. Tool results are never user parts: they are
/// `tool` messages (the normalisation rule in the format doc).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserPart {
    Text { text: String },
    Media { kind: MediaKind },
    Unknown,
}

/// What a media part holds. Its bytes are not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Audio,
    Document,
    Other,
}

/// An assistant message's part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssistantPart {
    Text {
        text: String,
    },
    /// Reasoning the provider showed. A signature, if any, is dropped.
    Reasoning {
        text: String,
    },
    /// Reasoning the provider sent encrypted or redacted. It has no text.
    ReasoningOpaque,
    ToolCall(ToolCall),
    /// The result of a provider-executed tool call, returned in the response.
    ServerToolResult(ToolResult),
    Unknown,
}

/// A tool message's part: one tool result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolPart {
    ToolResult(ToolResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// The provider's call id. Kept to pair calls and results; never part text.
    pub call_id: String,
    pub name: String,
    pub arguments: ToolArguments,
    pub execution: ToolExecution,
}

/// Tool-call arguments: canonical JSON, or text that is not JSON, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolArguments {
    Json(CanonicalJson),
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecution {
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResult {
    pub call_id: String,
    pub content: Vec<ResultContent>,
    pub outcome: ToolOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultContent {
    Text { text: String },
    Media { kind: MediaKind },
    Unknown,
}

/// Whether a tool call succeeded, as the protocol flags it; `unknown` for a
/// protocol without such a flag, whatever the result's text says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    Success,
    Error,
    Unknown,
}
