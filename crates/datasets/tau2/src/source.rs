//! τ²-bench as a stream of worlds, one per simulation. One results file is
//! parsed at a time.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, WorldKey};

use crate::schema::Results;
use crate::{DATASET, Options, Tau2Error, convert_simulation, files, load_results};

/// The results files under `root` that `options` picks, as worlds.
pub fn source(root: &Path, options: &Options) -> Result<Tau2Source, Tau2Error> {
    Ok(Tau2Source {
        dataset: DatasetId::new(DATASET)?,
        root: root.to_path_buf(),
        files: files::discover(root, options)?,
        limit: options.limit,
        filter: WorldFilter::All,
        files_read: FilesRead::new(),
    })
}

pub struct Tau2Source {
    dataset: DatasetId,
    root: PathBuf,
    files: Vec<PathBuf>,
    limit: Option<usize>,
    filter: WorldFilter,
    files_read: FilesRead,
}

impl Tau2Source {
    /// The results files, relative to the root.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The results files read so far, relative to the root.
    pub fn files_read(&self) -> &FilesRead {
        &self.files_read
    }

    /// Whether the filter may keep a world of results file `relative`.
    /// Without a limit, a file none of whose worlds are kept is never read.
    fn may_keep(&self, relative: &Path) -> bool {
        // World keys are `<file without .json>/<simulation>`.
        let file = relative.to_string_lossy();
        let prefix = format!("{}/", file.trim_end_matches(".json"));
        match &self.filter {
            WorldFilter::Only(keys) => keys.iter().any(|key| key.as_str().starts_with(&prefix)),
            WorldFilter::All | WorldFilter::Except(_) => true,
        }
    }
}

impl TraceSource for Tau2Source {
    type Error = Tau2Error;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    /// Simulations the filter drops are not converted. They still count
    /// toward `limit`, which picks simulations before any filter, as
    /// ct-eval does.
    fn select(&mut self, filter: &WorldFilter) {
        self.filter = filter.clone();
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, Tau2Error>> + '_ {
        Worlds {
            quota: files::quota(self.limit, self.files.len()),
            left: self.limit.unwrap_or(usize::MAX),
            next_file: 0,
            current: None,
            source: self,
        }
    }
}

/// The file being read: its results, its name, the simulations still to
/// convert.
struct Current {
    results: Results,
    file: String,
    picks: std::vec::IntoIter<usize>,
}

/// One pick at a time, files in order, at most `limit` picks (an
/// unreadable file counts as one).
struct Worlds<'a> {
    source: &'a mut Tau2Source,
    quota: Option<usize>,
    left: usize,
    next_file: usize,
    current: Option<Current>,
}

impl Iterator for Worlds<'_> {
    type Item = Result<World, Tau2Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.left > 0 {
            if let Some(current) = &mut self.current {
                let Some(index) = current.picks.next() else {
                    self.current = None;
                    continue;
                };
                self.left -= 1;
                let kept = WorldKey::new(files::world_name(&current.file, index))
                    .map_or(true, |key| self.source.filter.keeps(&key));
                if !kept {
                    continue;
                }
                return Some(convert_simulation(&current.results, &current.file, index));
            }
            let relative = self.source.files.get(self.next_file)?.clone();
            self.next_file += 1;
            if self.quota.is_none() && !self.source.may_keep(&relative) {
                continue;
            }
            self.source.files_read.record(relative.clone());
            match load_results(&self.source.root, &relative) {
                Ok(results) => {
                    let picks = files::pick(results.simulations.len(), self.quota);
                    self.current = Some(Current {
                        results,
                        file: relative.to_string_lossy().into_owned(),
                        picks: picks.into_iter(),
                    });
                }
                Err(error) => {
                    self.left -= 1;
                    return Some(Err(error));
                }
            }
        }
        None
    }
}
