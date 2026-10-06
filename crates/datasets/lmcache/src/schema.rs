//! The parts of an LMCache agentic-trace row the converter reads (it also
//! holds `output_length`, which is not projected).

use a2a_bench_corpus::helpers::chat::{ChatMessage, null_as_default};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LmcacheRow {
    /// `<benchmark>__<task…>__<model family>`.
    pub session_id: String,
    pub model: String,
    /// The request's messages: the session's whole history so far.
    #[serde(default, deserialize_with = "null_as_default")]
    pub input: Vec<ChatMessage>,
    /// Seconds since the session's previous request (0 for its first).
    #[serde(default)]
    pub pre_gap: Option<f64>,
}
