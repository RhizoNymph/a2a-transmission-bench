//! The matcher's run over one world (see the crate docs for the algorithm).
//!
//! - [`index`]: originated spans of a response, and the shingle index with
//!   its boilerplate cutoff.
//! - [`scan`]: hits of other agents' spans in one part of a new input, their
//!   class and their route.
//! - [`group`]: rereads dropped, hits grouped into transmissions.

mod group;
mod index;
mod scan;

use std::collections::{HashMap, HashSet};

use a2a_bench_format::check::WorldInputs;
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::location::Location;
use a2a_bench_format::message::Body;
use a2a_bench_format::predictions::{ContentEvidence, Prediction};

use crate::agents::Agents;
use crate::config::ReferenceConfig;
use crate::delta::new_inputs;
use crate::error::ReferenceError;
use crate::output::{WorldOutput, WorldSummary};
use index::Postings;

/// An originated span: whose output it is in, its text in the forms
/// classification compares against, and where it is.
struct IndexedSpan {
    agent: usize,
    raw: String,
    /// `raw` under case and whitespace folding alone.
    plain: String,
    /// `raw` with one string level undone, then folded like `plain`; only
    /// when `raw` holds a backslash.
    unescaped: Option<String>,
    location: Location,
}

/// A hit before grouping: its sender, its span, the key of its route, and
/// the evidence it becomes.
struct Hit {
    sender: usize,
    span: usize,
    route_key: String,
    evidence: ContentEvidence,
}

struct Matcher<'w> {
    inputs: &'w WorldInputs,
    agents: &'w Agents,
    config: ReferenceConfig,
    spans: Vec<IndexedSpan>,
    index: HashMap<u64, Postings>,
    /// Per agent, every shingle it has read or written.
    seen: Vec<HashSet<u64>>,
    /// (span, reader, channel route key) already reported, for rereads.
    delivered: HashSet<(usize, usize, String)>,
    matches: usize,
    rereads: usize,
    out_of_reach: usize,
}

/// Runs the reference matcher over one world's inputs. The config's
/// `min_span` is raised to `k`.
pub fn run(inputs: &WorldInputs, config: ReferenceConfig) -> Result<WorldOutput, ReferenceError> {
    let agents = Agents::of(inputs)?;
    let mut matcher = Matcher {
        inputs,
        agents: &agents,
        config: config.effective(),
        spans: Vec::new(),
        index: HashMap::new(),
        seen: vec![HashSet::new(); agents.len()],
        delivered: HashSet::new(),
        matches: 0,
        rereads: 0,
        out_of_reach: 0,
    };
    let mut previous: Vec<Option<&Exchange>> = vec![None; agents.len()];
    let mut transmissions = Vec::new();
    for (exchange, &reader) in inputs.exchanges().iter().zip(agents.assignments()) {
        let mut hits = Vec::new();
        for id in new_inputs(previous.get(reader).copied().flatten(), exchange) {
            let message = matcher.message(exchange, id)?;
            if matches!(message.body(), Body::Assistant(_)) {
                continue;
            }
            for part in 0..message.part_count() {
                let Ok(part) = u16::try_from(part) else { break };
                hits.extend(matcher.scan(exchange, reader, message, part));
            }
        }
        let hits = matcher.first_reads(reader, hits);
        transmissions.extend(matcher.group(exchange, hits)?);
        for id in &exchange.response.messages {
            let message = matcher.message(exchange, *id)?;
            matcher.index_response(exchange, reader, message);
        }
        if let Some(slot) = previous.get_mut(reader) {
            *slot = Some(exchange);
        }
    }
    let summary = WorldSummary {
        agents: agents.len(),
        exchanges: inputs.exchanges().len(),
        spans: matcher.spans.len(),
        matches: matcher.matches,
        rereads: matcher.rereads,
        out_of_reach: matcher.out_of_reach,
        transmissions: transmissions.len(),
    };
    tracing::debug!(
        world = %inputs.decl().key,
        agents = summary.agents,
        spans = summary.spans,
        matches = summary.matches,
        rereads = summary.rereads,
        out_of_reach = summary.out_of_reach,
        transmissions = summary.transmissions,
        "reference matcher finished world"
    );
    let predictions = agents
        .attributions()
        .into_iter()
        .map(Prediction::Attribution)
        .chain(transmissions.into_iter().map(Prediction::Transmission))
        .collect();
    Ok(WorldOutput {
        predictions,
        summary,
    })
}

impl<'w> Matcher<'w> {
    fn message(
        &self,
        exchange: &Exchange,
        id: a2a_bench_format::ids::MessageId,
    ) -> Result<&'w a2a_bench_format::message::Message, ReferenceError> {
        self.inputs
            .message(id)
            .ok_or(ReferenceError::MissingMessage {
                exchange: exchange.id,
                message: id,
            })
    }
}

/// Letters and digits in bytes `[start, end)` of folded text (any non-ASCII
/// byte counts).
fn word_chars(text: &str, start: usize, end: usize) -> usize {
    text.as_bytes().get(start..end).map_or(0, |bytes| {
        bytes
            .iter()
            .filter(|b| b.is_ascii_alphanumeric() || **b >= 0x80)
            .count()
    })
}
