//! Open-SWE as a stream of mixed worlds.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_format::ids::DatasetId;

use crate::DATASET;
use crate::error::OpenSweError;
use crate::files::{Shard, discover};
use crate::options::Options;
use crate::rows::RoundRobin;
use crate::trajectory::mix;

/// Worlds of `agents_per_world` trajectories, named `mix-00000`,
/// `mix-00001`, …, rows taken from the shards in turn.
pub struct OpenSweSource {
    dataset: DatasetId,
    root: PathBuf,
    shards: Vec<Shard>,
    options: Options,
    pace: Pace,
    files: FilesRead,
}

impl OpenSweSource {
    /// The shards of `options` under `root`; nothing is read until
    /// [`TraceSource::worlds`].
    pub fn open(root: &Path, options: &Options, pace: Pace) -> Result<Self, OpenSweError> {
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

    /// The shards opened so far (relative to the root), for the source
    /// digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }
}

impl TraceSource for OpenSweSource {
    type Error = OpenSweError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, OpenSweError>> + '_ {
        let size = self.options.agents_per_world.max(1);
        let mut rows = RoundRobin::new(&self.root, self.shards.clone(), self.options.count);
        let mut pending: VecDeque<OpenSweError> = VecDeque::new();
        let mut number = 0usize;
        let pace = self.pace;
        let files = &mut self.files;
        std::iter::from_fn(move || {
            if let Some(error) = pending.pop_front() {
                return Some(Err(error));
            }
            let mut batch = Vec::with_capacity(size);
            while batch.len() < size {
                match rows.next_row() {
                    None => break,
                    Some(Ok(row)) => batch.push(row),
                    Some(Err(error)) => pending.push_back(error),
                }
            }
            for opened in rows.take_opened() {
                files.record(opened);
            }
            if batch.is_empty() {
                return pending.pop_front().map(Err);
            }
            let key = format!("mix-{number:05}");
            number += 1;
            tracing::debug!(world = %key, trajectories = batch.len(), "mixing open-swe world");
            Some(mix(&key, &batch, pace))
        })
    }
}
