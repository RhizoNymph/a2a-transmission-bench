//! Why the generator could not produce a world.

use a2a_bench_corpus::world::CorpusError;
use a2a_bench_dataset_open_swe::OpenSweError;
use a2a_bench_format::location::EmptyRange;
use a2a_bench_format::message::NoPartText;

#[derive(Debug, thiserror::Error)]
pub enum SpliceError {
    #[error(transparent)]
    OpenSwe(Box<OpenSweError>),
    #[error("splice {0}: no trajectory in the pool has a file write to splice")]
    NoWriter(usize),
    #[error("splice {0}: no reader from another repository with a call to splice before")]
    NoReader(usize),
    #[error("splice {number}: the read's location: {source}")]
    Range {
        number: usize,
        #[source]
        source: EmptyRange,
    },
    #[error("splice {number}: the read's text: {source}")]
    Text {
        number: usize,
        #[source]
        source: NoPartText,
    },
    #[error("splice {number}: bytes {start}..{end} are not text of the read's result")]
    OutOfText {
        number: usize,
        start: usize,
        end: usize,
    },
    #[error(transparent)]
    Corpus(Box<CorpusError>),
}

impl From<CorpusError> for SpliceError {
    fn from(error: CorpusError) -> Self {
        Self::Corpus(Box::new(error))
    }
}

impl From<OpenSweError> for SpliceError {
    fn from(error: OpenSweError) -> Self {
        Self::OpenSwe(Box::new(error))
    }
}
