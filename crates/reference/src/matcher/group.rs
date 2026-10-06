//! Rereads and grouping.
//!
//! **Rereads.** A channel hit on a span the matcher already reported to the
//! same reader through the same channel, in an earlier exchange, is
//! dropped: the reader received that span at its first read (crosstalk's
//! `flow.correlator.reread-refreshes-delivery`, INV-1122, which the
//! datasets' reread controls encode). Hits on one span in one exchange all
//! count. Direct routes are never rereads.
//!
//! **Grouping.** An exchange's remaining hits become one confirmed
//! transmission per (sender, route key), in that order; its matches keep
//! the hits' order.

use std::collections::BTreeMap;

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::labels::Route;
use a2a_bench_format::predictions::{ContentEvidence, Transmission};

use super::{Hit, Matcher};
use crate::error::ReferenceError;
use crate::predict::{confirmed, transmission_ref};

impl Matcher<'_> {
    /// `hits` (one exchange's, of `reader`) without the channel hits whose
    /// span an earlier exchange already delivered to `reader` through the
    /// same channel; the rest are recorded as delivered.
    pub(super) fn first_reads(&mut self, reader: usize, hits: Vec<Hit>) -> Vec<Hit> {
        let is_channel = |hit: &Hit| matches!(hit.evidence.route, Route::Channel { .. });
        let key = |hit: &Hit| (hit.span, reader, hit.route_key.clone());
        let (fresh, rereads): (Vec<Hit>, Vec<Hit>) = hits
            .into_iter()
            .partition(|hit| !is_channel(hit) || !self.delivered.contains(&key(hit)));
        if !rereads.is_empty() {
            tracing::debug!(reader, rereads = rereads.len(), "reference rereads dropped");
            self.rereads += rereads.len();
        }
        for hit in &fresh {
            if is_channel(hit) {
                self.delivered.insert(key(hit));
            }
        }
        fresh
    }

    /// One confirmed transmission per (sender, route key) among an
    /// exchange's hits.
    pub(super) fn group(
        &self,
        exchange: &Exchange,
        hits: Vec<Hit>,
    ) -> Result<Vec<Transmission>, ReferenceError> {
        let mut groups: BTreeMap<(usize, String), Vec<ContentEvidence>> = BTreeMap::new();
        for hit in hits {
            groups
                .entry((hit.sender, hit.route_key))
                .or_default()
                .push(hit.evidence);
        }
        let mut out = Vec::with_capacity(groups.len());
        for ((sender, route_key), matches) in groups {
            let Some(name) = self.agents.name(sender) else {
                continue;
            };
            if matches.is_empty() {
                continue;
            }
            let id = transmission_ref(exchange.id, name, &route_key)?;
            out.push(confirmed(id, matches)?);
        }
        Ok(out)
    }
}
