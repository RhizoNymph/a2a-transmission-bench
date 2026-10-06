//! Truth rows to labels: transmissions, controls, exemptions and clusters,
//! each built through the format's checked constructors and checked
//! against the world before it is kept.

use std::collections::BTreeSet;

use a2a_bench_format::ids::{AgentKey, ExchangeId, LabelId, SourceRef};
use a2a_bench_format::labels::{
    AgentCluster, CarrierKind, ClusterFields, ClusterKind, Codec, ControlFields, Exemption,
    ExemptionFields, ExemptionReason, ExpectedContent, ExpectedTransmission, Label, MatchNeed,
    NegativeControl, NegativeReason, Route, Tier, TransmissionFields,
};
use a2a_bench_format::resource::Resource;
use a2a_bench_resource::normalized_url;

use super::Resolver;
use super::join::ReadJoin;
use crate::diagnostics::{Effect, JoinFailure, RowKind, Side};
use crate::schema::{Delivery, KeyGroup, Miss, TruthRoute, UnattributedRead};
use crate::truth_file::DeliveryKind;

/// `decoded [json_string]` when the page holds a character JSON escapes
/// (the writer's `PUT` carries it escaped inside its arguments, and the
/// reader gets it raw), `exact` otherwise: ct-eval's
/// `MatchNeed::through_json_string`.
pub fn needs(text: &str) -> MatchNeed {
    if json_escapes(text) {
        MatchNeed::Decoded {
            codecs: vec![Codec::JsonString],
        }
    } else {
        MatchNeed::Exact
    }
}

/// Whether JSON escapes a character of `text`: a quote, a backslash or a
/// control character below U+0020.
pub fn json_escapes(text: &str) -> bool {
    text.chars()
        .any(|ch| matches!(ch, '"' | '\\' | '\u{0}'..='\u{1f}'))
}

/// A label that could not be made, and why.
type Refused = String;

impl Resolver<'_> {
    fn key(name: &str) -> Result<AgentKey, Refused> {
        AgentKey::new(name).map_err(|error| error.to_string())
    }

    fn source(&self, line: usize) -> SourceRef {
        SourceRef::new(self.file, format!("line/{line}"))
    }

    fn id(text: String) -> Result<LabelId, Refused> {
        LabelId::new(text).map_err(|error| error.to_string())
    }

    /// `label` if the world accepts it.
    fn checked(&mut self, label: Label) -> Result<Label, Refused> {
        self.checker
            .check(&label)
            .map(|()| label)
            .map_err(|error| error.to_string())
    }

    pub(super) fn delivery(
        &mut self,
        kind: DeliveryKind,
        row: &Delivery,
        line: usize,
    ) -> Option<Label> {
        let row_kind = RowKind::from(kind);
        let TruthRoute::Channel { url } = &row.route;
        let resource = match normalized_url(url) {
            Ok(resource) => resource,
            Err(error) => {
                self.report(
                    line,
                    row_kind,
                    Side::Row,
                    JoinFailure::BadUrl {
                        url: url.clone(),
                        reason: error.to_string(),
                    },
                    Effect::Dropped,
                );
                return None;
            }
        };
        let read = self.read(
            line,
            row_kind,
            &row.reader_session,
            row.reader_turn,
            &row.content.at.tool_use_id,
            Some(row.content.blake3),
        )?;
        let made = match kind {
            DeliveryKind::Transmission => {
                let sender_exchange = self.write(line, row);
                self.transmission(row, sender_exchange, read, resource, line)
            }
            DeliveryKind::SelfRead => self.control(row, read, NegativeReason::SelfRead, line),
            DeliveryKind::Reread => self.control(row, read, NegativeReason::Reread, line),
        };
        match made {
            Ok(label) => Some(label),
            Err(reason) => {
                self.report(
                    line,
                    row_kind,
                    Side::Row,
                    JoinFailure::InvalidLabel { reason },
                    Effect::Dropped,
                );
                None
            }
        }
    }

    fn transmission_label(
        &self,
        row: &Delivery,
        sender_exchange: Option<ExchangeId>,
        read: &ReadJoin,
        resource: &Resource,
        line: usize,
    ) -> Result<Label, Refused> {
        ExpectedTransmission::new(TransmissionFields {
            id: Self::id(format!("line/{line}"))?,
            from: Self::key(&row.writer)?,
            to: Self::key(&row.reader)?,
            sender_exchange,
            reader_exchange: read.exchange,
            route: Route::Channel {
                resource: resource.clone(),
            },
            carrier: CarrierKind::ToolResult,
            content: ExpectedContent {
                text: read.found.text.clone(),
                at: read.found.at,
            },
            needs: needs(&read.found.text),
            tier: Tier::Construction,
            source: self.source(line),
        })
        .map(Label::Transmission)
        .map_err(|error| error.to_string())
    }

    /// A transmission; when the world refuses it only for its sender
    /// exchange (another agent's, or not before the read), it is kept
    /// without one and the refusal reported on the writer's side.
    fn transmission(
        &mut self,
        row: &Delivery,
        sender_exchange: Option<ExchangeId>,
        read: ReadJoin,
        resource: Resource,
        line: usize,
    ) -> Result<Label, Refused> {
        let label = self.transmission_label(row, sender_exchange, &read, &resource, line)?;
        let refused = match self.checked(label) {
            Ok(label) => return Ok(label),
            Err(reason) => reason,
        };
        if sender_exchange.is_none() {
            return Err(refused);
        }
        let without = self.transmission_label(row, None, &read, &resource, line)?;
        let kept = self.checked(without)?;
        self.report(
            line,
            RowKind::Transmission,
            Side::Writer,
            JoinFailure::InvalidLabel { reason: refused },
            Effect::KeptWithoutSender,
        );
        Ok(kept)
    }

    fn control(
        &mut self,
        row: &Delivery,
        read: ReadJoin,
        reason: NegativeReason,
        line: usize,
    ) -> Result<Label, Refused> {
        let label = NegativeControl::new(ControlFields {
            id: Self::id(format!("line/{line}"))?,
            from: Self::key(&row.writer)?,
            to: Self::key(&row.reader)?,
            reader_exchange: Some(read.exchange),
            at: Some(read.found.at),
            origin: None,
            text: Some(read.found.text),
            reason,
            tier: Tier::Construction,
            source: self.source(line),
        })
        .map(Label::NegativeControl)
        .map_err(|error| error.to_string())?;
        self.checked(label)
    }

    /// An unattributed read: joined like any read (session, turn, tool use
    /// id and hash), then exempt from judging, since its sender is unknown.
    pub(super) fn unattributed(&mut self, row: &UnattributedRead, line: usize) -> Option<Label> {
        let read = self.read(
            line,
            RowKind::UnattributedRead,
            &row.reader_session,
            row.reader_turn,
            &row.content.at.tool_use_id,
            Some(row.content.blake3),
        )?;
        let made = Self::key(&row.reader)
            .and_then(|to| {
                Ok(ExemptionFields {
                    id: Self::id(format!("line/{line}"))?,
                    to,
                    reader_exchange: read.exchange,
                    at: read.found.at,
                    text: Some(read.found.text),
                    reason: ExemptionReason::UnknownSender,
                    tier: Tier::Construction,
                    source: self.source(line),
                })
            })
            .and_then(|fields| Exemption::new(fields).map_err(|error| error.to_string()))
            .and_then(|exemption| self.checked(Label::Exemption(exemption)));
        match made {
            Ok(label) => Some(label),
            Err(reason) => {
                self.report(
                    line,
                    RowKind::UnattributedRead,
                    Side::Row,
                    JoinFailure::InvalidLabel { reason },
                    Effect::Dropped,
                );
                None
            }
        }
    }

    /// A miss: one control from every other agent of the world to the
    /// reader, at the read (the not-found result).
    pub(super) fn miss(&mut self, row: &Miss, line: usize, names: &BTreeSet<String>) -> Vec<Label> {
        let Some(read) = self.read(
            line,
            RowKind::Miss,
            &row.reader_session,
            row.reader_turn,
            &row.reader_tool_use_id,
            None,
        ) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in names.iter().filter(|name| **name != row.reader) {
            let made = Self::key(name)
                .and_then(|from| {
                    Ok(ControlFields {
                        id: Self::id(format!("line/{line}/{name}"))?,
                        from,
                        to: Self::key(&row.reader)?,
                        reader_exchange: Some(read.exchange),
                        at: Some(read.found.at),
                        origin: None,
                        text: None,
                        reason: NegativeReason::Miss,
                        tier: Tier::Construction,
                        source: self.source(line),
                    })
                })
                .and_then(|fields| NegativeControl::new(fields).map_err(|error| error.to_string()))
                .and_then(|control| self.checked(Label::NegativeControl(control)));
            match made {
                Ok(control) => out.push(control),
                Err(reason) => self.report(
                    line,
                    RowKind::Miss,
                    Side::Row,
                    JoinFailure::InvalidLabel { reason },
                    Effect::Dropped,
                ),
            }
        }
        out
    }

    /// A key group: agents sharing one API key, labelled as a `key_group`
    /// cluster when it holds two or more distinct agents.
    pub(super) fn cluster(&mut self, row: &KeyGroup, line: usize) -> Option<Label> {
        let distinct: BTreeSet<&String> = row.agents.iter().collect();
        if distinct.len() < 2 {
            self.report(
                line,
                RowKind::AgentCluster,
                Side::Row,
                JoinFailure::KeyGroupNotACluster {
                    key_group: row.key_group,
                    agents: row.agents.len(),
                },
                Effect::Noted,
            );
            return None;
        }
        let made = distinct
            .iter()
            .map(|name| Self::key(name))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|agents| {
                Ok(ClusterFields {
                    id: Self::id(format!("line/{line}"))?,
                    agents,
                    kind: ClusterKind::KeyGroup,
                    tier: Tier::Construction,
                    source: self.source(line),
                })
            })
            .and_then(|fields| AgentCluster::new(fields).map_err(|error| error.to_string()))
            .and_then(|cluster| self.checked(Label::AgentCluster(cluster)));
        match made {
            Ok(label) => Some(label),
            Err(reason) => {
                self.report(
                    line,
                    RowKind::AgentCluster,
                    Side::Row,
                    JoinFailure::InvalidLabel { reason },
                    Effect::Dropped,
                );
                None
            }
        }
    }
}
