//! LMCache traces as a stream of mixed worlds.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_format::ids::DatasetId;

use crate::DATASET;
use crate::calls::mix;
use crate::error::LmcacheError;
use crate::files::{discover, segments};
use crate::options::Options;
use crate::sessions::Sessions;

/// Worlds of `agents_per_world` sessions, named `mix-00000`, `mix-00001`, …
pub struct LmcacheSource {
    dataset: DatasetId,
    root: PathBuf,
    files: Vec<String>,
    options: Options,
    read: FilesRead,
}

impl LmcacheSource {
    /// The files of `options` under `root`; nothing is read until
    /// [`TraceSource::worlds`].
    pub fn open(root: &Path, options: &Options) -> Result<Self, LmcacheError> {
        Ok(Self {
            dataset: DatasetId::new(DATASET).map_err(CorpusError::from)?,
            root: root.to_path_buf(),
            files: discover(root, options.limit, &options.include)?,
            options: options.clone(),
            read: FilesRead::new(),
        })
    }

    /// The selected data files, relative to the root.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// The files read so far (every selected file once worlds are read:
    /// their row groups are listed first), for the source digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.read
    }
}

impl TraceSource for LmcacheSource {
    type Error = LmcacheError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, LmcacheError>> + '_ {
        let size = self.options.agents_per_world.max(1);
        for file in &self.files {
            self.read.record(file.clone());
        }
        // As ct-eval: errors are a stack, the latest first.
        let mut pending: Vec<LmcacheError> = Vec::new();
        let segments = match segments(&self.root, &self.files) {
            Ok(segments) => segments,
            Err(error) => {
                pending.push(error);
                Vec::new()
            }
        };
        let mut sessions = Sessions::new(&self.root, segments, self.options.count);
        let mut number = 0usize;
        std::iter::from_fn(move || {
            if let Some(error) = pending.pop() {
                return Some(Err(error));
            }
            let mut batch = Vec::with_capacity(size);
            while batch.len() < size {
                match sessions.next() {
                    None => break,
                    Some(Ok(session)) => batch.push(session),
                    Some(Err(error)) => pending.push(error),
                }
            }
            if batch.is_empty() {
                return pending.pop().map(Err);
            }
            let key = format!("mix-{number:05}");
            number += 1;
            tracing::debug!(world = %key, sessions = batch.len(), "mixing lmcache world");
            Some(mix(&key, &batch))
        })
    }
}
