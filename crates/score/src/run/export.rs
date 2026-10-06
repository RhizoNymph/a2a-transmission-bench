//! Scoring an export directory against a predictions file.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use a2a_bench_format::manifest::Manifest;

use super::{RunError, RunSummary, ScoreOptions, Streams, score_streams};
use crate::run::FileName;

fn open(path: &Path) -> Result<BufReader<File>, RunError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| RunError::Open {
            path: path.display().to_string(),
            source,
        })
}

/// Scores `predictions` against the export in `export` (its
/// `manifest.json`, `messages.jsonl`, `exchanges.jsonl` and
/// `labels.jsonl`). The predictions must name the export's manifest digest
/// (whatever `options.manifest_digest` says) and dataset.
pub fn score_export(
    export: &Path,
    predictions: &Path,
    mut options: ScoreOptions,
) -> Result<RunSummary, RunError> {
    let manifest_path = export.join("manifest.json");
    let manifest_error = |source| RunError::Manifest {
        path: manifest_path.display().to_string(),
        source,
    };
    let manifest: Manifest =
        serde_json::from_reader(open(&manifest_path)?).map_err(manifest_error)?;
    options.manifest_digest = Some(manifest.digest().map_err(manifest_error)?);
    let summary = score_streams(
        Streams {
            messages: open(&export.join("messages.jsonl"))?,
            exchanges: open(&export.join("exchanges.jsonl"))?,
            labels: open(&export.join("labels.jsonl"))?,
            predictions: open(predictions)?,
        },
        options,
    )?;
    if summary.dataset != manifest.dataset {
        return Err(RunError::DatasetMismatch {
            file: FileName::Exchanges,
            expected: manifest.dataset,
            got: summary.dataset,
        });
    }
    Ok(summary)
}
