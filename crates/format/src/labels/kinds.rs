//! Label and prediction dimensions: tiers, carriers, match classes, codecs,
//! routes and reasons. Names and meanings are crosstalk-eval's.

use serde::{Deserialize, Serialize};

use crate::resource::Resource;

/// How a label was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// The dataset's construction implies it.
    Construction,
    /// The dataset's structure records it.
    Structural,
    /// A converter rule infers it.
    Heuristic,
    /// A judge decided it.
    Judged,
    /// No detector is required to find it; reported apart, never in overall.
    OutOfReach,
    /// The sender pasted its own tool output; reported apart, never in overall.
    Forwarding,
}

impl Tier {
    /// Whether rows of this tier count toward overall and access-only recall.
    pub const fn in_overall(self) -> bool {
        !matches!(self, Self::OutOfReach | Self::Forwarding)
    }
}

/// Where the content sits in the reader's exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierKind {
    ToolResult,
    UserTurn,
    SystemPrompt,
    ReaderOutput,
}

/// How a content match was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchClass {
    Exact,
    Normalized,
    Decoded,
    Semantic,
}

/// A decoding step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Codec {
    Base64,
    Hex,
    UrlEncoding,
    /// NFKC normalization, confusable folding, zero-width character removal.
    UnicodeNormalization,
    /// One level of JSON string unescaping.
    JsonString,
    /// One level of YAML scalar unescaping.
    YamlString,
}

/// The weakest match a detector should need to find a label's content.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatchNeed {
    Exact,
    Normalized,
    Decoded {
        codecs: Vec<Codec>,
    },
    Semantic,
    /// Encoded beyond any codec (rotN, binary8, substitution, two string
    /// levels), named by `codec`. Out of reach.
    Undecodable {
        codec: String,
    },
    /// Read from a medium its sender never wrote, so nothing observable
    /// links the two. `arrival` is the class the text would match by, which
    /// picks its row. Out of reach.
    Unobserved {
        reason: String,
        arrival: MatchClass,
    },
}

/// The `Undecodable` codec name of text escaped two string levels deep.
pub const TWO_STRING_LEVELS: &str = "json_string+json_string";

/// The `Unobserved` reason of content read from a medium its sender never
/// wrote (INV-963 in crosstalk).
pub const SENDER_MEDIUM_UNOBSERVED: &str = "sender medium unobserved (INV-963)";

/// Whether writing `text` as a JSON string's contents changes it: it holds a
/// quote, a backslash or a control character.
pub fn json_escapes(text: &str) -> bool {
    text.chars()
        .any(|ch| matches!(ch, '"' | '\\' | '\u{0}'..='\u{1f}'))
}

impl MatchNeed {
    /// Text serialised once as a JSON string.
    pub fn json_string() -> Self {
        Self::Decoded {
            codecs: vec![Codec::JsonString],
        }
    }

    /// Text serialised once as a YAML scalar.
    pub fn yaml_string() -> Self {
        Self::Decoded {
            codecs: vec![Codec::YamlString],
        }
    }

    /// Text a writer put inside a JSON string (tool-call arguments) and a
    /// reader received raw: decoded once when escaping changes it, exact
    /// otherwise.
    pub fn through_json_string(text: &str) -> Self {
        if json_escapes(text) {
            Self::json_string()
        } else {
            Self::Exact
        }
    }

    /// Text that arrives only after two string levels are undone: out of reach.
    pub fn two_string_levels() -> Self {
        Self::Undecodable {
            codec: TWO_STRING_LEVELS.to_owned(),
        }
    }

    /// Content read from a medium its sender never wrote, arriving as
    /// `arrival` would: out of reach.
    pub fn sender_medium_unobserved(arrival: MatchClass) -> Self {
        Self::Unobserved {
            reason: SENDER_MEDIUM_UNOBSERVED.to_owned(),
            arrival,
        }
    }

    /// The tier a label with this need gets: out of reach when the need is,
    /// `in_reach` otherwise.
    pub fn tier(&self, in_reach: Tier) -> Tier {
        if self.out_of_reach() {
            Tier::OutOfReach
        } else {
            in_reach
        }
    }

    /// Whether no detector is required to find a label with this need.
    pub const fn out_of_reach(&self) -> bool {
        matches!(self, Self::Undecodable { .. } | Self::Unobserved { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationDirection {
    /// The parent's task prompt became the child's first user turn.
    ParentToChild,
    /// The child's final message came back as the parent's tool result.
    ChildToParent,
}

/// How content travelled from sender to reader.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Route {
    Channel { resource: Resource },
    Delegation { direction: DelegationDirection },
    Direct,
    Unobserved,
}

impl Route {
    pub const fn kind(&self) -> RouteKind {
        match self {
            Self::Channel { .. } => RouteKind::Channel,
            Self::Delegation { .. } => RouteKind::Delegation,
            Self::Direct => RouteKind::Direct,
            Self::Unobserved => RouteKind::Unobserved,
        }
    }
}

/// A route without its resource or direction, for report rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    Channel,
    Delegation,
    Direct,
    Unobserved,
}

/// Why a negative control must not yield a transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NegativeReason {
    RejectedSend,
    SharedSource,
    Boilerplate,
    NoSenderExchange,
    SelfRead,
    Reread,
    Miss,
}

/// Why a place is exempt from judging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExemptionReason {
    /// Content arrived from another agent, but the dataset does not know which.
    UnknownSender,
}

/// What groups the agents of a cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterKind {
    /// They share a credential (demo-swarm key groups); not one agent.
    KeyGroup,
    /// They are one agent.
    Identity,
}
