//! Why a results file could not be found or a simulation converted.

use a2a_bench_corpus::world::CorpusError;
use a2a_bench_format::ids::InvalidKey;
use a2a_bench_format::labels::InvalidLabel;
use a2a_bench_format::location::EmptyRange;
use a2a_bench_format::message::InvalidMessage;

use crate::time::TimeError;

#[derive(Debug, thiserror::Error)]
pub enum Tau2Error {
    #[error("{root} is not a directory of results files")]
    NoResults { root: String },
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a τ²-bench results file: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("simulation {0} does not exist")]
    NoSimulation(usize),
    #[error("simulation {simulation} names unknown task {task:?}")]
    UnknownTask { simulation: usize, task: String },
    #[error("a message has unknown role {0:?}")]
    UnknownRole(String),
    #[error("message {0} is a model call without a timestamp")]
    MissingTime(usize),
    #[error("time: {0}")]
    Time(#[from] TimeError),
    #[error("message {0}'s time overflows")]
    TimeOverflow(usize),
    #[error("location: {0}")]
    Location(#[from] EmptyRange),
    #[error("a location offset does not fit in 32 bits")]
    Offset,
    #[error("message {0}'s text is gone from its view")]
    NoText(usize),
    #[error("key: {0}")]
    Key(#[from] InvalidKey),
    #[error("message: {0}")]
    Message(#[from] InvalidMessage),
    #[error("label: {0}")]
    Label(#[from] InvalidLabel),
    /// Boxed: the corpus error is large, and this error is returned a lot.
    #[error("corpus: {0}")]
    Corpus(Box<CorpusError>),
}

impl From<CorpusError> for Tau2Error {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
