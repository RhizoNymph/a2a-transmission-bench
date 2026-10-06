//! Each agent's view of one simulation.
//!
//! A simulation records one conversation between the agent (`assistant`)
//! and the user simulator (`user`), with each side's tool calls and results
//! (`requestor`). Each side's model sees it differently:
//!
//! | record | agent sees | user simulator sees |
//! | --- | --- | --- |
//! | `assistant` | `Assistant` (text, tool calls) | `User` (its text only; nothing for a bare tool call) |
//! | `user` | `User` (its text only; nothing for a bare tool call) | `Assistant` (text, tool calls) |
//! | `tool`, requestor `assistant` | `Tool` | nothing |
//! | `tool`, requestor `user` | nothing | `Tool` |
//!
//! Each view starts with that side's system prompt.

use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{
    AssistantPart, Body, Message, ResultContent, SystemPart, ToolArguments, ToolCall,
    ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};

use crate::Tau2Error;
use crate::schema::{RawCall, RawMessage};

/// Whose view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Agent,
    User,
}

impl Side {
    /// The record role this side's model writes.
    pub fn role(self) -> &'static str {
        match self {
            Self::Agent => "assistant",
            Self::User => "user",
        }
    }

    pub fn peer(self) -> Self {
        match self {
            Self::Agent => Self::User,
            Self::User => Self::Agent,
        }
    }
}

/// One message of a view and the record it came from.
#[derive(Debug, Clone)]
pub struct Entry {
    pub raw: usize,
    pub message: Message,
}

/// A side's system prompt and the records it sees, in order.
#[derive(Debug, Clone)]
pub struct View {
    pub system: Message,
    pub entries: Vec<Entry>,
}

impl View {
    /// The position of record `raw` in the view.
    pub fn position(&self, raw: usize) -> Option<usize> {
        self.entries.iter().position(|entry| entry.raw == raw)
    }

    pub fn entry(&self, raw: usize) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.raw == raw)
    }

    /// The request of the call whose response is the entry at `position`:
    /// the system prompt and every earlier entry.
    pub fn request(&self, position: usize) -> Vec<Message> {
        std::iter::once(self.system.clone())
            .chain(
                self.entries
                    .iter()
                    .take(position)
                    .map(|entry| entry.message.clone()),
            )
            .collect()
    }
}

/// `side`'s view of `messages`, after `system`.
pub fn view(side: Side, messages: &[RawMessage], system: String) -> Result<View, Tau2Error> {
    let mut entries = Vec::with_capacity(messages.len());
    for (raw, message) in messages.iter().enumerate() {
        let body = match message.role.as_str() {
            "assistant" | "user" if message.role == side.role() => Some(own(message)),
            "assistant" | "user" => message.text().map(|text| {
                Body::User(vec![UserPart::Text {
                    text: text.to_owned(),
                }])
            }),
            "tool" => {
                let requestor = message.requestor.as_deref().unwrap_or("assistant");
                (requestor == side.role()).then(|| tool(message, raw))
            }
            other => return Err(Tau2Error::UnknownRole(other.to_owned())),
        };
        if let Some(body) = body {
            entries.push(Entry {
                raw,
                message: Message::new(body)?,
            });
        }
    }
    Ok(View {
        system: Message::new(Body::System(vec![SystemPart::Text { text: system }]))?,
        entries,
    })
}

/// A message the side's own model wrote: its text, then its tool calls.
fn own(message: &RawMessage) -> Body {
    let mut parts = Vec::new();
    if let Some(text) = message.text() {
        parts.push(AssistantPart::Text {
            text: text.to_owned(),
        });
    }
    parts.extend(
        message
            .calls()
            .iter()
            .map(|call| AssistantPart::ToolCall(tool_call(call))),
    );
    Body::Assistant(parts)
}

fn tool_call(call: &RawCall) -> ToolCall {
    let text = call.arguments.to_string();
    let arguments = match CanonicalJson::canonicalize(&text) {
        Ok(json) => ToolArguments::Json(json),
        Err(_) => ToolArguments::Invalid(text),
    };
    ToolCall {
        call_id: call.id.clone(),
        name: call.name.clone(),
        arguments,
        execution: ToolExecution::Client,
    }
}

fn tool(message: &RawMessage, raw: usize) -> Body {
    Body::Tool(vec![ToolPart::ToolResult(ToolResult {
        call_id: message
            .id
            .clone()
            .unwrap_or_else(|| format!("tau2-result-{raw}")),
        content: vec![ResultContent::Text {
            text: message.content.clone().unwrap_or_default(),
        }],
        outcome: if message.error == Some(true) {
            ToolOutcome::Error
        } else {
            ToolOutcome::Success
        },
    })])
}
