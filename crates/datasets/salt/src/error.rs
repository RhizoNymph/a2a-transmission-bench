//! Why a SALT world could not be produced.

use a2a_bench_corpus::clock::ClockError;
use a2a_bench_corpus::world::CorpusError;
use a2a_bench_format::ids::{InvalidKey, MessageId};
use a2a_bench_format::labels::InvalidLabel;
use a2a_bench_format::message::InvalidMessage;

#[derive(Debug, thiserror::Error)]
pub enum SaltError {
    #[error("{root} has no traces/ directory")]
    NoTraces { root: String },
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a SALT trace: {source}")]
    Json {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("a message has unknown role {0:?}")]
    UnknownRole(String),
    #[error("virtual clock: {0}")]
    Clock(#[source] ClockError),
    #[error("message: {0}")]
    Message(#[from] InvalidMessage),
    #[error("key: {0}")]
    Key(#[from] InvalidKey),
    #[error("label: {0}")]
    Label(#[from] InvalidLabel),
    /// A control's place names a message no exchange of the world carries,
    /// so it has no exchange to sit in (crosstalk's golden export refuses
    /// the same world).
    #[error("label {label}: no exchange carries message {message}")]
    UncarriedLocation { label: String, message: MessageId },
    /// Boxed: a corpus error (a failed world check) is large.
    #[error("corpus: {0}")]
    Corpus(#[source] Box<CorpusError>),
}

impl From<CorpusError> for SaltError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
