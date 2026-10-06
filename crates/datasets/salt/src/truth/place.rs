//! Labels before their places are final, and the index that places them.
//!
//! crosstalk-eval writes some negative controls whose place names a message
//! but no exchange: the shared system prompt (no reader exchange) and a
//! rejected send's origin. A bench location names its exchange, so such a
//! place goes in the first exchange (in world time order) of the reader
//! (for an origin, the sender) that carries the message, else the world's
//! first exchange that does; the control's `reader_exchange` stays absent.
//! This is how crosstalk's golden export places them, so the two agree.
//! Since a later episode's exchange may be that first carrier only once
//! all are added, labels wait as [`Draft`]s until the world's exchanges
//! are in, and get their ids (`t<index>`, the index in the world's truth)
//! when placed.

use std::collections::BTreeMap;

use a2a_bench_format::ids::{AgentKey, ExchangeId, LabelId, MessageId, SourceRef};
use a2a_bench_format::labels::{
    CarrierKind, ControlFields, ExpectedContent, ExpectedTransmission, Label, MatchNeed,
    NegativeControl, NegativeReason, Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::time::Timestamp;

use crate::SaltError;

/// A place before its exchange is known: a byte range of a message's part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub message: MessageId,
    pub part: u16,
    pub range: ByteRange,
}

impl Spot {
    fn in_exchange(self, exchange: ExchangeId) -> Location {
        Location {
            exchange,
            message: self.message,
            part: self.part,
            range: self.range,
        }
    }
}

/// A delivered message: sender to receiver, Direct, in a user turn.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub from: AgentKey,
    pub to: AgentKey,
    pub sender_exchange: Option<ExchangeId>,
    pub reader_exchange: ExchangeId,
    pub text: String,
    pub at: Spot,
    pub needs: MatchNeed,
    pub tier: Tier,
    pub source: SourceRef,
}

/// A negative control; `at` sits in `reader_exchange` when there is one.
#[derive(Debug, Clone)]
pub struct Control {
    pub from: AgentKey,
    pub to: AgentKey,
    pub reader_exchange: Option<ExchangeId>,
    pub at: Option<Spot>,
    pub origin: Option<Spot>,
    pub text: Option<String>,
    pub reason: NegativeReason,
    pub tier: Tier,
    pub source: SourceRef,
}

/// One label of the world's truth, not yet placed.
#[derive(Debug, Clone)]
pub enum Draft {
    Delivery(Delivery),
    Control(Control),
}

/// Which exchanges carry which messages: per agent and world-wide, the
/// first in world order (time, then agent, then id).
#[derive(Debug, Clone, Default)]
pub struct Carriers {
    by_agent: BTreeMap<(MessageId, AgentKey), (Timestamp, ExchangeId)>,
    first: BTreeMap<MessageId, (Timestamp, AgentKey, ExchangeId)>,
}

impl Carriers {
    /// Records exchange `id` of `agent` at `at`, carrying `messages`
    /// (request and response).
    pub fn add(
        &mut self,
        agent: &AgentKey,
        at: Timestamp,
        id: ExchangeId,
        messages: impl IntoIterator<Item = MessageId>,
    ) {
        for message in messages {
            let key = (message, agent.clone());
            let mine = (at, id);
            match self.by_agent.get(&key) {
                Some(known) if *known <= mine => {}
                _ => {
                    self.by_agent.insert(key, mine);
                }
            }
            let any = (at, agent.clone(), id);
            match self.first.get(&message) {
                Some(known) if *known <= any => {}
                _ => {
                    self.first.insert(message, any);
                }
            }
        }
    }

    /// The first exchange of `agent` that carries `message`, else the
    /// world's first that does.
    pub fn carrier(&self, message: MessageId, agent: &AgentKey) -> Option<ExchangeId> {
        self.by_agent
            .get(&(message, agent.clone()))
            .map(|(_, id)| *id)
            .or_else(|| self.first.get(&message).map(|(_, _, id)| *id))
    }
}

/// The world's labels from its drafts, in order, ids `t0`, `t1`, ….
pub fn place(drafts: Vec<Draft>, carriers: &Carriers) -> Result<Vec<Label>, SaltError> {
    let mut out = Vec::with_capacity(drafts.len());
    for (index, draft) in drafts.into_iter().enumerate() {
        let id = LabelId::new(format!("t{index}"))?;
        let carried = |spot: Spot, agent: &AgentKey| {
            carriers
                .carrier(spot.message, agent)
                .map(|exchange| spot.in_exchange(exchange))
                .ok_or_else(|| SaltError::UncarriedLocation {
                    label: id.to_string(),
                    message: spot.message,
                })
        };
        let label = match draft {
            Draft::Delivery(delivery) => {
                Label::Transmission(ExpectedTransmission::new(TransmissionFields {
                    id: id.clone(),
                    from: delivery.from,
                    to: delivery.to,
                    sender_exchange: delivery.sender_exchange,
                    reader_exchange: delivery.reader_exchange,
                    route: Route::Direct,
                    carrier: CarrierKind::UserTurn,
                    content: ExpectedContent {
                        text: delivery.text,
                        at: delivery.at.in_exchange(delivery.reader_exchange),
                    },
                    needs: delivery.needs,
                    tier: delivery.tier,
                    source: delivery.source,
                })?)
            }
            Draft::Control(control) => {
                let at = match (control.at, control.reader_exchange) {
                    (Some(spot), Some(reader)) => Some(spot.in_exchange(reader)),
                    (Some(spot), None) => Some(carried(spot, &control.to)?),
                    (None, _) => None,
                };
                let origin = match control.origin {
                    Some(spot) => Some(carried(spot, &control.from)?),
                    None => None,
                };
                Label::NegativeControl(NegativeControl::new(ControlFields {
                    id: id.clone(),
                    from: control.from,
                    to: control.to,
                    reader_exchange: control.reader_exchange,
                    at,
                    origin,
                    text: control.text,
                    reason: control.reason,
                    tier: control.tier,
                    source: control.source,
                })?)
            }
        };
        out.push(label);
    }
    Ok(out)
}
