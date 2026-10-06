//! The cipher corpus as a stream of worlds.

use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_format::ids::DatasetId;

use crate::DATASET;
use crate::codec::CipherKind;
use crate::error::CipherError;
use crate::options::Options;
use crate::pair::Pair;
use crate::pools::{Pool, load};
use crate::world::world;

/// `count` pair worlds per cipher kind, kinds in [`CipherKind::ALL`] order.
pub struct CipherSource {
    dataset: DatasetId,
    pools: Vec<Pool>,
    kinds: Vec<CipherKind>,
    pairs: usize,
    seed: u64,
    pace: Pace,
    files: FilesRead,
}

impl CipherSource {
    /// The pools of `options` under `root` (read now).
    pub fn open(root: &Path, options: &Options, pace: Pace) -> Result<Self, CipherError> {
        let (pools, read) = load(root, options.limit, &options.include)?;
        let mut source = Self::new(pools, options.count, options.seed)?.with_pace(pace);
        for file in read {
            source.files.record(file);
        }
        Ok(source)
    }

    /// Pairs over `pools`, `pairs` per cipher, the default pace.
    pub fn new(pools: Vec<Pool>, pairs: usize, seed: u64) -> Result<Self, CipherError> {
        Ok(Self {
            dataset: DatasetId::new(DATASET).map_err(CorpusError::from)?,
            pools,
            kinds: CipherKind::ALL.to_vec(),
            pairs,
            seed,
            pace: Pace::DEFAULT,
            files: FilesRead::new(),
        })
    }

    /// These worlds with calls `pace` apart.
    pub fn with_pace(mut self, pace: Pace) -> Self {
        self.pace = pace;
        self
    }

    /// Only these cipher kinds.
    pub fn with_kinds(mut self, kinds: Vec<CipherKind>) -> Self {
        self.kinds = kinds;
        self
    }

    /// Every planned pair, in world order.
    pub fn plan(&self) -> Vec<Pair> {
        self.kinds
            .iter()
            .flat_map(|kind| {
                (0..self.pairs).filter_map(|index| Pair::plan(*kind, index, &self.pools, self.seed))
            })
            .collect()
    }

    /// The pool files read (relative to the root), for the source digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }
}

impl TraceSource for CipherSource {
    type Error = CipherError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, CipherError>> + '_ {
        let pace = self.pace;
        self.plan().into_iter().map(move |pair| world(&pair, pace))
    }
}
