//! The converter's errors.

use a2a_bench_corpus::clock::ClockError;
use a2a_bench_corpus::world::CorpusError;
use a2a_bench_format::ids::InvalidKey;
use a2a_bench_format::json::JsonError;
use a2a_bench_format::labels::InvalidLabel;
use a2a_bench_format::location::EmptyRange;
use a2a_bench_format::message::InvalidMessage;

use crate::attribution::AttributionError;

#[derive(Debug, thiserror::Error)]
pub enum WikiError {
    #[error("{root} has no {file}")]
    Missing { root: String, file: String },
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} line {line} is not a wiki record: {source}")]
    Json {
        path: String,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("attribution: {0}")]
    Attribution(#[from] AttributionError),
    #[error("virtual clock: {0}")]
    Clock(#[from] ClockError),
    #[error("tool-call arguments are not valid JSON: {0}")]
    Arguments(#[source] JsonError),
    #[error("message: {0}")]
    Message(#[from] InvalidMessage),
    #[error("key: {0}")]
    Key(#[from] InvalidKey),
    /// Boxed: the corpus error is large and rare.
    #[error("corpus: {0}")]
    Corpus(Box<CorpusError>),
    #[error("label: {0}")]
    Label(#[from] InvalidLabel),
    #[error("location: {0}")]
    Location(#[from] EmptyRange),
    #[error("revision {0} is not in its world's page index")]
    UnplannedRevision(String),
}

impl From<CorpusError> for WikiError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
