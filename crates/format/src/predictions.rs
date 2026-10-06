//! What a detector writes: its agents as sets of exchanges, and its
//! transmissions with their evidence.

use serde::{Deserialize, Serialize};

use crate::ids::{DetectorAgent, ExchangeId, TransmissionRef};
use crate::labels::{CarrierKind, Codec, DelegationDirection, MatchClass, RouteKind};
use crate::location::Location;
use crate::resource::Resource;

/// How a content match was made, with the codecs a decoded one undid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatchKind {
    Exact,
    Normalized,
    Decoded { codecs: Vec<Codec> },
    Semantic,
}

impl MatchKind {
    pub const fn class(&self) -> MatchClass {
        match self {
            Self::Exact => MatchClass::Exact,
            Self::Normalized => MatchClass::Normalized,
            Self::Decoded { .. } => MatchClass::Decoded,
            Self::Semantic => MatchClass::Semantic,
        }
    }
}

/// A detector's agent and the exchanges it attributes to it. Agent names
/// are canonical: resolved through any merge before writing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    pub agent: DetectorAgent,
    pub exchanges: Vec<ExchangeId>,
}

/// A detector agent the detector names but cannot tie to exchanges. Its
/// predictions are reported as from an unknown agent, never scored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unattributed {
    pub agent: DetectorAgent,
}

/// One content match: sender's text found in the reader's exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentEvidence {
    pub from: DetectorAgent,
    pub to: DetectorAgent,
    pub reader_exchange: ExchangeId,
    pub read_at: Location,
    /// Where the sender's text is, when the detector knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_at: Option<Location>,
    #[serde(rename = "match")]
    pub kind: MatchKind,
    pub carrier: CarrierKind,
    pub route: PredictedRoute,
}

/// How a detector says content travelled. A channel names every resource
/// the detector's channel holds; a channel label aligns when one of them is
/// the label's resource, after canonicalisation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PredictedRoute {
    Channel { resources: Vec<Resource> },
    Delegation { direction: DelegationDirection },
    Direct,
    Unobserved,
}

impl PredictedRoute {
    pub const fn kind(&self) -> RouteKind {
        match self {
            Self::Channel { .. } => RouteKind::Channel,
            Self::Delegation { .. } => RouteKind::Delegation,
            Self::Direct => RouteKind::Direct,
            Self::Unobserved => RouteKind::Unobserved,
        }
    }
}

/// One co-access: the sender wrote a resource the reader then read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoAccess {
    pub from: DetectorAgent,
    pub to: DetectorAgent,
    pub write_exchange: ExchangeId,
    /// The whole write call.
    pub write_at: Location,
    pub reader_exchange: ExchangeId,
    /// The whole tool result the read returned.
    pub read_at: Location,
    pub resource: Resource,
}

/// A transmission's state when the detector wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Confirmed,
    Classified,
    Aggregated,
    Suspected,
    Discarded,
    Detected,
    AwaitingContent,
}

impl State {
    /// Content evidence decides the transmission.
    pub const fn is_content(self) -> bool {
        matches!(self, Self::Confirmed | Self::Classified | Self::Aggregated)
    }

    /// Only co-access evidence exists.
    pub const fn is_access(self) -> bool {
        matches!(self, Self::Suspected | Self::Discarded)
    }
}

/// What a transmission's quality row is keyed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Quality {
    /// By its strongest match's class and carrier.
    Content {
        class: MatchClass,
        carrier: CarrierKind,
    },
    Suspected,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTransmission {
    #[error("a {state:?} transmission needs content evidence and a content quality")]
    MissingContent { state: State },
    #[error("a {state:?} transmission carries no content evidence")]
    UnexpectedContent { state: State },
    #[error("a {state:?} transmission needs co-access evidence")]
    MissingAccess { state: State },
    #[error("a {state:?} transmission carries no evidence")]
    UnexpectedEvidence { state: State },
    #[error("the quality {quality:?} does not fit state {state:?}")]
    Quality {
        state: State,
        quality: Option<Quality>,
    },
    #[error("the quality's class and carrier are not any match's")]
    QualityNotAMatch,
    #[error("a decoded match names no codec")]
    NoCodec,
}

/// The fields of a transmission row, before its checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransmissionFields {
    pub id: TransmissionRef,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<Quality>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<ContentEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub co_access: Vec<CoAccess>,
}

/// A detector's transmission. Checked: content states carry matches and a
/// content quality that is one of theirs; access states carry co-access
/// records, no matches, and the quality of their state; detected and
/// awaiting-content ones carry nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TransmissionFields", into = "TransmissionFields")]
pub struct Transmission(TransmissionFields);

impl Transmission {
    pub fn new(fields: TransmissionFields) -> Result<Self, InvalidTransmission> {
        let state = fields.state;
        if fields
            .matches
            .iter()
            .any(|m| matches!(&m.kind, MatchKind::Decoded { codecs } if codecs.is_empty()))
        {
            return Err(InvalidTransmission::NoCodec);
        }
        if state.is_content() {
            let Some(Quality::Content { class, carrier }) = fields.quality else {
                return Err(InvalidTransmission::MissingContent { state });
            };
            if fields.matches.is_empty() {
                return Err(InvalidTransmission::MissingContent { state });
            }
            if !fields
                .matches
                .iter()
                .any(|m| m.kind.class() == class && m.carrier == carrier)
            {
                return Err(InvalidTransmission::QualityNotAMatch);
            }
        } else if state.is_access() {
            if !fields.matches.is_empty() {
                return Err(InvalidTransmission::UnexpectedContent { state });
            }
            if fields.co_access.is_empty() {
                return Err(InvalidTransmission::MissingAccess { state });
            }
            let expected = match state {
                State::Suspected => Quality::Suspected,
                _ => Quality::Discarded,
            };
            if fields.quality != Some(expected) {
                return Err(InvalidTransmission::Quality {
                    state,
                    quality: fields.quality,
                });
            }
        } else if !fields.matches.is_empty()
            || !fields.co_access.is_empty()
            || fields.quality.is_some()
        {
            return Err(InvalidTransmission::UnexpectedEvidence { state });
        }
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &TransmissionFields {
        &self.0
    }
}

impl TryFrom<TransmissionFields> for Transmission {
    type Error = InvalidTransmission;

    fn try_from(fields: TransmissionFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<Transmission> for TransmissionFields {
    fn from(transmission: Transmission) -> Self {
        transmission.0
    }
}

/// How the detector fared on one world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorldStatus {
    Scored,
    /// The detector ran but has nothing that detects (crosstalk's bare
    /// pipeline): counted unscored, never scored as zero.
    NoConsumers {
        ingested: u64,
    },
    Failed {
        reason: String,
    },
}

/// One row of a world's predictions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Prediction {
    Attribution(Attribution),
    Unattributed(Unattributed),
    Transmission(Transmission),
}
