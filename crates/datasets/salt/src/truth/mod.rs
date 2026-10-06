//! SALT ground truth, one episode at a time.
//!
//! **Positives (construction).** Every `channel_transcript` entry is a
//! delivered message: the sender's `send_message` argument, verbatim, in the
//! receiver's next user turn after `[round=r/n][from=…][type=…]` and a blank
//! line. Its label: sender to receiver, Direct route, UserTurn carrier, at
//! the receiver's first call after the turn, located at the content bytes.
//! It needs an exact match unless the content holds a character JSON
//! escapes (a quote, a backslash, a control character): the sender's copy
//! sits escaped inside canonical tool-call arguments, so then it needs one
//! level of JSON string decoding ([`MatchNeed::through_json_string`]).
//!
//! **Forwarding (`forwarding` tier).** A delivery whose content the sender
//! relayed from its own tool output (at least half of it, folded, in
//! 24-byte shingles of a tool result the sender received before the
//! sending call; [`crate::forwarding`]) keeps its label but takes the
//! `forwarding` tier instead of `construction`.
//!
//! **Negative controls.**
//! - `rejected_send` (construction): a `send_message` whose event failed
//!   (the 200-character limit) was never delivered. Its result is a tool
//!   result with outcome `error`. Its origin is the failed call's whole
//!   arguments: a prediction whose evidence lies there claims text arrived
//!   that never did.
//! - `no_sender_exchange` (construction): in `controlled_peer` Bob is
//!   scripted and makes no calls, so his delivered messages have no
//!   originating exchange. (Alice's messages to a scripted Bob have no
//!   reader exchange and are not labelled.)
//! - `shared_source` (structural): each agent's system prompt (about 95%
//!   shared with the peer's), once per reader per world, and results of
//!   tools that read the shared task resources ([`SHARED_TOOLS`]).
//! - `boilerplate` (structural): the harness's own user turns in the
//!   episode's region (phase headers, round prompts, feedback), including
//!   the peer's task text the communication header quotes.

pub mod place;

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_format::ids::{AgentKey, ExchangeId, MessageId, SourceRef};
use a2a_bench_format::labels::{MatchNeed, NegativeReason, Tier};
use a2a_bench_format::location::ByteRange;
use a2a_bench_format::message::{AssistantPart, Body};

use crate::episode::{AgentEpisode, delivered_turn};
use crate::forwarding::ToolOutput;
use crate::messages::{argument, content_text};
use crate::schema::Episode;
use place::{Control, Delivery, Draft, Spot};

/// Tools whose results come from resources both agents read.
pub const SHARED_TOOLS: &[&str] = &[
    "inspect_database",
    "query_database",
    "read_code",
    "read_source",
    "resolve_records",
];

/// One agent's reconstructed episode with the exchanges its turns got.
pub struct Labelled<'a> {
    pub key: AgentKey,
    pub episode: &'a AgentEpisode,
    /// The exchange of each turn, in turn order; empty for a scripted agent.
    pub exchanges: Vec<ExchangeId>,
    pub scripted: bool,
}

impl Labelled<'_> {
    /// The exchange of the first call after message `index`.
    fn reader_exchange(&self, index: usize) -> Option<ExchangeId> {
        let position = self.episode.turn_after(index)?;
        self.exchanges.get(position).copied()
    }

    /// The exchange of the turn whose response is message `index`.
    fn exchange_of_response(&self, index: usize) -> Option<ExchangeId> {
        let position = self.episode.turn_of_response(index)?;
        self.exchanges.get(position).copied()
    }

    fn message_id(&self, index: usize) -> Option<MessageId> {
        self.episode.messages.get(index).map(|m| m.id())
    }
}

/// Labels for one episode, given every agent's reconstruction.
pub struct EpisodeLabels<'a> {
    pub file: &'a str,
    pub position: usize,
    pub episode: &'a Episode,
    pub agents: &'a [Labelled<'a>],
    /// System prompts already labelled in this world, per reader.
    pub seen_system: &'a mut BTreeSet<(AgentKey, MessageId)>,
}

impl EpisodeLabels<'_> {
    fn agent(&self, name: &str) -> Option<&Labelled<'_>> {
        self.agents.iter().find(|agent| agent.key.as_str() == name)
    }

    fn source(&self, path: String) -> SourceRef {
        SourceRef::new(self.file, path)
    }

    /// The episode's labels: deliveries, rejected sends, then shared
    /// sources and boilerplate.
    pub fn label(mut self) -> Vec<Draft> {
        let mut out = Vec::new();
        self.deliveries(&mut out);
        self.rejected_sends(&mut out);
        self.shared_and_boilerplate(&mut out);
        out
    }

    fn deliveries(&self, out: &mut Vec<Draft>) {
        let outputs: BTreeMap<&str, ToolOutput> = self
            .agents
            .iter()
            .filter(|agent| !agent.scripted)
            .map(|agent| (agent.key.as_str(), tool_output(agent.episode)))
            .collect();
        for (at, delivery) in self.episode.channel_transcript.iter().enumerate() {
            let (Some(sender), Some(receiver)) =
                (self.agent(&delivery.sender), self.agent(&delivery.receiver))
            else {
                continue;
            };
            if receiver.scripted {
                continue;
            }
            let Some((&index, _)) = receiver
                .episode
                .delivered
                .iter()
                .find(|(_, event)| **event == delivery.event_id)
            else {
                continue;
            };
            let Some(reader_exchange) = receiver.reader_exchange(index) else {
                continue;
            };
            let Some(raw) = receiver.episode.raw.get(index) else {
                continue;
            };
            let text = content_text(&raw.content);
            let Some(turn) = delivered_turn(&text) else {
                continue;
            };
            let (Some(message), Ok(start), Ok(end)) = (
                receiver.message_id(index),
                u32::try_from(turn.content_start),
                u32::try_from(text.len()),
            ) else {
                continue;
            };
            let Ok(range) = ByteRange::new(start, end) else {
                continue;
            };
            let spot = Spot {
                message,
                part: 0,
                range,
            };
            let source = self.source(format!(
                "/results/{}/channel_transcript/{at}",
                self.position
            ));
            if sender.scripted {
                out.push(Draft::Control(Control {
                    from: sender.key.clone(),
                    to: receiver.key.clone(),
                    reader_exchange: Some(reader_exchange),
                    at: Some(spot),
                    origin: None,
                    text: Some(delivery.content.clone()),
                    reason: NegativeReason::NoSenderExchange,
                    tier: Tier::Construction,
                    source,
                }));
                continue;
            }
            let call = sender.episode.call_of_event(delivery.event_id);
            let sender_exchange =
                call.and_then(|(message, _)| sender.exchange_of_response(message));
            let forwarded = call.is_some_and(|(message, _)| {
                outputs
                    .get(sender.key.as_str())
                    .is_some_and(|output| output.forwards(&delivery.content, message))
            });
            out.push(Draft::Delivery(Delivery {
                from: sender.key.clone(),
                to: receiver.key.clone(),
                sender_exchange,
                reader_exchange,
                text: delivery.content.clone(),
                at: spot,
                needs: MatchNeed::through_json_string(&delivery.content),
                tier: if forwarded {
                    Tier::Forwarding
                } else {
                    Tier::Construction
                },
                source,
            }));
        }
    }

    /// A failed send is never delivered; this control checks no detector
    /// credits its text anyway.
    fn rejected_sends(&self, out: &mut Vec<Draft>) {
        for (at, event) in self.episode.events.iter().enumerate() {
            if event.tool.as_deref() != Some("send_message") || event.success != Some(false) {
                continue;
            }
            let Some(recipient) = event.recipient.as_deref() else {
                continue;
            };
            let (Some(sender), Some(receiver)) = (self.agent(&event.actor), self.agent(recipient))
            else {
                continue;
            };
            if sender.scripted || receiver.scripted {
                continue;
            }
            let Some((message, call)) = sender.episode.call_of_event(event.event_id) else {
                continue;
            };
            let Some(raw_call) = sender
                .episode
                .raw
                .get(message)
                .and_then(|raw| raw.tool_calls.as_ref())
                .and_then(|calls| calls.get(call))
            else {
                continue;
            };
            let Some(origin) = call_spot(sender.episode, message, &raw_call.id) else {
                continue;
            };
            let text =
                argument(&raw_call.function.arguments, "content").or_else(|| event.content.clone());
            out.push(Draft::Control(Control {
                from: sender.key.clone(),
                to: receiver.key.clone(),
                reader_exchange: None,
                at: None,
                origin: Some(origin),
                text,
                reason: NegativeReason::RejectedSend,
                tier: Tier::Construction,
                source: self.source(format!("/results/{}/events/{at}", self.position)),
            }));
        }
    }

    fn shared_and_boilerplate(&mut self, out: &mut Vec<Draft>) {
        let mut labels = Vec::new();
        for reader in self.agents.iter().filter(|agent| !agent.scripted) {
            for peer in self.agents.iter().filter(|agent| agent.key != reader.key) {
                let episode = reader.episode;
                for (index, raw) in episode.raw.iter().enumerate() {
                    let Some(message) = reader.message_id(index) else {
                        continue;
                    };
                    let path = format!(
                        "/results/{}/agents/{}/messages/{index}",
                        self.position, reader.key
                    );
                    let text = content_text(&raw.content);
                    if text.trim().is_empty() {
                        continue;
                    }
                    let (reason, reader_exchange) = match raw.role.as_str() {
                        "system" if index == 0 => {
                            if !self.seen_system.insert((reader.key.clone(), message)) {
                                continue;
                            }
                            (NegativeReason::SharedSource, None)
                        }
                        "tool"
                            if index >= episode.start
                                && raw
                                    .tool_call_id
                                    .as_deref()
                                    .is_some_and(|id| shared_tool_call(episode, id)) =>
                        {
                            (NegativeReason::SharedSource, reader.reader_exchange(index))
                        }
                        "user"
                            if index >= episode.start
                                && !episode.delivered.contains_key(&index) =>
                        {
                            (NegativeReason::Boilerplate, reader.reader_exchange(index))
                        }
                        _ => continue,
                    };
                    if reason != NegativeReason::SharedSource && reader_exchange.is_none() {
                        continue;
                    }
                    let Ok(len) = u32::try_from(text.len()) else {
                        continue;
                    };
                    let Ok(range) = ByteRange::new(0, len) else {
                        continue;
                    };
                    labels.push(Draft::Control(Control {
                        from: peer.key.clone(),
                        to: reader.key.clone(),
                        reader_exchange,
                        at: Some(Spot {
                            message,
                            part: 0,
                            range,
                        }),
                        origin: None,
                        text: None,
                        reason,
                        tier: Tier::Structural,
                        source: self.source(path.clone()),
                    }));
                }
            }
        }
        out.extend(labels);
    }
}

/// Every tool result in the agent's message list, for the forwarding rule.
fn tool_output(episode: &AgentEpisode) -> ToolOutput {
    let mut output = ToolOutput::default();
    for (index, raw) in episode.raw.iter().enumerate() {
        if raw.role == "tool" {
            output.add(index, &content_text(&raw.content));
        }
    }
    output
}

/// The whole part text of tool call `id`'s arguments in message `index` of
/// the episode: none when the call is missing or its arguments are empty.
fn call_spot(episode: &AgentEpisode, index: usize, id: &str) -> Option<Spot> {
    let message = episode.messages.get(index)?;
    let Body::Assistant(parts) = message.body() else {
        return None;
    };
    let part = parts
        .iter()
        .position(|part| matches!(part, AssistantPart::ToolCall(call) if call.call_id == id))?;
    let part = u16::try_from(part).ok()?;
    let text = message.part_text(part).ok()?;
    let range = ByteRange::new(0, u32::try_from(text.len()).ok()?).ok()?;
    Some(Spot {
        message: message.id(),
        part,
        range,
    })
}

/// Whether call `id` in the episode's region is to a shared-resource tool.
fn shared_tool_call(episode: &AgentEpisode, id: &str) -> bool {
    episode.raw.iter().skip(episode.start).any(|message| {
        message
            .tool_calls
            .iter()
            .flatten()
            .any(|call| call.id == id && SHARED_TOOLS.contains(&call.function.name.as_str()))
    })
}
