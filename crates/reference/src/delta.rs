//! New inputs: what an exchange's request adds to the agent's previous one.
//!
//! Requests carry the whole history, so most of a request was already read
//! by the agent's previous exchange. The new inputs are the request's
//! messages beyond those, as a multiset: a message whose id occurs `n`
//! times in the previous request (or its response, which the next request
//! echoes) has its first `n` occurrences here counted as old. Comparing by
//! id rather than by prefix keeps a truncated or rewritten history
//! (sliding-window memory, summaries) from counting everything it kept as
//! new.

use std::collections::BTreeMap;

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::ids::MessageId;

/// The messages of `current`'s request that are new relative to
/// `previous`, the same agent's preceding exchange, in request order.
pub fn new_inputs(previous: Option<&Exchange>, current: &Exchange) -> Vec<MessageId> {
    let mut seen: BTreeMap<MessageId, usize> = BTreeMap::new();
    if let Some(previous) = previous {
        for id in previous
            .request
            .messages
            .iter()
            .chain(&previous.response.messages)
        {
            *seen.entry(*id).or_insert(0) += 1;
        }
    }
    current
        .request
        .messages
        .iter()
        .filter(|id| match seen.get_mut(*id) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .copied()
        .collect()
}
