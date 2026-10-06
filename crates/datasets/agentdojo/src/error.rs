//! Why a run could not be found or converted.

use a2a_bench_corpus::clock::ClockError;
use a2a_bench_corpus::world::CorpusError;
use a2a_bench_format::ids::InvalidKey;
use a2a_bench_format::labels::InvalidLabel;
use a2a_bench_format::location::EmptyRange;
use a2a_bench_format::message::InvalidMessage;

#[derive(Debug, thiserror::Error)]
pub enum AgentDojoError {
    #[error("{root} has no runs/ directory")]
    NoRuns { root: String },
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not an AgentDojo run: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("a message has unknown role {0:?}")]
    UnknownRole(String),
    #[error("virtual clock: {0}")]
    Clock(#[from] ClockError),
    #[error("location: {0}")]
    Location(#[from] EmptyRange),
    #[error("a location offset does not fit in 32 bits")]
    Offset,
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

impl From<CorpusError> for AgentDojoError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
