//! Pass 2: channel transmissions, reread controls and relays, in revision
//! (so read) order.
//!
//! **Rereads.** A reader that edits a page again reads it again, and the
//! body still holds lines it already received. A run of lines from a
//! revision whose lines this reader already read on this page is a
//! `reread` control at the later read, not another transmission: the
//! transmission is at the first read. Several runs of one revision in one
//! read are all transmissions; a revision's lines count as received once
//! that read's labels are emitted.

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_corpus::world::WorldBuilder;
use a2a_bench_format::ids::{AgentKey, ExchangeId, LabelId, MessageId, SourceRef};
use a2a_bench_format::labels::{
    CarrierKind, Codec, ControlFields, ExpectedContent, ExpectedTransmission, Label, MatchNeed,
    NegativeControl, NegativeReason, Route, Tier, TransmissionFields,
};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::resource::Resource;

use super::messages::canonical_args;
use super::turns::{ReadRecord, RevRecord};
use super::{PageRevs, inserted_text};
use crate::REVISIONS_FILE;
use crate::attribution::{line_byte_range, lines, runs};
use crate::error::WikiError;
use crate::resource::page_url;
use crate::schema::Revision;
use crate::tools;

/// The shortest labelled content, in bytes: the reference matcher's span
/// floor, so a label names content a matcher could in principle find.
const MIN_BYTES: usize = 24;
/// The fewest letters and digits a labelled content holds, so a label is
/// never a run of wiki markup punctuation.
const MIN_WORD_CHARS: usize = 20;

/// Which revisions' lines each reader has read on each page: (reader
/// identity, page id, source revision id).
#[derive(Default)]
pub struct Received(BTreeSet<(String, String, String)>);

impl Received {
    fn key(reader: &str, page: &Revision, source: &Revision) -> (String, String, String) {
        (
            reader.to_owned(),
            page.page_id.clone(),
            source.rev_id.clone(),
        )
    }

    fn contains(&self, reader: &str, page: &Revision, source: &Revision) -> bool {
        self.0.contains(&Self::key(reader, page, source))
    }
}

/// One page read: the revision whose edit read it and the revision read.
pub struct Read<'r, 'a> {
    pub rev: &'r Revision,
    pub prev: &'r Revision,
    pub page: &'r PageRevs<'a>,
    /// `rev`'s page-local index (`prev` is the one before).
    pub li: usize,
}

/// The agent `identity` of the world `builder` is building.
fn agent(identity: String) -> Result<AgentKey, WikiError> {
    Ok(AgentKey::new(identity)?)
}

fn location(
    exchange: ExchangeId,
    message: MessageId,
    start: u32,
    end: u32,
) -> Result<Location, WikiError> {
    Ok(Location {
        exchange,
        message,
        part: 0,
        range: ByteRange::new(start, end)?,
    })
}

/// A channel transmission from each earlier author whose run of lines
/// survives into the body `read.rev`'s author read, or a reread control
/// when that reader already read the run's revision on this page.
pub fn channel(
    builder: &mut WorldBuilder,
    read: &Read<'_, '_>,
    at_read: &ReadRecord,
    resource: &Resource,
    records: &BTreeMap<&str, RevRecord>,
    received: &mut Received,
) -> Result<(), WikiError> {
    let (rev, prev, page) = (read.rev, read.prev, read.page);
    let reader_id = rev.identity();
    let body_prev = &prev.body;
    let prev_lines = lines(body_prev);
    let Some(prev_sources) = page.sources.get(read.li - 1) else {
        return Ok(());
    };
    let mut read_now = Vec::new();
    for run in runs(prev_sources) {
        let Some(&author) = page.revs.get(run.source) else {
            continue;
        };
        if author.identity() == reader_id {
            continue;
        }
        let Some((start, end)) = line_byte_range(&prev_lines, run.from, run.to) else {
            continue;
        };
        let Some(text) = body_prev.get(start as usize..end as usize) else {
            continue;
        };
        if text.len() < MIN_BYTES || word_chars(text) < MIN_WORD_CHARS {
            continue;
        }
        let at = location(at_read.exchange, at_read.result, start, end)?;
        let source = SourceRef::new(
            REVISIONS_FILE,
            format!("/rev/{}/read/run/{}", rev.rev_id, run.from),
        );
        let id = LabelId::new(source.path())?;
        if received.contains(&reader_id, rev, author) {
            let control = NegativeControl::new(ControlFields {
                id,
                from: agent(author.identity())?,
                to: agent(reader_id.clone())?,
                reader_exchange: Some(at_read.exchange),
                at: Some(at),
                origin: None,
                text: Some(text.to_owned()),
                reason: NegativeReason::Reread,
                tier: Tier::Heuristic,
                source,
            })?;
            builder.label(Label::NegativeControl(control))?;
            continue;
        }
        read_now.push(Received::key(&reader_id, rev, author));
        let label = ExpectedTransmission::new(TransmissionFields {
            id,
            from: agent(author.identity())?,
            to: agent(reader_id.clone())?,
            sender_exchange: records
                .get(author.rev_id.as_str())
                .map(|record| record.edit.exchange),
            reader_exchange: at_read.exchange,
            route: Route::Channel {
                resource: resource.clone(),
            },
            carrier: CarrierKind::ToolResult,
            content: ExpectedContent {
                text: text.to_owned(),
                at,
            },
            needs: through_json_string(text),
            tier: Tier::Heuristic,
            source,
        })?;
        builder.label(Label::Transmission(label))?;
    }
    received.0.extend(read_now);
    Ok(())
}

/// A relay (`reader_output`) for each line `read.rev` inserted that an
/// earlier distinct author wrote, verbatim, in the body it read, located in
/// the write call's canonical arguments.
pub fn relay(
    builder: &mut WorldBuilder,
    read: &Read<'_, '_>,
    record: &RevRecord,
    resource: &Resource,
    records: &BTreeMap<&str, RevRecord>,
) -> Result<(), WikiError> {
    let (rev, prev, page, own) = (read.rev, read.prev, read.page, read.li);
    let reader_id = rev.identity();
    let (Some(prev_sources), Some(own_sources)) =
        (page.sources.get(own - 1), page.sources.get(own))
    else {
        return Ok(());
    };
    // Lines of the prior body an earlier author wrote, keyed by text.
    let prev_lines = lines(&prev.body);
    let mut earlier: BTreeMap<&str, &Revision> = BTreeMap::new();
    for (line, &src) in prev_lines.iter().zip(prev_sources) {
        let Some(&author) = page.revs.get(src) else {
            continue;
        };
        if author.identity() != reader_id {
            earlier.entry(*line).or_insert(author);
        }
    }
    let inserted = inserted_text(&rev.body, own_sources, own);
    let url = page_url(&rev.wiki, &rev.name);
    let Ok(canonical) = canonical_args(&tools::write_args(&url, &inserted)) else {
        return Ok(());
    };
    let args_text = canonical.as_str();
    for (index, (line, &src)) in lines(&rev.body).iter().zip(own_sources).enumerate() {
        if src != own
            || line.len() < MIN_BYTES
            || word_chars(line) < MIN_WORD_CHARS
            || json_escapes(line)
        {
            continue;
        }
        let Some(&author) = earlier.get(*line) else {
            continue;
        };
        let Some(offset) = args_text.find(*line) else {
            continue;
        };
        let (Ok(start), Ok(end)) = (u32::try_from(offset), u32::try_from(offset + line.len()))
        else {
            continue;
        };
        let at = location(record.edit.exchange, record.edit.response, start, end)?;
        let path = format!("/rev/{}/relay", rev.rev_id);
        let label = ExpectedTransmission::new(TransmissionFields {
            id: LabelId::new(format!("{path}/{index}"))?,
            from: agent(author.identity())?,
            to: agent(reader_id.clone())?,
            sender_exchange: records
                .get(author.rev_id.as_str())
                .map(|record| record.edit.exchange),
            reader_exchange: record.edit.exchange,
            route: Route::Channel {
                resource: resource.clone(),
            },
            carrier: CarrierKind::ReaderOutput,
            content: ExpectedContent {
                text: (*line).to_owned(),
                at,
            },
            needs: MatchNeed::Exact,
            tier: Tier::Heuristic,
            source: SourceRef::new(REVISIONS_FILE, path),
        })?;
        builder.label(Label::Transmission(label))?;
    }
    Ok(())
}

/// `Exact`, or `Decoded([JsonString])` when `text` holds a character JSON
/// escapes (it sits escaped inside the writer's tool arguments).
pub fn through_json_string(text: &str) -> MatchNeed {
    if json_escapes(text) {
        MatchNeed::Decoded {
            codecs: vec![Codec::JsonString],
        }
    } else {
        MatchNeed::Exact
    }
}

/// Whether JSON escapes a character of `text` (`"`, `\` or a control).
pub fn json_escapes(text: &str) -> bool {
    text.chars()
        .any(|ch| matches!(ch, '"' | '\\' | '\u{0}'..='\u{1f}'))
}

/// Letters and digits in `text` (ASCII alphanumerics and any non-ASCII
/// byte), as the reference matcher counts them.
pub fn word_chars(text: &str) -> usize {
    text.bytes()
        .filter(|b| b.is_ascii_alphanumeric() || *b >= 0x80)
        .count()
}
