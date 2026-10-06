//! Predictions: a detector's rows in the scorer's terms.
//!
//! A [`Prediction`] is one piece of evidence of one transmission: who sent,
//! who read, at which reader exchange and location, by which route and
//! carrier, and what the evidence was. [`from_transmission`] turns a
//! transmission row into predictions:
//!
//! - confirmed, classified or aggregated: one per content match, of the
//!   match's class;
//! - suspected or discarded: one per co-access, the writer to the reader at
//!   the read's exchange and tool result, carrier `tool_result`, routed
//!   through the co-access's resource, of class
//!   [`EvidenceClass::Suspected`] or [`EvidenceClass::Discarded`];
//! - detected or awaiting content: none (the detector has not decided).
//!
//! Every prediction carries its transmission's `quality`, which keys the
//! transmission rows. Detector agents become true agents through
//! [`AgentMap`]; a transmission naming an agent the map cannot place is
//! reported as an [`UnknownDetectedAgent`] and never scored.

mod agents;

use serde::{Deserialize, Serialize};

use a2a_bench_format::ids::{AgentKey, DetectorAgent, ExchangeId, TransmissionRef};
use a2a_bench_format::labels::{CarrierKind, Label, Route};
use a2a_bench_format::location::Location;
use a2a_bench_format::predictions::{Prediction as Row, Quality, State, Transmission};

pub use agents::{AgentMap, AgentMapError, exchange_agents};

use crate::class::EvidenceClass;

/// One piece of evidence a detector reported.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Prediction {
    /// The detector's transmission this evidence belongs to.
    pub transmission: TransmissionRef,
    pub from: AgentKey,
    pub to: AgentKey,
    pub reader_exchange: ExchangeId,
    pub route: Route,
    pub carrier: CarrierKind,
    pub class: EvidenceClass,
    /// The transmission's quality: its strongest match's class and carrier,
    /// or suspected, or discarded.
    pub quality: Quality,
    pub read_at: Location,
    /// Where the matched text (or the write's tool call) sits in the
    /// sender's output, when the detector knows it.
    pub origin_at: Option<Location>,
}

/// A transmission naming a detector agent with no true agent: an
/// unattributed agent, or one attributed no exchange. Its predictions are
/// dropped, as ct-eval's `unknown_detected_agent` diagnostic drops them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("transmission {transmission} names detector agent {agent}, which has no exchanges")]
pub struct UnknownDetectedAgent {
    pub transmission: TransmissionRef,
    pub agent: DetectorAgent,
}

/// The predictions `transmission` makes (module docs).
pub fn from_transmission(
    transmission: &Transmission,
    agents: &AgentMap,
) -> Result<Vec<Prediction>, UnknownDetectedAgent> {
    let fields = transmission.fields();
    let Some(quality) = fields.quality else {
        return Ok(Vec::new());
    };
    let agent = |id: &DetectorAgent| {
        agents.get(id).cloned().ok_or_else(|| UnknownDetectedAgent {
            transmission: fields.id.clone(),
            agent: id.clone(),
        })
    };
    let class = match fields.state {
        State::Suspected => EvidenceClass::Suspected,
        State::Discarded => EvidenceClass::Discarded,
        State::Confirmed | State::Classified | State::Aggregated => {
            return fields
                .matches
                .iter()
                .map(|evidence| {
                    Ok(Prediction {
                        transmission: fields.id.clone(),
                        from: agent(&evidence.from)?,
                        to: agent(&evidence.to)?,
                        reader_exchange: evidence.reader_exchange,
                        route: evidence.route.clone(),
                        carrier: evidence.carrier,
                        class: EvidenceClass::from(evidence.kind.class()),
                        quality,
                        read_at: evidence.read_at,
                        origin_at: evidence.origin_at,
                    })
                })
                .collect();
        }
        State::Detected | State::AwaitingContent => return Ok(Vec::new()),
    };
    fields
        .co_access
        .iter()
        .map(|access| {
            Ok(Prediction {
                transmission: fields.id.clone(),
                from: agent(&access.from)?,
                to: agent(&access.to)?,
                reader_exchange: access.reader_exchange,
                route: Route::Channel {
                    resource: access.resource.clone(),
                },
                carrier: CarrierKind::ToolResult,
                class,
                quality,
                read_at: access.read_at,
                origin_at: Some(access.write_at),
            })
        })
        .collect()
}

/// One world's predictions, and the transmissions left out for naming an
/// agent with no true agent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldPredictions {
    pub predictions: Vec<Prediction>,
    pub unknown: Vec<UnknownDetectedAgent>,
}

/// A world's prediction rows as predictions, in row order, against the
/// world's labels (their `exchange_agent` rows). A merged attribution fails
/// the world.
pub fn world_predictions(
    labels: &[Label],
    rows: &[Row],
) -> Result<WorldPredictions, AgentMapError> {
    let agents = AgentMap::from_rows(&exchange_agents(labels), rows)?;
    let mut out = WorldPredictions::default();
    for row in rows {
        let Row::Transmission(transmission) = row else {
            continue;
        };
        match from_transmission(transmission, &agents) {
            Ok(made) => out.predictions.extend(made),
            Err(unknown) => out.unknown.push(unknown),
        }
    }
    Ok(out)
}
