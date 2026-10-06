//! `a2a-bench run`: input view, detector process, score.
//!
//! ```text
//! run --export <dir> --detector-cmd "<program> [args…]" --out <dir>
//!     [--gates <path>] [--examples N] [--holdout-release <detector>@<tag>] [--twice]
//! ```
//!
//! 1. `<out>/input` is the export's input view (never `labels.jsonl`).
//! 2. The detector runs as `<program> --input <out>/input --output
//!    <out>/predictions.jsonl [args…]` (the detector contract, design §4);
//!    its stdout goes to the bench's stderr. A non-zero exit fails the run.
//! 3. With `--twice` it runs again into `<out>/predictions.second.jsonl`,
//!    and the run fails if the two files differ (the determinism check).
//! 4. The predictions are scored as `a2a-bench score` does.
//!
//! A holdout export needs `--holdout-release` and an output outside any git
//! repository, checked before the detector runs.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use a2a_bench_corpus::export::{ExportError, LABELS_FILE, input_view, read_manifest};
use a2a_bench_format::ids::Digest;
use a2a_bench_score::report::Report;
use a2a_bench_score::run::DEFAULT_EXAMPLES;
use clap::Args;

use super::score::{ScoreError, ScoreRequest, score};
use crate::holdout::{self, HoldoutError};

/// The input view's directory under `--out`.
pub const INPUT_DIR: &str = "input";
/// The predictions file under `--out`.
pub const PREDICTIONS_FILE: &str = "predictions.jsonl";
/// The second run's predictions under `--out` (`--twice`).
pub const SECOND_PREDICTIONS_FILE: &str = "predictions.second.jsonl";

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    /// The export directory.
    #[arg(long)]
    pub export: PathBuf,
    /// The detector's program and its own arguments, split like a shell
    /// would (quotes respected); the bench adds --input and --output.
    #[arg(long)]
    pub detector_cmd: String,
    /// The run directory: input view, predictions, reports.
    #[arg(long)]
    pub out: PathBuf,
    /// A gates file or directory.
    #[arg(long)]
    pub gates: Option<PathBuf>,
    /// How many misses and false positives to keep as examples.
    #[arg(long, default_value_t = DEFAULT_EXAMPLES)]
    pub examples: usize,
    /// The release a holdout export is run for: <detector>@<tag>.
    #[arg(long)]
    pub holdout_release: Option<String>,
    /// Run the detector twice and fail if the predictions differ.
    #[arg(long)]
    pub twice: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RunCommandError {
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error(transparent)]
    Holdout(#[from] HoldoutError),
    #[error("--detector-cmd is empty or not valid shell words")]
    DetectorCmd,
    #[error("the input view holds {LABELS_FILE}")]
    LabelsInView,
    #[error("starting the detector {program}: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("the detector {program} failed: {status}")]
    Detector { program: String, status: ExitStatus },
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "the detector is not deterministic: two runs wrote different predictions ({first} and {second})"
    )]
    NotDeterministic { first: Digest, second: Digest },
    #[error(transparent)]
    Score(#[from] ScoreError),
}

/// A finished run.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub report: Report,
    pub predictions: PathBuf,
    /// The predictions' BLAKE3, with `--twice` (both runs wrote it).
    pub twice: Option<Digest>,
}

fn file_digest(path: &Path) -> Result<Digest, RunCommandError> {
    let bytes = std::fs::read(path).map_err(|source| RunCommandError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Digest::from_bytes(*blake3::hash(&bytes).as_bytes()))
}

/// Runs the detector once, writing `output`.
fn detect(argv: &[String], input: &Path, output: &Path) -> Result<(), RunCommandError> {
    let (program, rest) = argv.split_first().ok_or(RunCommandError::DetectorCmd)?;
    tracing::info!(detector = %program, input = %input.display(), output = %output.display(), "running the detector");
    let status = Command::new(program)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .status()
        .map_err(|source| RunCommandError::Spawn {
            program: program.clone(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(RunCommandError::Detector {
            program: program.clone(),
            status,
        })
    }
}

/// Runs `run` (module docs).
pub fn run(args: &RunArgs) -> Result<RunOutcome, RunCommandError> {
    let manifest = read_manifest(&args.export)?;
    if holdout::run_release(&manifest, args.holdout_release.as_deref())?.is_some() {
        holdout::outside_repository(&args.out)?;
    }
    let argv = shlex::split(&args.detector_cmd)
        .filter(|argv| !argv.is_empty())
        .ok_or(RunCommandError::DetectorCmd)?;
    let input = args.out.join(INPUT_DIR);
    input_view(&args.export, &input)?;
    if input.join(LABELS_FILE).exists() {
        return Err(RunCommandError::LabelsInView);
    }
    let predictions = args.out.join(PREDICTIONS_FILE);
    detect(&argv, &input, &predictions)?;
    let twice = if args.twice {
        let second = args.out.join(SECOND_PREDICTIONS_FILE);
        detect(&argv, &input, &second)?;
        let (first, second) = (file_digest(&predictions)?, file_digest(&second)?);
        if first != second {
            return Err(RunCommandError::NotDeterministic { first, second });
        }
        tracing::info!(digest = %first, "two detector runs wrote identical predictions");
        Some(first)
    } else {
        None
    };
    let report = score(&ScoreRequest {
        export: &args.export,
        predictions: &predictions,
        out: &args.out,
        gates: args.gates.clone(),
        examples: args.examples,
        holdout_release: args.holdout_release.as_deref(),
    })?;
    Ok(RunOutcome {
        report,
        predictions,
        twice,
    })
}
