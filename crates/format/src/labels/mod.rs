//! Truth: what a detector should and should not report. Rows are built
//! only through checked constructors, and deserialized through the same
//! checks. Checks that need the world's messages (content text equals the
//! text at its location) are in [`crate::check`].

mod kinds;

use serde::{Deserialize, Serialize};

use crate::ids::{AgentKey, ExchangeId, LabelId, SourceRef};
use crate::location::Location;

pub use kinds::{
    CarrierKind, ClusterKind, Codec, DelegationDirection, ExemptionReason, MatchClass, MatchNeed,
    NegativeReason, Route, RouteKind, Tier,
};

/// Why a label row is invalid on its own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidLabel {
    #[error("{0} is both sender and reader")]
    SelfTransmission(AgentKey),
    #[error("content text is {text} bytes, its location {location}")]
    ContentLength { text: usize, location: u32 },
    #[error("a location is in exchange {location}, the row's reader exchange is {reader}")]
    ElsewhereThanReader {
        location: ExchangeId,
        reader: ExchangeId,
    },
    #[error("a negative control needs a reader exchange, a location or an origin")]
    Unbounded,
    #[error("a cluster needs at least two distinct agents")]
    SmallCluster,
    #[error("the match need and the tier disagree on whether the label is out of reach")]
    Reach,
    #[error("an access-only label needs a channel route")]
    AccessOffChannel,
}

/// A labelled content and where it sits in the reader's exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedContent {
    pub text: String,
    pub at: Location,
}

/// The fields of a transmission label, before its checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransmissionFields {
    pub id: LabelId,
    pub from: AgentKey,
    pub to: AgentKey,
    /// The sender's exchange whose output holds the content, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_exchange: Option<ExchangeId>,
    /// Where the content first arrived, not a later exchange still carrying it.
    pub reader_exchange: ExchangeId,
    pub route: Route,
    pub carrier: CarrierKind,
    pub content: ExpectedContent,
    pub needs: MatchNeed,
    pub tier: Tier,
    pub source: SourceRef,
}

fn check_reader(location: &Location, reader: ExchangeId) -> Result<(), InvalidLabel> {
    if location.exchange == reader {
        Ok(())
    } else {
        Err(InvalidLabel::ElsewhereThanReader {
            location: location.exchange,
            reader,
        })
    }
}

/// A transmission a detector should report with content evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TransmissionFields", into = "TransmissionFields")]
pub struct ExpectedTransmission(TransmissionFields);

impl ExpectedTransmission {
    /// Sender and reader differ; the content is as long as its location and
    /// sits in the reader exchange; the need is out of reach exactly when
    /// the tier is.
    pub fn new(fields: TransmissionFields) -> Result<Self, InvalidLabel> {
        if fields.from == fields.to {
            return Err(InvalidLabel::SelfTransmission(fields.from));
        }
        let text = fields.content.text.len();
        let location = fields.content.at.range.len();
        if u32::try_from(text).ok() != Some(location) {
            return Err(InvalidLabel::ContentLength { text, location });
        }
        check_reader(&fields.content.at, fields.reader_exchange)?;
        if fields.needs.out_of_reach() != (fields.tier == Tier::OutOfReach) {
            return Err(InvalidLabel::Reach);
        }
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &TransmissionFields {
        &self.0
    }
}

impl TryFrom<TransmissionFields> for ExpectedTransmission {
    type Error = InvalidLabel;

    fn try_from(fields: TransmissionFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<ExpectedTransmission> for TransmissionFields {
    fn from(label: ExpectedTransmission) -> Self {
        label.0
    }
}

/// A transmission only access evidence can find (no content links write and
/// read): a valid [`ExpectedTransmission`] on a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TransmissionFields", into = "TransmissionFields")]
pub struct ExpectedAccess(ExpectedTransmission);

impl ExpectedAccess {
    pub fn new(fields: TransmissionFields) -> Result<Self, InvalidLabel> {
        if !matches!(fields.route, Route::Channel { .. }) {
            return Err(InvalidLabel::AccessOffChannel);
        }
        ExpectedTransmission::new(fields).map(Self)
    }

    pub fn transmission(&self) -> &ExpectedTransmission {
        &self.0
    }

    pub fn fields(&self) -> &TransmissionFields {
        self.0.fields()
    }
}

impl TryFrom<TransmissionFields> for ExpectedAccess {
    type Error = InvalidLabel;

    fn try_from(fields: TransmissionFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<ExpectedAccess> for TransmissionFields {
    fn from(label: ExpectedAccess) -> Self {
        label.0.0
    }
}

/// The fields of a negative control, before its checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlFields {
    pub id: LabelId,
    pub from: AgentKey,
    pub to: AgentKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_exchange: Option<ExchangeId>,
    /// Where in the reader's exchange the trap sits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Location>,
    /// The sender-side text the trap is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Location>,
    /// The text concerned, for reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub reason: NegativeReason,
    pub tier: Tier,
    pub source: SourceRef,
}

/// A place where a detector must not report a transmission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ControlFields", into = "ControlFields")]
pub struct NegativeControl(ControlFields);

impl NegativeControl {
    /// Sender and reader differ (except for `self_read`); bounded by a
    /// reader exchange, a location or an origin; a location sits in the
    /// reader exchange when both are given.
    pub fn new(fields: ControlFields) -> Result<Self, InvalidLabel> {
        if fields.from == fields.to && fields.reason != NegativeReason::SelfRead {
            return Err(InvalidLabel::SelfTransmission(fields.from));
        }
        if fields.reader_exchange.is_none() && fields.at.is_none() && fields.origin.is_none() {
            return Err(InvalidLabel::Unbounded);
        }
        if let (Some(at), Some(reader)) = (&fields.at, fields.reader_exchange) {
            check_reader(at, reader)?;
        }
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &ControlFields {
        &self.0
    }
}

impl TryFrom<ControlFields> for NegativeControl {
    type Error = InvalidLabel;

    fn try_from(fields: ControlFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<NegativeControl> for ControlFields {
    fn from(control: NegativeControl) -> Self {
        control.0
    }
}

/// The fields of an exemption, before its checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExemptionFields {
    pub id: LabelId,
    pub to: AgentKey,
    pub reader_exchange: ExchangeId,
    pub at: Location,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub reason: ExemptionReason,
    pub tier: Tier,
    pub source: SourceRef,
}

/// One place in one reader exchange where predictions are unjudged. Always
/// bounded to that exchange and location, never the whole world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ExemptionFields", into = "ExemptionFields")]
pub struct Exemption(ExemptionFields);

impl Exemption {
    pub fn new(fields: ExemptionFields) -> Result<Self, InvalidLabel> {
        check_reader(&fields.at, fields.reader_exchange)?;
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &ExemptionFields {
        &self.0
    }
}

impl TryFrom<ExemptionFields> for Exemption {
    type Error = InvalidLabel;

    fn try_from(fields: ExemptionFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<Exemption> for ExemptionFields {
    fn from(exemption: Exemption) -> Self {
        exemption.0
    }
}

/// The fields of an agent cluster, before its checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterFields {
    pub id: LabelId,
    pub agents: Vec<AgentKey>,
    pub kind: ClusterKind,
    pub tier: Tier,
    pub source: SourceRef,
}

/// Agents grouped for identity tests: at least two distinct agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ClusterFields", into = "ClusterFields")]
pub struct AgentCluster(ClusterFields);

impl AgentCluster {
    pub fn new(fields: ClusterFields) -> Result<Self, InvalidLabel> {
        let mut distinct = fields.agents.clone();
        distinct.sort();
        distinct.dedup();
        if distinct.len() < 2 || distinct.len() != fields.agents.len() {
            return Err(InvalidLabel::SmallCluster);
        }
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &ClusterFields {
        &self.0
    }
}

impl TryFrom<ClusterFields> for AgentCluster {
    type Error = InvalidLabel;

    fn try_from(fields: ClusterFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<AgentCluster> for ClusterFields {
    fn from(cluster: AgentCluster) -> Self {
        cluster.0
    }
}

/// Which agent made an exchange: truth, so it is here and not beside the
/// exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeAgent {
    pub exchange: ExchangeId,
    pub agent: AgentKey,
}

/// One row of a world's labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Label {
    ExchangeAgent(ExchangeAgent),
    Transmission(ExpectedTransmission),
    AccessOnly(ExpectedAccess),
    NegativeControl(NegativeControl),
    Exemption(Exemption),
    AgentCluster(AgentCluster),
}
