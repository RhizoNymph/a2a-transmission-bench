//! Why the converter could not produce a world.

use a2a_bench_corpus::clock::ClockError;
use a2a_bench_corpus::helpers::chat::ChatError;
use a2a_bench_corpus::helpers::parquet_rows::ParquetError;
use a2a_bench_corpus::world::CorpusError;

#[derive(Debug, thiserror::Error)]
pub enum OpenSweError {
    #[error("{root} has no data/ directory")]
    NoData { root: String },
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Parquet(#[from] ParquetError),
    #[error("{file} row {row}: {source}")]
    Chat {
        file: String,
        row: usize,
        #[source]
        source: ChatError,
    },
    #[error("virtual clock: {0}")]
    Clock(#[source] ClockError),
    #[error(transparent)]
    Corpus(Box<CorpusError>),
}

impl From<CorpusError> for OpenSweError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}
