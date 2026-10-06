//! Originated spans and the shingle index.
//!
//! **Boilerplate.** A shingle held by more than `max_postings` distinct
//! originated spans is boilerplate, as crosstalk L4's frequency cutoff
//! makes it: its postings are dropped and it is ignored on lookup from then
//! on, for the rest of the world. The reference counts only originated
//! spans toward the frequency (L4 also counts scanned inputs) and has no
//! retention window: a world is one replay.

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::Message;

use super::{IndexedSpan, Matcher, word_chars};
use crate::text::classify;
use crate::text::fold::{self, Folded, fold};
use crate::text::opaque::segments;
use crate::text::shingle::{covered, shingles};

/// A shingle's postings: the spans whose originated text holds it, until
/// more than `max_postings` do; then it is boilerplate for the rest of the
/// world.
pub(super) enum Postings {
    Spans(Vec<usize>),
    Boilerplate,
}

impl Postings {
    /// Posts `span` (once), turning boilerplate past `max`.
    fn post(&mut self, span: usize, max: usize) {
        let Self::Spans(spans) = self else {
            return;
        };
        if spans.last() == Some(&span) {
            return;
        }
        if spans.len() >= max {
            *self = Self::Boilerplate;
        } else {
            spans.push(span);
        }
    }

    pub(super) fn spans(&self) -> &[usize] {
        match self {
            Self::Spans(spans) => spans,
            Self::Boilerplate => &[],
        }
    }
}

/// The folded pieces of `text` between its opaque blobs.
pub(super) fn folded_pieces(text: &str) -> Vec<Folded> {
    segments(text)
        .into_iter()
        .map(|(offset, piece)| fold(piece, offset))
        .collect()
}

impl Matcher<'_> {
    /// Indexes the originated spans of one of `agent`'s response messages,
    /// then marks all of it seen by `agent`. A span is a run of windows the
    /// agent has not seen, at least `min_span` folded bytes and
    /// `min_word_chars` letters and digits long.
    pub(super) fn index_response(&mut self, exchange: &Exchange, agent: usize, response: &Message) {
        let k = self.config.k;
        for part in 0..response.part_count() {
            let Ok(part) = u16::try_from(part) else { break };
            let Ok(text) = response.part_text(part) else {
                continue;
            };
            let mut all = Vec::new();
            for folded in folded_pieces(&text) {
                let windows = shingles(folded.text.as_bytes(), k);
                let novel: Vec<usize> = windows
                    .iter()
                    .filter(|(hash, _)| !self.seen[agent].contains(hash))
                    .map(|&(_, offset)| offset)
                    .collect();
                for (start, end) in covered(&novel, k) {
                    if end - start < self.config.min_span
                        || word_chars(&folded.text, start, end) < self.config.min_word_chars
                    {
                        continue;
                    }
                    let Some((raw_start, raw_end)) = folded.raw_range(start, end) else {
                        continue;
                    };
                    let Ok(range) = ByteRange::new(raw_start, raw_end) else {
                        continue;
                    };
                    let raw = text
                        .get(raw_start as usize..raw_end as usize)
                        .unwrap_or_default()
                        .to_owned();
                    let span = self.spans.len();
                    self.spans.push(IndexedSpan {
                        agent,
                        plain: fold::fold_plain(&raw),
                        unescaped: classify::unescaped_plain(&raw),
                        raw,
                        location: Location {
                            exchange: exchange.id,
                            message: response.id(),
                            part,
                            range,
                        },
                    });
                    for &(hash, offset) in &windows {
                        if offset >= start && offset + k <= end && !self.seen[agent].contains(&hash)
                        {
                            self.index
                                .entry(hash)
                                .or_insert_with(|| Postings::Spans(Vec::new()))
                                .post(span, self.config.max_postings);
                        }
                    }
                }
                all.extend(windows.into_iter().map(|(hash, _)| hash));
            }
            self.seen[agent].extend(all);
        }
    }
}
