//! SALT-NLP "Emergent Collusion in Long-Horizon LLM Agent Interaction" as
//! a2a-transmission-bench worlds: two agents (Alice and Bob) per trajectory
//! exchanging messages through a harness-mediated channel. 2,650 trace
//! files, 53 conditions × 50 repetitions.
//!
//! One trace file (`traces/<exp>/<cond>/repNNN.json[.gz]`) is one world.
//! Each episode's calls are reconstructed per agent ([`episode`]), turned
//! into exchanges on the virtual clock, and labelled ([`truth`]). The
//! world's coverage is complete at the construction tier: the only way the
//! two agents talk is the logged channel, so a prediction no label explains
//! is a false positive.
//!
//! Ported from crosstalk-eval at 7f8a2fb (`datasets/salt`); version 1
//! reproduces its corpus. See `docs/features/dataset-salt.md`.

pub mod episode;
mod error;
pub mod files;
pub mod forwarding;
pub mod messages;
pub mod need;
pub mod schema;
pub mod truth;
mod world;

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::export::FilesRead;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::manifest::Setting;
use flate2::read::GzDecoder;

pub use error::SaltError;
pub use world::convert_trace;

/// The dataset's id (crosstalk-eval's).
pub const DATASET: &str = "salt";

/// The converter's dataset version: 1 reproduces crosstalk-eval's corpus.
pub const VERSION: u32 = 1;

/// Which trace files to read: crosstalk-eval's `--limit` and `--include`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// At most this many files, in stratified order ([`files::discover`]).
    pub limit: Option<usize>,
    /// Keep only files whose relative path contains one of these (all files
    /// when empty).
    pub include: Vec<String>,
}

impl Options {
    /// The manifest's `selection`: `limit` when set, and `include[i]` per
    /// include, as crosstalk's golden export writes them.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let mut out = BTreeMap::new();
        if let Some(limit) = self.limit {
            out.insert(
                "limit".to_owned(),
                Setting::Int(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
        }
        for (at, include) in self.include.iter().enumerate() {
            out.insert(format!("include[{at}]"), Setting::Text(include.clone()));
        }
        out
    }
}

/// SALT under `root` as a stream of worlds, one per selected trace file,
/// calls `pace` apart.
pub fn source(root: &Path, options: &Options, pace: Pace) -> Result<SaltSource, SaltError> {
    let files = files::discover(root, options)?;
    Ok(SaltSource {
        dataset: DatasetId::new(DATASET)?,
        root: root.to_path_buf(),
        files,
        pace,
        filter: WorldFilter::All,
        read: FilesRead::new(),
    })
}

/// SALT as a stream of worlds, one per trace file.
pub struct SaltSource {
    dataset: DatasetId,
    root: PathBuf,
    files: Vec<PathBuf>,
    pace: Pace,
    filter: WorldFilter,
    read: FilesRead,
}

impl SaltSource {
    /// The selected trace files, relative to the root, in world order.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// The trace files read so far (relative to the root), for the source
    /// digest.
    pub fn files_read(&self) -> &FilesRead {
        &self.read
    }

    pub fn pace(&self) -> Pace {
        self.pace
    }
}

impl TraceSource for SaltSource {
    type Error = SaltError;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    /// World keys are file paths, so files the export will not keep are
    /// never read.
    fn select(&mut self, filter: &WorldFilter) {
        self.filter = filter.clone();
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, SaltError>> + '_ {
        let Self {
            root,
            files,
            pace,
            filter,
            read,
            ..
        } = self;
        let pace = *pace;
        files.iter().filter_map(move |relative| {
            let kept = WorldKey::new(files::world_name(relative))
                .map(|key| filter.keeps(&key))
                .unwrap_or(true);
            if !kept {
                return None;
            }
            read.record(relative.clone());
            Some(load_world_paced(root, relative, pace))
        })
    }
}

/// Reads and converts one trace file, calls [`Pace::DEFAULT`] apart.
pub fn load_world(root: &Path, relative: &Path) -> Result<World, SaltError> {
    load_world_paced(root, relative, Pace::DEFAULT)
}

/// Reads and converts one trace file (relative to `root`), calls `pace`
/// apart.
pub fn load_world_paced(root: &Path, relative: &Path, pace: Pace) -> Result<World, SaltError> {
    let path = root.join(relative);
    let bytes = read(&path)?;
    let trace: schema::Trace =
        serde_json::from_slice(&bytes).map_err(|source| SaltError::Json {
            path: path.display().to_string(),
            source,
        })?;
    convert_trace(&trace, &files::source_file(relative), pace)
}

/// A file's bytes, gunzipped when it ends in `.gz`.
fn read(path: &Path) -> Result<Vec<u8>, SaltError> {
    let io = |source| SaltError::Io {
        path: path.display().to_string(),
        source,
    };
    let raw = fs::read(path).map_err(io)?;
    if path.extension().is_some_and(|ext| ext == "gz") {
        let mut out = Vec::with_capacity(raw.len().saturating_mul(8));
        GzDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .map_err(io)?;
        Ok(out)
    } else {
        Ok(raw)
    }
}
