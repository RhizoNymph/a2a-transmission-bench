//! The Claude Code agent's stream: its exact model calls and the chat it
//! read through the village MCP server, as worlds with construction-tier
//! labels.
//!
//! **Exchanges.** One world per context (a session cut at compaction
//! boundaries, [`entries::contexts`]); its exchanges are the agent's calls
//! ([`calls::context`]), `reconstructed`: each SDK message id is one API
//! call, its request rebuilt from the entries before it. The system prompt
//! and per-query prompts are not recorded, so requests carry neither.
//!
//! **Originating exchanges.** Each chat message the agent read was written
//! by another agent. Its `AGENT_TALK` event keeps the author's raw model
//! response (`data.output`); the world gets one exchange of that author
//! with that response, at the event's time, and an empty request
//! (`synthetic`: the author's request is not rebuilt here).
//!
//! **Labels (construction).** The first `get_events` result that delivers
//! an `AGENT_TALK` event (keyed by event id) to a call is a transmission
//! from the author to the Claude Code agent: route `direct`, carrier
//! `tool_result`, at the reader call that first carries the result, located
//! at the content's JSON-escaped bytes in the result text. `get_events`
//! takes no argument naming a resource and the authors never call it, so
//! it is the agent's personal feed, not a shared channel.
//!
//! **Coverage** is partial: the agent also reads other agents' work
//! through repositories and the web, which this stream does not label.
//!
//! The agent's own `chat_message` calls are matched to `chat_messages` rows
//! by exact content ([`ClaudeCodeStats::chat_writes_matched`]).

pub mod calls;
pub mod entries;
pub mod events;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use a2a_bench_corpus::world::{ExchangeDraft, World, WorldAgent, WorldBuilder};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DatasetId, ExchangeId, SourceRef, WorldKey};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedContent, ExpectedTransmission, Label, Route, Tier, TransmissionFields,
};
use a2a_bench_format::message::{AssistantPart, Message, ResultContent};
use a2a_bench_format::time::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::labels::LabelIds;
use super::location::in_message;
use super::provider;
use super::schema::{ChatRow, EventRow};
use super::stream::Table;
use super::tables::{AgentInfo, Directory, events_by_id, load_directory};
use super::text::{class_name, need, tier};
use super::time::parse_timestamp;
use super::{DATASET, Error};
use calls::Context;
use entries::{Entry, EntryKind};
use events::{CHAT_MESSAGE, GET_EVENTS, talks};

/// The Claude Code agent's model string prefix.
pub const MODEL_PREFIX: &str = "claude-code::";

/// What the stream saw, summed over the worlds produced so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeCodeStats {
    pub contexts: u64,
    pub calls: u64,
    pub get_events_results: u64,
    /// Results the agent never read: the context was compacted first.
    pub get_events_unread: u64,
    pub get_events_unreadable: u64,
    /// `AGENT_TALK` deliveries from other agents, counting repeats.
    pub talk_deliveries: u64,
    pub talks_without_id: u64,
    pub own_talks: u64,
    /// Deliveries of an event already labelled at an earlier call.
    pub redeliveries: u64,
    pub unknown_speaker: u64,
    /// Events with no raw model output to originate from.
    pub unoriginated: u64,
    /// Contents not found in the result text.
    pub unlocated: u64,
    pub labels: u64,
    pub labels_by_need: BTreeMap<String, u64>,
    pub originating_exchanges: u64,
    pub chat_writes: u64,
    pub chat_writes_matched: u64,
}

/// A stream of Claude Code worlds, one per context.
pub struct ClaudeCodeStream {
    directory: Directory,
    agent: AgentInfo,
    entries: Vec<Entry>,
    contexts: Vec<Range<usize>>,
    events: HashMap<String, EventRow>,
    chat: HashMap<String, Vec<String>>,
    delivered: HashSet<String>,
    next: usize,
    limit: Option<usize>,
    stats: ClaudeCodeStats,
}

impl ClaudeCodeStream {
    /// Reads the Claude Code table, the events it delivered, and the
    /// agent's chat messages. `limit` caps the number of contexts.
    pub fn open(root: &Path, limit: Option<usize>) -> Result<Self, Error> {
        let directory = load_directory(root)?;
        let agent = directory
            .agents
            .values()
            .find(|agent| agent.model.starts_with(MODEL_PREFIX))
            .cloned()
            .ok_or_else(|| Error::NoAgent("Claude Code".to_owned()))?;
        let entries = entries::load(root)?;
        let contexts = entries::contexts(&entries);
        let ids = delivered_events(&entries);
        tracing::info!(
            entries = entries.len(),
            contexts = contexts.len(),
            events = ids.len(),
            "read the Claude Code stream"
        );
        let events = events_by_id(root, &ids)?;
        let mut chat: HashMap<String, Vec<String>> = HashMap::new();
        Table::ChatMessages.scan::<Error>(root, |line| {
            if !line.contains(&agent.id) {
                return Ok(());
            }
            let row: ChatRow = super::stream::decode(Table::ChatMessages, line)?;
            if row.agent_speaker_id.as_deref() == Some(agent.id.as_str()) {
                chat.entry(row.content).or_default().push(row.id);
            }
            Ok(())
        })?;
        Ok(Self {
            directory,
            agent,
            entries,
            contexts,
            events,
            chat,
            delivered: HashSet::new(),
            next: 0,
            limit,
            stats: ClaudeCodeStats::default(),
        })
    }

    /// The tables this mode reads.
    pub const TABLES: [Table; 5] = [
        Table::Agents,
        Table::ChatRooms,
        Table::ClaudeCodeMessages,
        Table::Events,
        Table::ChatMessages,
    ];

    pub fn stats(&self) -> &ClaudeCodeStats {
        &self.stats
    }

    /// The next context's world, or `None` after the last.
    pub fn next_world(&mut self) -> Option<Result<World, Error>> {
        let end = self
            .limit
            .map_or(self.contexts.len(), |limit| limit.min(self.contexts.len()));
        if self.next >= end {
            return None;
        }
        let index = self.next;
        self.next += 1;
        let range = self.contexts.get(index)?.clone();
        Some(self.world(index, range))
    }

    fn world(&mut self, index: usize, range: Range<usize>) -> Result<World, Error> {
        let entries = self.entries.get(range.clone()).unwrap_or_default();
        let context = calls::context(entries)?;
        let name = match entries.first() {
            Some(entry) => format!(
                "claude-code/{index:04}-{}",
                super::time::format_seconds(entry.at).replace(' ', "T")
            ),
            None => format!("claude-code/{index:04}"),
        };
        let key = WorldKey::new(name)?;
        let mut builder = WorldBuilder::new(DatasetId::new(DATASET)?, key.clone());
        let reader = builder.model_agent(&self.agent.name, &self.agent.model)?;
        let file = Table::ClaudeCodeMessages.file_name();
        let mut exchanges = Vec::with_capacity(context.calls.len());
        let mut last: Option<Timestamp> = None;
        for call in &context.calls {
            let at = after(last, call.at);
            last = Some(at);
            exchanges.push(builder.exchange(ExchangeDraft {
                agent: reader.clone(),
                at,
                model: Some(call.model.clone()),
                request: call.request.clone(),
                response: vec![call.response.clone()],
                tools: None,
                stop: Some(call.stop),
                session: None,
                turn: None,
                fidelity: Fidelity::Reconstructed,
                source: SourceRef::new(
                    file.clone(),
                    format!("/{}#message={}", call.row, call.message_id),
                ),
            })?);
        }
        self.stats.contexts += 1;
        self.stats.calls += context.calls.len() as u64;
        let mut ids = LabelIds::new(&key);
        self.label(&mut builder, &mut ids, &reader, &context, &exchanges)?;
        self.chat_writes(&context);
        Ok(builder.finish(Coverage::Partial)?)
    }

    fn label(
        &mut self,
        builder: &mut WorldBuilder,
        ids: &mut LabelIds,
        reader: &WorldAgent,
        context: &Context,
        exchanges: &[ExchangeId],
    ) -> Result<(), Error> {
        let mut speakers: BTreeMap<String, (WorldAgent, Option<Timestamp>)> = BTreeMap::new();
        for result in context.results.iter().filter(|r| r.tool == GET_EVENTS) {
            self.stats.get_events_results += 1;
            let Some(reader_call) = result.reader else {
                self.stats.get_events_unread += 1;
                continue;
            };
            let Ok(text) = result.message.part_text(result.part) else {
                self.stats.get_events_unreadable += 1;
                continue;
            };
            let Ok((delivered, without_id)) = talks(&text) else {
                self.stats.get_events_unreadable += 1;
                continue;
            };
            self.stats.talks_without_id += without_id as u64;
            for talk in delivered {
                if talk.speaker == self.agent.name {
                    self.stats.own_talks += 1;
                    continue;
                }
                self.stats.talk_deliveries += 1;
                if !self.delivered.insert(talk.event.clone()) {
                    self.stats.redeliveries += 1;
                    continue;
                }
                let Some(speaker) = self.directory.by_name(&talk.speaker).cloned() else {
                    self.stats.unknown_speaker += 1;
                    continue;
                };
                let Some((event_at, output)) = self.events.get(&talk.event).and_then(|row| {
                    let output = row.data.get("output").filter(|o| !o.is_null())?;
                    Some((parse_timestamp(&row.created_at).ok()?, output.clone()))
                }) else {
                    self.stats.unoriginated += 1;
                    continue;
                };
                let Some((start, end)) = talk.range else {
                    self.stats.unlocated += 1;
                    continue;
                };
                let (key, last) = match speakers.get(&speaker.id) {
                    Some(entry) => entry.clone(),
                    None => (builder.model_agent(&speaker.name, &speaker.model)?, None),
                };
                let response = provider::response(&output, &talk.event);
                let message = Message::new(response.body())?;
                let at = after(last, event_at);
                let sender_exchange = builder.exchange(ExchangeDraft {
                    agent: key.clone(),
                    at,
                    model: Some(speaker.model.clone()),
                    request: Vec::new(),
                    response: vec![message.clone()],
                    tools: None,
                    stop: Some(response.stop),
                    session: None,
                    turn: None,
                    fidelity: Fidelity::Synthetic,
                    source: SourceRef::new(
                        Table::Events.file_name(),
                        format!("/{}/data/output", talk.event),
                    ),
                })?;
                speakers.insert(speaker.id.clone(), (key.clone(), Some(at)));
                self.stats.originating_exchanges += 1;
                let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end)) else {
                    self.stats.unlocated += 1;
                    continue;
                };
                let Some(&reader_exchange) = exchanges.get(reader_call) else {
                    continue;
                };
                let at_location =
                    in_message(reader_exchange, &result.message, result.part, start, end)?;
                let needs = need(&message, &talk.escaped);
                let tier = tier(&needs, Tier::Construction);
                *self
                    .stats
                    .labels_by_need
                    .entry(class_name(&needs).to_owned())
                    .or_default() += 1;
                self.stats.labels += 1;
                let label = ExpectedTransmission::new(TransmissionFields {
                    id: ids.take()?,
                    from: key.key().clone(),
                    to: reader.key().clone(),
                    sender_exchange: Some(sender_exchange),
                    reader_exchange,
                    route: Route::Direct,
                    carrier: CarrierKind::ToolResult,
                    content: ExpectedContent {
                        text: talk.escaped.clone(),
                        at: at_location,
                    },
                    needs,
                    tier,
                    source: SourceRef::new(
                        Table::ClaudeCodeMessages.file_name(),
                        format!(
                            "/{}/content/message/content/{}#event={}",
                            result.row, result.block, talk.event
                        ),
                    ),
                })?;
                builder.label(Label::Transmission(label))?;
            }
        }
        Ok(())
    }

    fn chat_writes(&mut self, context: &Context) {
        for tool_use in context.tool_uses.iter().filter(|u| u.name == CHAT_MESSAGE) {
            self.stats.chat_writes += 1;
            if events::chat_content(&tool_use.arguments)
                .is_some_and(|content| self.chat.contains_key(&content))
            {
                self.stats.chat_writes_matched += 1;
            }
        }
    }
}

/// `at`, or one microsecond after `last` when it is not later.
pub fn after(last: Option<Timestamp>, at: Timestamp) -> Timestamp {
    match last {
        Some(last) if at <= last => Timestamp::from_micros(last.as_micros() + 1),
        _ => at,
    }
}

/// The ids of every `AGENT_TALK` event a `get_events` result delivered.
fn delivered_events(entries: &[Entry]) -> HashSet<String> {
    let mut calls: HashSet<&str> = HashSet::new();
    for entry in entries {
        if let EntryKind::Assistant { parts, .. } = &entry.kind {
            for part in parts {
                if let AssistantPart::ToolCall(call) = part
                    && call.name == GET_EVENTS
                {
                    calls.insert(call.call_id.as_str());
                }
            }
        }
    }
    let mut ids = HashSet::new();
    for entry in entries {
        let EntryKind::ToolResults(results) = &entry.kind else {
            continue;
        };
        for (_, result) in results {
            if !calls.contains(result.call_id.as_str()) {
                continue;
            }
            for content in &result.content {
                if let ResultContent::Text { text } = content
                    && let Ok(value) = serde_json::from_str::<Value>(text)
                {
                    for event in value
                        .get("events")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if event.get("actionType").and_then(Value::as_str) == Some("AGENT_TALK")
                            && let Some(id) = event.get("id").and_then(Value::as_str)
                        {
                            ids.insert(id.to_owned());
                        }
                    }
                }
            }
        }
    }
    ids
}
