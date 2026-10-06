//! Scanning one part of a new input for other agents' spans.
//!
//! Each piece between opaque blobs is folded and shingled; every run of
//! windows shared with one span, at least `min_span` folded bytes and
//! `min_word_chars` letters and digits long, is a candidate, classified
//! against the span ([`classify`](crate::text::classify)). Candidate tokens
//! are also decoded (base64, hex, URL encoding) and looked up; a decoded
//! token sharing a run of at least `min_span` with a span is a `decoded`
//! hit over the whole token. Per (span, raw range) the first in-reach
//! reading wins; a range only two string levels explain is counted out of
//! reach and reported as no match.

use std::collections::BTreeMap;

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::labels::{CarrierKind, Route};
use a2a_bench_format::location::{ByteRange, Location};
use a2a_bench_format::message::{Body, Message, ToolPart};
use a2a_bench_format::predictions::{ContentEvidence, MatchKind};

use super::{Hit, Matcher, word_chars};
use crate::route::{extract_resource, find_call};
use crate::text::classify::{self, Classified, SpanForms};
use crate::text::decode::decode_candidates;
use crate::text::fold::fold;
use crate::text::opaque::segments;
use crate::text::shingle::{covered, shingles};

impl Matcher<'_> {
    /// Hits of other agents' spans in one part of one new input of `reader`;
    /// the part's shingles are then seen by `reader`.
    pub(super) fn scan(
        &mut self,
        exchange: &Exchange,
        reader: usize,
        message: &Message,
        part: u16,
    ) -> Vec<Hit> {
        let Ok(text) = message.part_text(part) else {
            return Vec::new();
        };
        let k = self.config.k;
        // `None`: a hit only two string levels explain, unless another
        // reading of the same range is in reach.
        let mut found: BTreeMap<(usize, u32, u32), Option<MatchKind>> = BTreeMap::new();
        for (offset, piece) in segments(&text) {
            let folded = fold(piece, offset);
            let windows = shingles(folded.text.as_bytes(), k);
            for (span, positions) in self.lookup(&windows, reader) {
                for (start, end) in covered(&positions, k) {
                    if end - start < self.config.min_span
                        || word_chars(&folded.text, start, end) < self.config.min_word_chars
                    {
                        continue;
                    }
                    let Some((raw_start, raw_end)) = folded.raw_range(start, end) else {
                        continue;
                    };
                    let read = text
                        .get(raw_start as usize..raw_end as usize)
                        .unwrap_or_default();
                    let indexed = &self.spans[span];
                    let forms = SpanForms {
                        raw: &indexed.raw,
                        plain: &indexed.plain,
                        unescaped: indexed.unescaped.as_deref(),
                    };
                    let kind = match classify::classify_forms(forms, read) {
                        Classified::Match(kind) => Some(kind),
                        Classified::TwoStringLevels => None,
                    };
                    let slot = found.entry((span, raw_start, raw_end)).or_insert(None);
                    if slot.is_none() {
                        *slot = kind;
                    }
                }
            }
            self.seen[reader].extend(windows.iter().map(|&(hash, _)| hash));
            for decoded in decode_candidates(piece, offset, self.config.min_decoded) {
                let folded_decoded = fold(&decoded.text, 0);
                let decoded_windows = shingles(folded_decoded.text.as_bytes(), k);
                for (span, positions) in self.lookup(&decoded_windows, reader) {
                    let longest = covered(&positions, k)
                        .into_iter()
                        .map(|(start, end)| end - start)
                        .max()
                        .unwrap_or(0);
                    if longest < self.config.min_span {
                        continue;
                    }
                    let (Ok(start), Ok(end)) =
                        (u32::try_from(decoded.start), u32::try_from(decoded.end))
                    else {
                        continue;
                    };
                    let slot = found.entry((span, start, end)).or_insert(None);
                    if slot.is_none() {
                        *slot = Some(MatchKind::Decoded {
                            codecs: vec![decoded.codec],
                        });
                    }
                }
            }
        }
        let mut hits = Vec::with_capacity(found.len());
        for ((span, start, end), kind) in found {
            let Some(kind) = kind else {
                self.out_of_reach += 1;
                continue;
            };
            let Ok(range) = ByteRange::new(start, end) else {
                continue;
            };
            let read_at = Location {
                exchange: exchange.id,
                message: message.id(),
                part,
                range,
            };
            if let Some(hit) = self.hit(exchange, reader, message, part, span, read_at, kind) {
                hits.push(hit);
            }
        }
        hits
    }

    /// Spans of agents other than `reader` sharing shingles with `windows`,
    /// with the offsets that hit, in span order.
    fn lookup(&self, windows: &[(u64, usize)], reader: usize) -> BTreeMap<usize, Vec<usize>> {
        let mut hits: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(hash, offset) in windows {
            if let Some(postings) = self.index.get(&hash) {
                for &span in postings.spans() {
                    if self.spans[span].agent != reader {
                        hits.entry(span).or_default().push(offset);
                    }
                }
            }
        }
        hits
    }

    #[allow(clippy::too_many_arguments)]
    fn hit(
        &mut self,
        exchange: &Exchange,
        reader: usize,
        message: &Message,
        part: u16,
        span: usize,
        read_at: Location,
        kind: MatchKind,
    ) -> Option<Hit> {
        let (carrier, route, route_key) = self.route_of(exchange, message, part);
        let indexed = &self.spans[span];
        let sender = indexed.agent;
        let evidence = ContentEvidence {
            from: self.agents.name(sender)?.clone(),
            to: self.agents.name(reader)?.clone(),
            reader_exchange: exchange.id,
            read_at,
            origin_at: Some(indexed.location),
            kind,
            carrier,
            route,
        };
        self.matches += 1;
        Some(Hit {
            sender,
            span,
            route_key,
            evidence,
        })
    }

    /// The carrier, route and route key of a hit in `part` of `message`.
    fn route_of(
        &self,
        exchange: &Exchange,
        message: &Message,
        part: u16,
    ) -> (CarrierKind, Route, String) {
        let user_turn = || {
            (
                CarrierKind::UserTurn,
                Route::Direct,
                "direct:user_turn".to_owned(),
            )
        };
        match message.body() {
            Body::System(_) => (
                CarrierKind::SystemPrompt,
                Route::Direct,
                "direct:system_prompt".to_owned(),
            ),
            Body::Tool(results) => {
                let Some(ToolPart::ToolResult(result)) = results.get(usize::from(part)) else {
                    return user_turn();
                };
                let call = find_call(self.inputs, exchange, &result.call_id);
                match call.and_then(extract_resource) {
                    Some(resource) => (
                        CarrierKind::ToolResult,
                        Route::Channel {
                            resource: resource.resource(),
                        },
                        format!("channel:{}", resource.key()),
                    ),
                    None => {
                        let name = call.map_or("unknown", |call| call.name.as_str());
                        (
                            CarrierKind::ToolResult,
                            Route::Direct,
                            format!("direct:tool:{name}"),
                        )
                    }
                }
            }
            Body::User(_) | Body::Assistant(_) => user_turn(),
        }
    }
}
