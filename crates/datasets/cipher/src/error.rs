//! Why the generator could not produce a world.

use a2a_bench_corpus::clock::ClockError;
use a2a_bench_corpus::world::CorpusError;
use a2a_bench_format::location::EmptyRange;

#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{root} holds no payload pool")]
    NoPools { root: String },
    #[error("virtual clock: {0}")]
    Clock(#[source] ClockError),
    #[error("location: {0}")]
    Location(#[from] EmptyRange),
    #[error("the encoding of pair {index} is longer than a location can name")]
    TooLong { index: usize },
    #[error("corpus: {0}")]
    Corpus(Box<CorpusError>),
}

impl From<CorpusError> for CipherError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
