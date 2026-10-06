//! `a2a-bench score`: a predictions file against an export.
//!
//! ```text
//! score --export <dir> --predictions <file> --out <dir> [--gates <path>]
//!       [--examples N] [--holdout-release <detector>@<tag>]
//! ```
//!
//! Scores with the bench's resource canonicaliser on labels and predictions
//! alike, evaluates the gates the run's detector name and variant select
//! (`--gates`, then `A2A_BENCH_GATES`, then the repository's `gates/`),
//! writes `report.json` and `report.txt` into `--out` and returns the
//! report; the binary prints the table and exits 2 on a failed gate.
//! swarm-traces is always scored with `--examples 0` (its labels are real
//! attack payloads). A holdout export needs `--holdout-release`, a detector
//! at that tag and an output outside any git repository, and gets an
//! aggregate-only report.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{ExportError, read_manifest};
use a2a_bench_format::files::{DetectorInfo, Predictions};
use a2a_bench_format::jsonl::{FileReader, ReadError};
use a2a_bench_score::report::gates::{GateError, GateSearch, GatesFrom};
use a2a_bench_score::report::{Disclosure, Report, ReportError};
use a2a_bench_score::run::{DEFAULT_EXAMPLES, RunError, ScoreOptions, score_export};
use clap::Args;

use crate::canon::ResourceCanon;
use crate::holdout::{self, HoldoutError};
use crate::safe;

/// The dataset whose labels hold real attack payloads: never any example.
pub const NO_EXAMPLES_DATASET: &str = a2a_bench_dataset_swarm::DATASET;

#[derive(Debug, Clone, Args)]
pub struct ScoreArgs {
    /// The export directory.
    #[arg(long)]
    pub export: PathBuf,
    /// The detector's predictions.jsonl.
    #[arg(long)]
    pub predictions: PathBuf,
    /// Where report.json and report.txt go.
    #[arg(long)]
    pub out: PathBuf,
    /// A gates file or directory (default: A2A_BENCH_GATES, then the
    /// repository's gates/).
    #[arg(long)]
    pub gates: Option<PathBuf>,
    /// How many misses and false positives to keep as examples.
    #[arg(long, default_value_t = DEFAULT_EXAMPLES)]
    pub examples: usize,
    /// The release a holdout export is scored for: <detector>@<tag>.
    #[arg(long)]
    pub holdout_release: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ScoreError {
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error(transparent)]
    Holdout(#[from] HoldoutError),
    #[error("opening {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("reading the predictions header of {path}: {problem}")]
    Header { path: PathBuf, problem: String },
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Gates(#[from] GateError),
    #[error(transparent)]
    Report(#[from] ReportError),
    #[error("creating {path}: {source}")]
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// What to score.
#[derive(Debug, Clone)]
pub struct ScoreRequest<'a> {
    pub export: &'a Path,
    pub predictions: &'a Path,
    pub out: &'a Path,
    pub gates: Option<PathBuf>,
    pub examples: usize,
    pub holdout_release: Option<&'a str>,
}

impl<'a> From<&'a ScoreArgs> for ScoreRequest<'a> {
    fn from(args: &'a ScoreArgs) -> Self {
        Self {
            export: &args.export,
            predictions: &args.predictions,
            out: &args.out,
            gates: args.gates.clone(),
            examples: args.examples,
            holdout_release: args.holdout_release.as_deref(),
        }
    }
}

/// The detector a predictions file names in its header.
pub fn detector_of(path: &Path) -> Result<DetectorInfo, ScoreError> {
    let file = File::open(path).map_err(|source| ScoreError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let reader =
        FileReader::<Predictions, _>::open(BufReader::new(file)).map_err(|error: ReadError| {
            ScoreError::Header {
                path: path.to_path_buf(),
                problem: safe::read_error(&error),
            }
        })?;
    Ok(reader.header().detector.clone())
}

/// Scores, evaluates the gates and writes the report (module docs).
pub fn score(request: &ScoreRequest<'_>) -> Result<Report, ScoreError> {
    let manifest = read_manifest(request.export)?;
    let release = holdout::run_release(&manifest, request.holdout_release)?;
    let disclosure = match &release {
        None => Disclosure::Full,
        Some(release) => {
            holdout::tagged_detector(release, &detector_of(request.predictions)?)?;
            holdout::outside_repository(request.out)?;
            Disclosure::Holdout
        }
    };
    let examples = if manifest.dataset.as_str() == NO_EXAMPLES_DATASET {
        0
    } else {
        request.examples
    };
    let summary = score_export(
        request.export,
        request.predictions,
        ScoreOptions {
            example_cap: examples,
            canonicalizer: Box::new(ResourceCanon),
            ..ScoreOptions::default()
        },
    )?;
    let (gates, location) = GateSearch::from_env(request.gates.clone()).load()?;
    match &location {
        Some(location) => tracing::info!(
            gates = %location.path.display(),
            from = match location.from {
                GatesFrom::Flag => "--gates",
                GatesFrom::Env => "A2A_BENCH_GATES",
                GatesFrom::Repo => "repository",
            },
            "gates loaded"
        ),
        None => tracing::info!("no gates"),
    }
    let outcomes = gates
        .for_run(&summary.detector.name, &summary.detector.variant)
        .evaluate(&summary.score);
    let report = Report::new(summary, outcomes, disclosure);
    std::fs::create_dir_all(request.out).map_err(|source| ScoreError::Create {
        path: request.out.to_path_buf(),
        source,
    })?;
    report.write(request.out)?;
    tracing::info!(
        dataset = %report.dataset,
        detector = %report.detector.name,
        variant = %report.detector.variant,
        disclosure = ?report.disclosure,
        recall = ?report.overall.recall,
        precision = ?report.overall.precision,
        gates_failed = report.gates_failed(),
        out = %request.out.display(),
        "scored"
    );
    Ok(report)
}
