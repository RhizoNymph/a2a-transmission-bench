//! τ²-bench ground truth.
//!
//! **Positives (Structural).** The agent and the user simulator talk only
//! through their turns. Each text turn one side's model wrote is a
//! transmission to the other side's next call: Direct route, UserTurn
//! carrier (the reader sees it as a user turn either way), located at the
//! whole text, `Exact` (the reader's copy is verbatim). A turn the peer
//! never answers (the closing `###STOP###`) has no reader exchange and no
//! label.
//!
//! The scenario's `known_info` travels harness → user simulator's system
//! prompt → agent's user turn. The first hop is from no agent and is not
//! labelled; the second hop is the user simulator's turn, labelled where
//! the simulator originated the text. A turn that only relays its sender's
//! own system prompt verbatim (whitespace aside: a scenario line, or the
//! transfer message the agent's policy dictates) originates nothing; it is
//! a `Boilerplate` control instead.
//!
//! **Negative controls (Structural).**
//! - `Boilerplate`: the agent's hard-coded greeting, which no model wrote,
//!   and turns that relay their sender's system prompt.
//! - `SharedSource`: each side's tool results, read from the shared
//!   environment, not written by the peer. A result read after the
//!   reader's last call has no reader exchange: its place is the reader's
//!   first exchange carrying the message (crosstalk's golden export's
//!   rule), and when there is none the bench cannot place it and the
//!   control is left out, counted in the world's `uncarried_control` note.
//!
//! **Ids.** Label `t<n>` is crosstalk-eval's `n`-th expectation of the
//! world (the golden export's numbering); a left-out control keeps its
//! number unused.

use a2a_bench_format::ids::{AgentKey, ExchangeId, LabelId, MessageId, SourceRef};
use a2a_bench_format::labels::{
    CarrierKind, ControlFields, ExpectedContent, ExpectedTransmission, Label, MatchNeed,
    NegativeControl, NegativeReason, Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::Message;

use crate::Tau2Error;
use crate::schema::RawMessage;
use crate::views::{Side, View};

/// One side of a simulation: its key, its view and its exchanges.
pub struct Participant {
    pub key: AgentKey,
    pub view: View,
    /// (record index, exchange), in record order.
    pub exchanges: Vec<(usize, ExchangeId)>,
}

impl Participant {
    /// The first exchange after record `raw`: the first whose request
    /// carries it.
    fn reader_exchange(&self, raw: usize) -> Option<ExchangeId> {
        self.exchanges
            .iter()
            .find(|(index, _)| *index > raw)
            .map(|(_, exchange)| *exchange)
    }

    fn exchange_of(&self, raw: usize) -> Option<ExchangeId> {
        self.exchanges
            .iter()
            .find(|(index, _)| *index == raw)
            .map(|(_, exchange)| *exchange)
    }

    /// The first exchange whose request or response carries `message` (by
    /// id): the system prompt, or an entry up to the exchange's own.
    fn first_carrier(&self, message: MessageId) -> Option<ExchangeId> {
        let system = self.view.system.id() == message;
        self.exchanges
            .iter()
            .find(|(raw, _)| {
                system
                    || self
                        .view
                        .entries
                        .iter()
                        .take_while(|entry| entry.raw <= *raw)
                        .any(|entry| entry.message.id() == message)
            })
            .map(|(_, exchange)| *exchange)
    }
}

/// Everything labelling one simulation needs.
pub struct SimulationLabels<'a> {
    pub file: &'a str,
    pub simulation: usize,
    pub messages: &'a [RawMessage],
    pub agent: &'a Participant,
    /// Absent when the agent works alone.
    pub user: Option<&'a Participant>,
    /// The agent's system prompt.
    pub agent_prompt: &'a str,
    /// The user simulator's system prompt.
    pub user_prompt: &'a str,
}

/// A simulation's labels, and how many controls it left out because no
/// exchange of their reader carries the message.
pub struct Labelled {
    pub labels: Vec<Label>,
    pub uncarried: u64,
}

/// Numbers labels in crosstalk-eval's truth order.
#[derive(Default)]
struct Numbering(usize);

impl Numbering {
    fn next(&mut self) -> Result<LabelId, Tau2Error> {
        let id = LabelId::new(format!("t{}", self.0))?;
        self.0 += 1;
        Ok(id)
    }
}

/// The whole text of `message`'s first part, in `exchange`.
fn whole(exchange: ExchangeId, message: &Message, raw: usize) -> Result<Location, Tau2Error> {
    let text = message.part_text(0).map_err(|_| Tau2Error::NoText(raw))?;
    let end = u32::try_from(text.len()).map_err(|_| Tau2Error::Offset)?;
    Ok(Location {
        exchange,
        message: message.id(),
        part: 0,
        range: ByteRange::new(0, end)?,
    })
}

impl SimulationLabels<'_> {
    fn side(&self, side: Side) -> Option<&Participant> {
        match side {
            Side::Agent => Some(self.agent),
            Side::User => self.user,
        }
    }

    fn source(&self, raw: usize) -> SourceRef {
        SourceRef::new(
            self.file,
            format!("/simulations/{}/messages/{raw}", self.simulation),
        )
    }

    pub fn label(&self) -> Result<Labelled, Tau2Error> {
        let mut out = Vec::new();
        let mut uncarried = 0;
        let mut ids = Numbering::default();
        let agent_prompt = collapse(self.agent_prompt);
        let user_prompt = collapse(self.user_prompt);
        for (raw, message) in self.messages.iter().enumerate() {
            match message.role.as_str() {
                "assistant" => {
                    self.turn(Side::Agent, raw, message, &agent_prompt, &mut ids, &mut out)?;
                }
                "user" => self.turn(Side::User, raw, message, &user_prompt, &mut ids, &mut out)?,
                "tool" => {
                    if !self.tool_result(raw, message, &mut ids, &mut out)? {
                        uncarried += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(Labelled {
            labels: out,
            uncarried,
        })
    }

    fn turn(
        &self,
        sender_side: Side,
        raw: usize,
        message: &RawMessage,
        sender_prompt: &str,
        ids: &mut Numbering,
        out: &mut Vec<Label>,
    ) -> Result<(), Tau2Error> {
        let (Some(sender), Some(reader)) = (self.side(sender_side), self.side(sender_side.peer()))
        else {
            return Ok(());
        };
        let Some(text) = message.text() else {
            return Ok(());
        };
        let Some(entry) = reader.view.entry(raw) else {
            return Ok(());
        };
        let Some(reader_exchange) = reader.reader_exchange(raw) else {
            return Ok(());
        };
        let at = whole(reader_exchange, &entry.message, raw)?;
        if !message.is_model_call() || relays(text, sender_prompt) {
            out.push(Label::NegativeControl(NegativeControl::new(
                ControlFields {
                    id: ids.next()?,
                    from: sender.key.clone(),
                    to: reader.key.clone(),
                    reader_exchange: Some(reader_exchange),
                    at: Some(at),
                    origin: None,
                    text: Some(text.to_owned()),
                    reason: NegativeReason::Boilerplate,
                    tier: Tier::Structural,
                    source: self.source(raw),
                },
            )?));
            return Ok(());
        }
        out.push(Label::Transmission(ExpectedTransmission::new(
            TransmissionFields {
                id: ids.next()?,
                from: sender.key.clone(),
                to: reader.key.clone(),
                sender_exchange: sender.exchange_of(raw),
                reader_exchange,
                route: Route::Direct,
                carrier: CarrierKind::UserTurn,
                content: ExpectedContent {
                    text: text.to_owned(),
                    at,
                },
                needs: MatchNeed::Exact,
                tier: Tier::Structural,
                source: self.source(raw),
            },
        )?));
        Ok(())
    }

    /// A tool result's `SharedSource` control; `false` when the control
    /// was left out because no exchange of its reader carries it.
    fn tool_result(
        &self,
        raw: usize,
        message: &RawMessage,
        ids: &mut Numbering,
        out: &mut Vec<Label>,
    ) -> Result<bool, Tau2Error> {
        let side = if message.requestor.as_deref() == Some("user") {
            Side::User
        } else {
            Side::Agent
        };
        let (Some(reader), Some(peer)) = (self.side(side), self.side(side.peer())) else {
            return Ok(true);
        };
        if message.text().is_none_or(|text| text.trim().is_empty()) {
            return Ok(true);
        }
        let Some(entry) = reader.view.entry(raw) else {
            return Ok(true);
        };
        let id = ids.next()?;
        let reader_exchange = reader.reader_exchange(raw);
        let Some(place) = reader_exchange.or_else(|| reader.first_carrier(entry.message.id()))
        else {
            tracing::debug!(
                file = self.file,
                simulation = self.simulation,
                message = raw,
                label = %id,
                "left out a shared-source control no exchange of its reader carries"
            );
            return Ok(false);
        };
        let at = whole(place, &entry.message, raw)?;
        out.push(Label::NegativeControl(NegativeControl::new(
            ControlFields {
                id,
                from: peer.key.clone(),
                to: reader.key.clone(),
                reader_exchange,
                at: Some(at),
                origin: None,
                text: None,
                reason: NegativeReason::SharedSource,
                tier: Tier::Structural,
                source: self.source(raw),
            },
        )?));
        Ok(true)
    }
}

/// The shortest turn (collapsed bytes) judged a relay: a shorter one that
/// happens to occur in the scenario ("Yes, please.") is still the
/// simulator's own.
const RELAY_MIN: usize = 24;

/// Whether a turn only relays its sender's system prompt (`collapsed`).
fn relays(text: &str, scenario: &str) -> bool {
    let turn = collapse(text);
    turn.len() >= RELAY_MIN && scenario.contains(&turn)
}

/// `text` with whitespace runs collapsed to one space and trimmed.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
