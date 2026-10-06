//! A labelled run as a trace source of one world, and the export it
//! completes: the corpus export writer's four files plus
//! `diagnostics.json`, the typed join-diagnostics table.

use std::convert::Infallible;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{
    DigestError, ExportError, Exported, FilesRead, ManifestInfo, export,
};
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::split::Selection;
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, InvalidKey, WorldKey};
use a2a_bench_format::manifest::{Converter, Source};

use crate::capture::{self, CaptureError};
use crate::label::{LabelError, Labelled, label};
use crate::truth_file::{self, TruthFile, TruthFileError};
use crate::{Options, VERSION};

/// The file the diagnostics report is written to, beside the labels.
pub const DIAGNOSTICS_FILE: &str = "diagnostics.json";

/// Why a run could not be labelled or exported.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("opening {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("truth file {path}: {source}")]
    Truth {
        path: PathBuf,
        source: TruthFileError,
    },
    #[error("the truth header names no valid dataset or world: {0}")]
    Key(#[from] InvalidKey),
    #[error(transparent)]
    Capture(#[from] CaptureError),
    #[error(transparent)]
    Label(#[from] LabelError),
    #[error("the source digest: {0}")]
    Digest(#[from] DigestError),
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error("encoding {DIAGNOSTICS_FILE}: {0}")]
    Encode(serde_json::Error),
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The files a run is labelled from: the truth file and the capture, each
/// relative to `root` (or absolute under it), which the source digest is
/// taken over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    pub root: PathBuf,
    pub truth: PathBuf,
    pub messages: PathBuf,
    pub exchanges: PathBuf,
}

impl Inputs {
    /// `truth.jsonl`, `messages.jsonl` and `exchanges.jsonl` in `dir`.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            root: dir.to_path_buf(),
            truth: PathBuf::from("truth.jsonl"),
            messages: PathBuf::from("messages.jsonl"),
            exchanges: PathBuf::from("exchanges.jsonl"),
        }
    }

    fn at(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }
}

/// Reads the truth file at `path`.
pub fn read_truth(path: &Path) -> Result<TruthFile, Error> {
    let file = File::open(path).map_err(|source| Error::Open {
        path: path.to_path_buf(),
        source,
    })?;
    truth_file::read(BufReader::new(file)).map_err(|source| Error::Truth {
        path: path.to_path_buf(),
        source,
    })
}

/// One labelled run, yielded as one world.
#[derive(Debug)]
pub struct DemoSwarmSource {
    dataset: DatasetId,
    labelled: Labelled,
    run: String,
    files: FilesRead,
    yielded: bool,
}

impl DemoSwarmSource {
    /// Reads the truth and the capture and labels the run.
    pub fn open(inputs: &Inputs, options: &Options) -> Result<Self, Error> {
        let truth_path = inputs.at(&inputs.truth);
        let truth = read_truth(&truth_path)?;
        let dataset = truth.header.scenario().dataset()?;
        let world = WorldKey::new(truth.header.world.clone())?;
        let messages = inputs.at(&inputs.messages);
        let exchanges = inputs.at(&inputs.exchanges);
        let captured = capture::read(&messages, &exchanges, &dataset, &world)?;
        let name = truth_path.file_name().map_or_else(
            || "truth.jsonl".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let labelled = label(&truth, &name, captured, options)?;
        let mut files = FilesRead::new();
        for path in [truth_path, messages, exchanges] {
            files.record(path);
        }
        Ok(Self {
            dataset,
            labelled,
            run: truth.header.run.clone(),
            files,
            yielded: false,
        })
    }

    /// The labelled run: the world and its diagnostics.
    pub fn labelled(&self) -> &Labelled {
        &self.labelled
    }

    /// The labelled run, owned.
    pub fn into_labelled(self) -> Labelled {
        self.labelled
    }

    /// The files read (the truth and the capture).
    pub fn files_read(&self) -> &FilesRead {
        &self.files
    }

    /// The swarm run's id (the truth header's `run`), recorded as the
    /// source revision.
    pub fn run(&self) -> &str {
        &self.run
    }
}

impl TraceSource for DemoSwarmSource {
    type Error = Infallible;

    fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    fn worlds(&mut self) -> impl Iterator<Item = Result<World, Infallible>> + '_ {
        let first = !std::mem::replace(&mut self.yielded, true);
        first.then(|| Ok(self.labelled.world.clone())).into_iter()
    }
}

/// Labels the run in `inputs` and writes the whole export to `out_dir`
/// (empty or new): `messages.jsonl`, `exchanges.jsonl` (the world's part of
/// the capture), `labels.jsonl`, `manifest.json`, then
/// [`DIAGNOSTICS_FILE`]. `source_path` is the run's directory as the
/// manifest records it; the revision is the run id.
pub fn write_export(
    inputs: &Inputs,
    out_dir: &Path,
    options: &Options,
    converter: Converter,
    source_path: &str,
) -> Result<Exported<Infallible>, Error> {
    let mut source = DemoSwarmSource::open(inputs, options)?;
    let info = ManifestInfo {
        dataset: source.dataset.clone(),
        dataset_version: VERSION,
        source: Source {
            path: source_path.to_owned(),
            revision: source.run.clone(),
            digest: source.files.digest(&inputs.root)?,
        },
        converter,
        selection: options.settings(),
        pace: std::collections::BTreeMap::new(),
    };
    let exported = export(&mut source, out_dir, info, &Selection::Unsplit)?;
    write_diagnostics(out_dir, source.labelled(), options)?;
    Ok(exported)
}

/// Writes `labelled`'s diagnostics report to `dir/diagnostics.json`
/// (pretty JSON, a final newline).
pub fn write_diagnostics(
    dir: &Path,
    labelled: &Labelled,
    options: &Options,
) -> Result<PathBuf, Error> {
    let path = dir.join(DIAGNOSTICS_FILE);
    let mut bytes =
        serde_json::to_vec_pretty(&labelled.report(options.margins())).map_err(Error::Encode)?;
    bytes.push(b'\n');
    let write = |source| Error::Write {
        path: path.clone(),
        source,
    };
    let mut file = File::create(&path).map_err(write)?;
    file.write_all(&bytes)
        .and_then(|()| file.flush())
        .map_err(write)?;
    Ok(path)
}
