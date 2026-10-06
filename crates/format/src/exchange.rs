//! Worlds and exchanges: the inputs a detector reads, beside messages.

use serde::{Deserialize, Serialize};

use crate::ids::{AgentKey, ExchangeId, MessageId, SourceRef, WorldKey};
use crate::json::CanonicalJson;
use crate::time::Timestamp;

/// A world's declaration: the agents that only talk to each other. The
/// agent list is public; which agent made which exchange is truth
/// (`labels.jsonl`, `exchange_agent` rows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldDecl {
    pub key: WorldKey,
    pub agents: Vec<AgentDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDecl {
    pub key: AgentKey,
    pub driven: Driven,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Whether an agent is a model (it makes exchanges) or scripted (it never does).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Driven {
    Model,
    Scripted,
}

/// How closely an exchange matches what was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// The request and response as sent.
    Exact,
    /// Rebuilt from a log that holds every message.
    Reconstructed,
    /// Rebuilt with gaps filled by the converter.
    Synthetic,
}

/// What a proxy in front of the model observes about the caller. Not the
/// agent: several agents may share a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    /// An opaque, stable fingerprint of the caller's credential.
    pub credential: String,
    /// A session or conversation header, when the dataset has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The request's 0-based ordinal within `session`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// A tool the request offered the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDecl {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The input schema, as canonical JSON text (exact numbers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<CanonicalJson>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// One ordered list, system messages inline in their original position.
    pub messages: Vec<MessageId>,
    /// Absent when the dataset does not record the tools offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDecl>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub messages: Vec<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
    /// Why the call failed, when it did; `messages` may then be empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One model call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub id: ExchangeId,
    pub at_us: Timestamp,
    pub client: Client,
    pub request: Request,
    pub response: Response,
    pub fidelity: Fidelity,
    pub source: SourceRef,
}

/// Which side of an exchange a message is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Request,
    Response,
}

impl Exchange {
    /// The side of this exchange that holds `message`, request first.
    pub fn side_of(&self, message: MessageId) -> Option<Side> {
        if self.request.messages.contains(&message) {
            Some(Side::Request)
        } else if self.response.messages.contains(&message) {
            Some(Side::Response)
        } else {
            None
        }
    }
}
