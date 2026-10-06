//! Splices as a stream of worlds.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_dataset_open_swe::{RoundRobin, Shard, discover};
use a2a_bench_format::ids::DatasetId;

use crate::DATASET;
use crate::error::SpliceError;
use crate::options::Options;
use crate::plan::plan;
use crate::pool::Pooled;
use crate::world::world;

/// `count` splice worlds over a pool read from the selected shards.
pub struct SpliceSource {
    dataset: DatasetId,
    root: PathBuf,
    shards: Vec<Shard>,
    options: Options,
    pace: Pace,
    files: FilesRead,
}

impl SpliceSource {
    /// The shards of `options` under `root`; nothing is read until
    /// [`SpliceSource::pool`] or [`TraceSource::worlds`].
    pub fn open(root: &Path, options: &Options, pace: Pace) -> Result<Self, SpliceError> {
        Ok(Self {
            dataset: DatasetId::new(DATASET).map_err(CorpusError::from)?,
            root: root.to_path_buf(),
            shards: discover(root, options.limit, &options.include)?,
            options: options.clone(),
            pace,
            files: FilesRead::new(),
        })
    }

    pub fn shards(&self) -> &[Shard] {
        &self.shards
    }

    /// Reads the pool: [`Options::pool_size`] trajectories round-robin
    /// over the shards (at most `ceil(pool_size / shards)` from each). A
    /// row that fails to read fails the pool.
    pub fn pool(&mut self) -> Result<Vec<Pooled>, SpliceError> {
        let size = self.options.pool_size();
        let per_shard = size.div_ceil(self.shards.len().max(1));
        let mut rows = RoundRobin::new(&self.root, self.shards.clone(), Some(per_shard));
        let mut pool = Vec::with_capacity(size);
        let mut read = || -> Result<(), SpliceError> {
            while let Some(row) = rows.next_row() {
                let (shard, row, record) = row?;
                pool.push(Pooled::new(shard, row, record));
            }
            Ok(())
        };
        let outcome = read();
        for opened in rows.take_opened() {
            self.files.record(opened);
        }
        outcome.map(|()| pool)
    }

    /// The shards read so far (relative to the root), for the source
    /// digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }
}

impl TraceSource for SpliceSource {
    type Error = SpliceError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, SpliceError>> + '_ {
        let (pool, failure) = match self.pool() {
            Ok(pool) => (pool, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let seed = self.options.seed;
        let pace = self.pace;
        let splices = if failure.is_some() {
            0
        } else {
            self.options.count
        };
        failure
            .into_iter()
            .map(Err)
            .chain((0..splices).map(move |number| {
                tracing::debug!(splice = number, "planting splice");
                plan(&pool, number, seed).and_then(|plan| world(&pool, &plan, pace))
            }))
    }
}
