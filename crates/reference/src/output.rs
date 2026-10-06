//! What the reference reports for one world.

use a2a_bench_format::predictions::Prediction;
use serde::Serialize;

/// One world's predictions and the counts beside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldOutput {
    /// Attribution rows (one per credential, by name), then confirmed
    /// transmissions in reader-exchange order.
    pub predictions: Vec<Prediction>,
    pub summary: WorldSummary,
}

/// Counts the predictions file has no place for; the binary logs them per
/// world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct WorldSummary {
    /// Detector agents: distinct credentials.
    pub agents: usize,
    pub exchanges: usize,
    /// Originated spans indexed.
    pub spans: usize,
    /// Hits made, rereads included.
    pub matches: usize,
    /// Channel hits dropped as rereads.
    pub rereads: usize,
    /// Hits only two string levels undone explain: out of the spec's reach,
    /// so reported as no match.
    pub out_of_reach: usize,
    pub transmissions: usize,
}
