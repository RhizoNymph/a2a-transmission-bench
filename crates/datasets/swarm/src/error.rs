//! The converter's errors. None carries payload text: a token is named by
//! its length, and a malformed line by its number, column and category
//! only (a JSON error message could quote the value).

use a2a_bench_corpus::clock::ClockError;
use a2a_bench_corpus::world::CorpusError;
use a2a_bench_format::ids::InvalidKey;
use a2a_bench_format::json::JsonError;
use a2a_bench_format::labels::InvalidLabel;
use a2a_bench_format::location::EmptyRange;
use a2a_bench_format::message::InvalidMessage;

#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    #[error("{root} has no {file}")]
    Missing { root: String, file: String },
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} line {line} column {column} is not a swarm record ({category})")]
    Json {
        path: String,
        line: usize,
        column: usize,
        category: &'static str,
    },
    #[error("virtual clock: {0}")]
    Clock(#[from] ClockError),
    #[error("tool-call arguments are not valid JSON: {0}")]
    Arguments(#[source] JsonError),
    #[error("message: {0}")]
    Message(#[from] InvalidMessage),
    #[error("key: {0}")]
    Key(#[from] InvalidKey),
    #[error("location: {0}")]
    Location(#[from] EmptyRange),
    #[error("a {len}-byte token is longer than a location can address")]
    TooLong { len: usize },
    #[error("label: {0}")]
    Label(#[from] InvalidLabel),
    /// Carries only the token's length: the token is payload text.
    #[error("a planned {len}-byte token no longer decodes")]
    Undecodable { len: usize },
    /// Boxed: the corpus error is large and rare.
    #[error("corpus: {0}")]
    Corpus(Box<CorpusError>),
}

/// The category of a JSON error, without its message.
pub fn category(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        serde_json::error::Category::Io => "io",
        serde_json::error::Category::Syntax => "syntax",
        serde_json::error::Category::Data => "data",
        serde_json::error::Category::Eof => "eof",
    }
}

impl From<CorpusError> for SwarmError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
