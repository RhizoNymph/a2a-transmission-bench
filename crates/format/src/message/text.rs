//! The text of one message part: what a location's byte range indexes.
//!
//! | Part | Text |
//! | --- | --- |
//! | `text` (any role) | the text |
//! | `reasoning` | the text |
//! | `tool_call` | its arguments: the canonical JSON, or the invalid text verbatim |
//! | `tool_result`, `server_tool_result` | its `text` contents in order, joined with [`TOOL_RESULT_SEPARATOR`] |
//! | `reasoning_opaque`, `media`, `unknown`, a result without text | none |
//!
//! This is crosstalk-spec's `Message::part_text` table; the parity tests
//! check the two agree on every part of every golden export. Changing it is
//! a format major.

use std::borrow::Cow;

use super::part::{
    AssistantPart, ResultContent, SystemPart, ToolArguments, ToolPart, ToolResult, UserPart,
};
use super::{Body, Message};

/// Joins the text contents of one tool result into its part text.
pub const TOOL_RESULT_SEPARATOR: &str = "\n";

/// Why a part has no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoPartText {
    #[error("no part {index}: the message has {parts}")]
    NoSuchPart { index: u16, parts: usize },
    #[error("part {index} has no text")]
    NotText { index: u16 },
}

fn result_text(result: &ToolResult) -> Option<Cow<'_, str>> {
    let texts: Vec<&str> = result
        .content
        .iter()
        .filter_map(|content| match content {
            ResultContent::Text { text } => Some(text.as_str()),
            ResultContent::Media { .. } | ResultContent::Unknown => None,
        })
        .collect();
    match texts.as_slice() {
        [] => None,
        [one] => Some(Cow::Borrowed(one)),
        many => Some(Cow::Owned(many.join(TOOL_RESULT_SEPARATOR))),
    }
}

fn arguments_text(arguments: &ToolArguments) -> &str {
    match arguments {
        ToolArguments::Json(json) => json.as_str(),
        ToolArguments::Invalid(text) => text,
    }
}

impl Message {
    /// How many parts the message holds: the range of a part index.
    pub fn part_count(&self) -> usize {
        match &self.body {
            Body::System(parts) => parts.len(),
            Body::User(parts) => parts.len(),
            Body::Assistant(parts) => parts.len(),
            Body::Tool(parts) => parts.len(),
        }
    }

    /// The text of part `index`.
    pub fn part_text(&self, index: u16) -> Result<Cow<'_, str>, NoPartText> {
        let at = usize::from(index);
        let missing = NoPartText::NoSuchPart {
            index,
            parts: self.part_count(),
        };
        let text = match &self.body {
            Body::System(parts) => match parts.get(at).ok_or(missing)? {
                SystemPart::Text { text } => Some(Cow::Borrowed(text.as_str())),
                SystemPart::Unknown => None,
            },
            Body::User(parts) => match parts.get(at).ok_or(missing)? {
                UserPart::Text { text } => Some(Cow::Borrowed(text.as_str())),
                UserPart::Media { .. } | UserPart::Unknown => None,
            },
            Body::Assistant(parts) => match parts.get(at).ok_or(missing)? {
                AssistantPart::Text { text } | AssistantPart::Reasoning { text } => {
                    Some(Cow::Borrowed(text.as_str()))
                }
                AssistantPart::ToolCall(call) => {
                    Some(Cow::Borrowed(arguments_text(&call.arguments)))
                }
                AssistantPart::ServerToolResult(result) => result_text(result),
                AssistantPart::ReasoningOpaque | AssistantPart::Unknown => None,
            },
            Body::Tool(parts) => match parts.get(at).ok_or(missing)? {
                ToolPart::ToolResult(result) => result_text(result),
            },
        };
        text.ok_or(NoPartText::NotText { index })
    }
}
