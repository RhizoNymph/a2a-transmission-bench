//! The parts of an Open-SWE-Traces row the converter reads. The shards also
//! hold `tools` (JSON tool definitions), `resolved`, `license`, `language`
//! and `metadata`; they are not projected.

use a2a_bench_corpus::helpers::chat::{ChatMessage, null_as_default};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSweRow {
    pub instance_id: String,
    /// `owner/repo`.
    pub repo: String,
    pub trajectory_id: String,
    /// The whole trajectory, in OpenAI chat format.
    #[serde(default, deserialize_with = "null_as_default")]
    pub messages: Vec<ChatMessage>,
}
