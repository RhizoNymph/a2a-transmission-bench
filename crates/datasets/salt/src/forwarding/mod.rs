//! Which SALT deliveries are forwarding: content the sender relayed from its
//! own tool output rather than wrote.
//!
//! **Rule.** A delivery is forwarding when at least half of its content,
//! after the reference matcher's fold (string escapes undone at any depth,
//! case folded, whitespace collapsed; [`fold::fold`]), is covered by
//! [`FORWARD_K`]-byte shingles that occur in a tool result the sender
//! received before the call that sent it (any tool, any earlier message of
//! the sender's list). Content shorter than one shingle is never
//! forwarding. A pasted `get_log` chunk or `inspect_database` schema is
//! forwarding; a message that quotes a line of a tool result inside its own
//! prose is not.
//!
//! This is the converter's own knowledge (it holds the sender's messages,
//! so it knows which text came back to the sender from its tools), measured
//! the way crosstalk-eval's reference matcher decides what is seen input
//! rather than originated text. The fold and the shingle hash are ported
//! here bit for bit so the tier does not depend on a matcher crate.

pub mod fold;
pub mod shingle;

use std::collections::HashSet;

use fold::fold;
use shingle::{covered, shingles};

/// Shingle length, in folded bytes: the reference matcher's `k`.
pub const FORWARD_K: usize = 24;

/// The share of the content's folded bytes that must be covered:
/// `numerator / denominator`.
pub const FORWARDED_SHARE: (usize, usize) = (1, 2);

/// The shingles of each tool result in one agent's message list, by the
/// result's index in the list.
#[derive(Debug, Default, Clone)]
pub struct ToolOutput {
    results: Vec<(usize, HashSet<u64>)>,
}

impl ToolOutput {
    /// Adds the tool result at `index` of the list.
    pub fn add(&mut self, index: usize, text: &str) {
        let folded = fold(text);
        let hashes = shingles(folded.as_bytes(), FORWARD_K)
            .into_iter()
            .map(|(hash, _)| hash)
            .collect();
        self.results.push((index, hashes));
    }

    fn seen_before(&self, hash: u64, before: usize) -> bool {
        self.results
            .iter()
            .any(|(index, hashes)| *index < before && hashes.contains(&hash))
    }

    /// Whether `content`, sent by the call in message `before`, is
    /// forwarded from the tool results before it (see the module doc).
    pub fn forwards(&self, content: &str, before: usize) -> bool {
        let folded = fold(content);
        let len = folded.len();
        if len < FORWARD_K {
            return false;
        }
        let hits: Vec<usize> = shingles(folded.as_bytes(), FORWARD_K)
            .into_iter()
            .filter(|&(hash, _)| self.seen_before(hash, before))
            .map(|(_, offset)| offset)
            .collect();
        let covered: usize = covered(&hits, FORWARD_K)
            .into_iter()
            .map(|(start, end)| end - start)
            .sum();
        let (numerator, denominator) = FORWARDED_SHARE;
        covered * denominator >= len * numerator
    }
}
