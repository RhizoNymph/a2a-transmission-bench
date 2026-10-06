//! AgentDojo as a stream of worlds, one per run file.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, WorldKey};

use crate::files::{self, RunFile};
use crate::{AgentDojoError, DATASET, Options, Tally, load_world};

/// The runs under `root` that `options` picks, as worlds with calls `pace`
/// apart.
pub fn source(
    root: &Path,
    options: &Options,
    pace: Pace,
) -> Result<AgentDojoSource, AgentDojoError> {
    Ok(AgentDojoSource {
        dataset: DatasetId::new(DATASET)?,
        root: root.to_path_buf(),
        files: files::discover(root, options)?,
        filter: WorldFilter::All,
        tally: Tally::default(),
        files_read: FilesRead::new(),
        pace,
    })
}

/// The run files a selection picked, read one at a time. The tally of
/// every run converted so far is kept for the report, and every file read
/// is recorded for the source digest.
pub struct AgentDojoSource {
    dataset: DatasetId,
    root: PathBuf,
    files: Vec<RunFile>,
    filter: WorldFilter,
    tally: Tally,
    files_read: FilesRead,
    pace: Pace,
}

impl AgentDojoSource {
    pub fn files(&self) -> &[RunFile] {
        &self.files
    }

    /// What the runs converted so far counted.
    pub fn tally(&self) -> Tally {
        self.tally
    }

    /// The run files read so far, relative to the root.
    pub fn files_read(&self) -> &FilesRead {
        &self.files_read
    }

    pub fn pace(&self) -> Pace {
        self.pace
    }

    /// Whether the filter keeps `file`'s world. A key that is not valid is
    /// kept, so its conversion reports why.
    fn keeps(&self, file: &RunFile) -> bool {
        WorldKey::new(files::world_name(&file.relative)).map_or(true, |key| self.filter.keeps(&key))
    }
}

impl TraceSource for AgentDojoSource {
    type Error = AgentDojoError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    /// World keys are run paths, known before parsing: runs the filter
    /// drops are never read.
    fn select(&mut self, filter: &WorldFilter) {
        self.filter = filter.clone();
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, AgentDojoError>> + '_ {
        let files = std::mem::take(&mut self.files);
        let kept: Vec<RunFile> = files
            .iter()
            .filter(|file| self.keeps(file))
            .cloned()
            .collect();
        self.files = files;
        kept.into_iter().map(move |file| {
            self.files_read.record(file.relative.clone());
            let loaded = load_world(&self.root, &file.relative, self.pace)?;
            self.tally.add(&loaded.tally);
            Ok(loaded.world)
        })
    }
}
