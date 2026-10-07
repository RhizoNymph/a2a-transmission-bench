//! A labelled run as a trace source of one world, and the export it
//! completes: the corpus export writer's four files, with the capture's
//! manifest kept as the export's input view (`capture_manifest`), plus
//! `diagnostics.json`, the typed join-diagnostics table and the bench's
//! labelling provenance.

use std::convert::Infallible;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{
    DigestError, ExportError, Exported, FilesRead, MANIFEST_FILE, ManifestInfo, export,
    write_manifest,
};
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::split::{Release, Selection};
use a2a_bench_corpus::world::World;
use a2a_bench_format::ids::{DatasetId, InvalidKey, WorldKey};
use a2a_bench_format::manifest::Converter;

use serde::Serialize;

use crate::Options;
use crate::capture::{self, CaptureError};
use crate::capture_manifest::{self, CAPTURE_MANIFEST_FILE, CaptureManifestError};
use crate::holdout::{self, HoldoutError};
use crate::label::{DiagnosticsReport, LabelError, Labelled, label};
use crate::truth_file::{self, TruthFile, TruthFileError};

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
    #[error("the capture manifest {path}: {problem}")]
    CaptureManifest {
        path: PathBuf,
        problem: CaptureManifestError,
    },
    #[error(
        "the export's input view differs from the capture's manifest at {paths:?}: the capture is not exactly the labelled run"
    )]
    NotTheCapture { paths: Vec<String> },
    #[error("the holdout export: {0}")]
    Holdout(#[from] HoldoutError),
    #[error("encoding {DIAGNOSTICS_FILE}: {0}")]
    Encode(serde_json::Error),
    #[error("removing {path}: {source}")]
    Remove {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The files a run is labelled from: the truth file and the capture
/// (messages, exchanges and its manifest), each relative to `root` (or
/// absolute).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    pub root: PathBuf,
    pub truth: PathBuf,
    pub messages: PathBuf,
    pub exchanges: PathBuf,
    pub manifest: PathBuf,
}

impl Inputs {
    /// `truth.jsonl`, `messages.jsonl`, `exchanges.jsonl` and
    /// `manifest.json` in `dir`.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            root: dir.to_path_buf(),
            truth: PathBuf::from("truth.jsonl"),
            messages: PathBuf::from("messages.jsonl"),
            exchanges: PathBuf::from("exchanges.jsonl"),
            manifest: PathBuf::from(CAPTURE_MANIFEST_FILE),
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

/// Who labelled the run: the bench build and the truth file, recorded in
/// [`DIAGNOSTICS_FILE`] (the manifest keeps the capture's provenance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Labelling {
    /// The bench's version and commit.
    pub bench: Converter,
    pub truth: TruthRef,
}

/// The truth file as read: its path as given ([`Inputs::truth`]) and the
/// BLAKE3 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TruthRef {
    pub path: String,
    pub blake3: String,
}

/// `path` as the caller gave it (relative to the inputs' root, or
/// absolute), the digest of the file it names at `at`.
fn truth_ref(path: &Path, at: &Path) -> Result<TruthRef, Error> {
    let open = |source| Error::Open {
        path: at.to_path_buf(),
        source,
    };
    let mut file = File::open(at).map_err(open)?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher).map_err(open)?;
    Ok(TruthRef {
        path: path.display().to_string(),
        blake3: hasher.finalize().to_hex().to_string(),
    })
}

/// Labels the run in `inputs` and writes the whole export to `out_dir`
/// (empty or new): `messages.jsonl`, `exchanges.jsonl` (the world's part of
/// the capture), `labels.jsonl`, `manifest.json`, then
/// [`DIAGNOSTICS_FILE`].
///
/// The bench only adds truth: the manifest is the capture's
/// (`inputs.manifest`, an input view) plus the labels digest and each
/// world's label count and notes, so its input view, and the digest the
/// capture's predictions name, are the capture's. A capture manifest that
/// is missing, not an input view, of another dataset, version or world, or
/// cut with other margins than `options` is refused before anything is
/// written; an export whose input view still differs (the capture holds
/// more than the labelled run, or its digests are not its files') is
/// refused and its `manifest.json` removed. `bench` is recorded in the
/// diagnostics as the labeller.
pub fn write_export(
    inputs: &Inputs,
    out_dir: &Path,
    options: &Options,
    bench: Converter,
) -> Result<Exported<Infallible>, Error> {
    write(inputs, out_dir, options, bench, None)
}

/// [`write_export`] for a holdout run and `release`: the same checks
/// against the capture (its input view must equal the capture's), then the
/// manifest is marked as the release's holdout ([`holdout::mark`]: `split:
/// holdout`, `selection.release`, `selection.capture_digest`). The capture
/// must be a dev view. Whether the run is a holdout run (its seed) and the
/// commitment are the caller's.
pub fn write_holdout_export(
    inputs: &Inputs,
    out_dir: &Path,
    options: &Options,
    bench: Converter,
    release: &Release,
) -> Result<Exported<Infallible>, Error> {
    write(inputs, out_dir, options, bench, Some(release))
}

fn write(
    inputs: &Inputs,
    out_dir: &Path,
    options: &Options,
    bench: Converter,
    release: Option<&Release>,
) -> Result<Exported<Infallible>, Error> {
    let mut source = DemoSwarmSource::open(inputs, options)?;
    let manifest_path = inputs.at(&inputs.manifest);
    let in_manifest = |problem| Error::CaptureManifest {
        path: manifest_path.clone(),
        problem,
    };
    let capture = capture_manifest::read(&manifest_path).map_err(in_manifest)?;
    capture_manifest::check(
        &capture,
        &source.dataset,
        source.labelled.world.key(),
        &options.settings(),
    )
    .map_err(in_manifest)?;
    if release.is_some() {
        holdout::check_capture(&capture)?;
    }
    let labelling = Labelling {
        bench,
        truth: truth_ref(&inputs.truth, &inputs.at(&inputs.truth))?,
    };
    let info = ManifestInfo {
        dataset: capture.dataset.clone(),
        dataset_version: capture.dataset_version,
        source: capture.source.clone(),
        converter: capture.converter.clone(),
        selection: capture.selection.clone(),
        pace: capture.pace.clone(),
    };
    let mut exported = export(&mut source, out_dir, info, &Selection::Unsplit)?;
    // The split's own keys are not the capture's: its selection as written.
    exported.manifest.selection = capture.selection.clone();
    exported.manifest.split = capture.split;
    let paths =
        capture_manifest::differences(&exported.manifest, &capture).map_err(Error::Encode)?;
    if !paths.is_empty() {
        let written = out_dir.join(MANIFEST_FILE);
        std::fs::remove_file(&written).map_err(|source| Error::Remove {
            path: written,
            source,
        })?;
        return Err(Error::NotTheCapture { paths });
    }
    if let Some(release) = release {
        holdout::mark(&mut exported.manifest, &capture, release)?;
    }
    write_manifest(out_dir, &exported.manifest)?;
    write_diagnostics(out_dir, source.labelled(), options, &labelling)?;
    Ok(exported)
}

/// What `diagnostics.json` holds: the labelling provenance, then the
/// report.
#[derive(Serialize)]
struct DiagnosticsFile<'a> {
    labelling: &'a Labelling,
    #[serde(flatten)]
    report: DiagnosticsReport<'a>,
}

/// Writes `labelling` and `labelled`'s diagnostics report to
/// `dir/diagnostics.json` (pretty JSON, a final newline).
pub fn write_diagnostics(
    dir: &Path,
    labelled: &Labelled,
    options: &Options,
    labelling: &Labelling,
) -> Result<PathBuf, Error> {
    let path = dir.join(DIAGNOSTICS_FILE);
    let file = DiagnosticsFile {
        labelling,
        report: labelled.report(options.margins()),
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(Error::Encode)?;
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
