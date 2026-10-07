//! `a2a-bench validate <export> [--predictions <file>]`: every format check
//! over an export (or an input view) and, optionally, a predictions file.
//!
//! The files are read in lockstep, one world at a time: framing (header,
//! world order, trailer counts and digest), each world's inputs
//! (`WorldInputs::new`), its labels (`check_labels`) and predictions
//! (`check_predictions`); then the trailers against the manifest's
//! `files`, the manifest's world list (keys, order, exchange and label
//! counts) against the files, and the predictions header's dataset and
//! manifest digest. The report holds counts and ids only: never label or
//! message text (swarm-traces labels are real attack payloads).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{
    EXCHANGES_FILE, ExportError, LABELS_FILE, MESSAGES_FILE, read_manifest,
};
use a2a_bench_format::check::{WorldInputs, check_labels, check_predictions};
use a2a_bench_format::files::{ExchangeRow, Exchanges, Labels, MessageRow, Messages, Predictions};
use a2a_bench_format::ids::{DatasetId, Digest, WorldKey};
use a2a_bench_format::jsonl::{FileKind, FileReader, HeaderFields, Keyed, WorldSection};
use a2a_bench_format::labels::Label;
use a2a_bench_format::manifest::Manifest;
use a2a_bench_format::predictions::{Prediction, WorldStatus};
use clap::Args;

use crate::safe;

#[derive(Debug, Clone, Args)]
pub struct ValidateArgs {
    /// The export directory (or an input view).
    pub export: PathBuf,
    /// A predictions file to check against the export.
    #[arg(long)]
    pub predictions: Option<PathBuf>,
}

/// One failed check: where, and what (ids and counts only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: &'static str,
    pub world: Option<WorldKey>,
    pub problem: String,
}

/// What `validate` read and found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidateReport {
    pub dataset: Option<DatasetId>,
    pub worlds: u64,
    pub messages: u64,
    pub exchanges: u64,
    /// Label rows by kind; `None` for an input view.
    pub labels: Option<BTreeMap<&'static str, u64>>,
    /// Prediction world statuses and rows by kind; `None` without
    /// `--predictions`.
    pub predictions: Option<BTreeMap<&'static str, u64>>,
    pub findings: Vec<Finding>,
}

impl ValidateReport {
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }

    /// Counts, then each finding.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let dataset = self
            .dataset
            .as_ref()
            .map_or_else(|| "?".to_owned(), ToString::to_string);
        let _ = writeln!(
            out,
            "dataset {dataset}: {} worlds, {} messages, {} exchanges",
            self.worlds, self.messages, self.exchanges
        );
        let counts = |map: &BTreeMap<&'static str, u64>| {
            map.iter()
                .map(|(kind, n)| format!("{kind} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match &self.labels {
            Some(labels) => {
                let _ = writeln!(out, "labels: {}", counts(labels));
            }
            None => {
                let _ = writeln!(out, "labels: none (input view)");
            }
        }
        if let Some(predictions) = &self.predictions {
            let _ = writeln!(out, "predictions: {}", counts(predictions));
        }
        if self.passed() {
            let _ = writeln!(out, "valid");
        } else {
            let _ = writeln!(out, "{} problems:", self.findings.len());
            for finding in &self.findings {
                match &finding.world {
                    Some(world) => {
                        let _ =
                            writeln!(out, "  {} world {world}: {}", finding.file, finding.problem);
                    }
                    None => {
                        let _ = writeln!(out, "  {}: {}", finding.file, finding.problem);
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ValidateError {
    #[error(transparent)]
    Manifest(#[from] ExportError),
    #[error("opening {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn label_kind(label: &Label) -> &'static str {
    match label {
        Label::ExchangeAgent(_) => "exchange_agent",
        Label::Transmission(_) => "transmission",
        Label::AccessOnly(_) => "access_only",
        Label::NegativeControl(_) => "negative_control",
        Label::Exemption(_) => "exemption",
        Label::AgentCluster(_) => "agent_cluster",
    }
}

fn prediction_kind(row: &Prediction) -> &'static str {
    match row {
        Prediction::Attribution(_) => "attribution",
        Prediction::Unattributed(_) => "unattributed",
        Prediction::Transmission(_) => "transmission",
    }
}

fn status_kind(status: &WorldStatus) -> &'static str {
    match status {
        WorldStatus::Scored => "worlds_scored",
        WorldStatus::NoConsumers { .. } => "worlds_no_consumers",
        WorldStatus::Failed { .. } => "worlds_failed",
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// One file being read, or the finding that stopped it.
struct Stream<K: FileKind> {
    name: &'static str,
    reader: Option<FileReader<K, BufReader<File>>>,
}

impl<K: FileKind> Stream<K> {
    fn open(
        path: &Path,
        name: &'static str,
        findings: &mut Vec<Finding>,
    ) -> Result<Self, ValidateError> {
        let file = File::open(path).map_err(|source| ValidateError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let reader = match FileReader::open(BufReader::new(file)) {
            Ok(reader) => Some(reader),
            Err(error) => {
                findings.push(Finding {
                    file: name,
                    world: None,
                    problem: safe::read_error(&error),
                });
                None
            }
        };
        Ok(Self { name, reader })
    }

    /// The next world, or `None` at the end or after a read error (which
    /// becomes a finding and stops this file).
    fn next(&mut self, findings: &mut Vec<Finding>) -> Option<WorldSection<K>> {
        let reader = self.reader.as_mut()?;
        match reader.next_world() {
            Ok(section) => section,
            Err(error) => {
                findings.push(Finding {
                    file: self.name,
                    world: None,
                    problem: safe::read_error(&error),
                });
                self.reader = None;
                None
            }
        }
    }

    /// The next world, which must be `key`.
    fn expect(&mut self, key: &WorldKey, findings: &mut Vec<Finding>) -> Option<WorldSection<K>> {
        let alive = self.reader.is_some();
        let section = self.next(findings);
        match &section {
            Some(section) if section.world.key() != key => {
                findings.push(Finding {
                    file: self.name,
                    world: Some(key.clone()),
                    problem: format!("world {} is where {key} should be", section.world.key()),
                });
                self.reader = None;
                None
            }
            None if alive && self.reader.is_some() => {
                findings.push(Finding {
                    file: self.name,
                    world: Some(key.clone()),
                    problem: "the world is missing".to_owned(),
                });
                None
            }
            _ => section,
        }
    }

    /// After the last world: no more worlds, and the trailer's digest.
    fn finish(&mut self, findings: &mut Vec<Finding>) -> Option<Digest> {
        while let Some(extra) = self.next(findings) {
            findings.push(Finding {
                file: self.name,
                world: Some(extra.world.key().clone()),
                problem: "a world the exchanges file does not hold".to_owned(),
            });
        }
        self.reader
            .as_ref()
            .and_then(|reader| reader.trailer())
            .map(|trailer| trailer.digest)
    }

    fn dataset(&self) -> Option<DatasetId> {
        self.reader
            .as_ref()
            .map(|reader| reader.header().dataset().clone())
    }
}

fn digest_check(
    findings: &mut Vec<Finding>,
    file: &'static str,
    manifest: Option<Digest>,
    trailer: Option<Digest>,
) {
    match (manifest, trailer) {
        (Some(manifest), Some(trailer)) if manifest != trailer => findings.push(Finding {
            file,
            world: None,
            problem: format!("the trailer digest {trailer} is not the manifest's {manifest}"),
        }),
        (None, Some(_)) => findings.push(Finding {
            file,
            world: None,
            problem: "the manifest records no digest for this file".to_owned(),
        }),
        _ => {}
    }
}

/// Validates `args.export` (module docs). Problems are findings in the
/// report; an error means a file could not be opened at all.
pub fn validate(args: &ValidateArgs) -> Result<ValidateReport, ValidateError> {
    let manifest: Manifest = read_manifest(&args.export)?;
    let mut findings = Vec::new();
    let mut report = ValidateReport {
        dataset: Some(manifest.dataset.clone()),
        ..ValidateReport::default()
    };
    let dir = &args.export;
    let mut messages =
        Stream::<Messages>::open(&dir.join(MESSAGES_FILE), MESSAGES_FILE, &mut findings)?;
    let mut exchanges =
        Stream::<Exchanges>::open(&dir.join(EXCHANGES_FILE), EXCHANGES_FILE, &mut findings)?;
    let labels_path = dir.join(LABELS_FILE);
    let mut labels = if labels_path.is_file() {
        report.labels = Some(BTreeMap::new());
        Some(Stream::<Labels>::open(
            &labels_path,
            LABELS_FILE,
            &mut findings,
        )?)
    } else {
        if manifest.files.labels.is_some() {
            findings.push(Finding {
                file: LABELS_FILE,
                world: None,
                problem: "the manifest records labels but the file is missing".to_owned(),
            });
        }
        None
    };
    let mut predictions = match &args.predictions {
        Some(path) => {
            report.predictions = Some(BTreeMap::new());
            Some(Stream::<Predictions>::open(
                path,
                "predictions",
                &mut findings,
            )?)
        }
        None => None,
    };
    for (name, dataset) in [
        (MESSAGES_FILE, messages.dataset()),
        (EXCHANGES_FILE, exchanges.dataset()),
        (LABELS_FILE, labels.as_ref().and_then(Stream::dataset)),
        (
            "predictions",
            predictions.as_ref().and_then(Stream::dataset),
        ),
    ] {
        if let Some(dataset) = dataset
            && dataset != manifest.dataset
        {
            findings.push(Finding {
                file: name,
                world: None,
                problem: format!("dataset {dataset}, the manifest's is {}", manifest.dataset),
            });
        }
    }
    if let Some(reader) = predictions.as_ref().and_then(|p| p.reader.as_ref()) {
        let capture = a2a_bench_dataset_demo_swarm::holdout::capture_digest(&manifest);
        if let Err(error) = &capture {
            findings.push(Finding {
                file: "manifest.json",
                world: None,
                problem: error.to_string(),
            });
        }
        let capture = capture.ok().flatten();
        match manifest.digest() {
            Ok(_) if capture == Some(reader.header().manifest_digest) => {}
            Ok(digest) if digest != reader.header().manifest_digest => findings.push(Finding {
                file: "predictions",
                world: None,
                problem: format!(
                    "the header names manifest {}, the export's is {digest}",
                    reader.header().manifest_digest
                ),
            }),
            Ok(_) => {}
            Err(_) => findings.push(Finding {
                file: "manifest.json",
                world: None,
                problem: "the manifest cannot be digested".to_owned(),
            }),
        }
    }

    let mut seen: Vec<(WorldKey, u64, Option<u64>)> = Vec::new();
    while let Some(exchange_section) = exchanges.next(&mut findings) {
        let key = exchange_section.world.key.clone();
        let message_section = messages.expect(&key, &mut findings);
        let label_section = labels.as_mut().and_then(|l| l.expect(&key, &mut findings));
        let prediction_section = predictions
            .as_mut()
            .and_then(|p| p.expect(&key, &mut findings));
        report.worlds += 1;
        report.exchanges += count(exchange_section.rows.len());
        seen.push((
            key.clone(),
            count(exchange_section.rows.len()),
            label_section.as_ref().map(|s| count(s.rows.len())),
        ));
        if let (Some(counts), Some(section)) = (report.labels.as_mut(), &label_section) {
            for row in &section.rows {
                *counts.entry(label_kind(row)).or_insert(0) += 1;
            }
        }
        if let (Some(counts), Some(section)) = (report.predictions.as_mut(), &prediction_section) {
            *counts
                .entry(status_kind(&section.world.status))
                .or_insert(0) += 1;
            for row in &section.rows {
                *counts.entry(prediction_kind(row)).or_insert(0) += 1;
            }
        }
        let Some(message_section) = message_section else {
            continue;
        };
        report.messages += count(message_section.rows.len());
        let inputs = WorldInputs::new(
            &message_section.world.key,
            message_section
                .rows
                .into_iter()
                .map(|MessageRow::Message(message)| message)
                .collect(),
            exchange_section.world,
            exchange_section
                .rows
                .into_iter()
                .map(|ExchangeRow::Exchange(exchange)| exchange)
                .collect(),
        );
        let inputs = match inputs {
            Ok(inputs) => inputs,
            Err(error) => {
                findings.push(Finding {
                    file: EXCHANGES_FILE,
                    world: Some(key),
                    problem: error.to_string(),
                });
                continue;
            }
        };
        if let Some(section) = &label_section
            && let Err(error) = check_labels(&inputs, &section.rows)
        {
            findings.push(Finding {
                file: LABELS_FILE,
                world: Some(key.clone()),
                problem: error.to_string(),
            });
        }
        if let Some(section) = &prediction_section
            && let Err(error) = check_predictions(&inputs, &section.rows)
        {
            findings.push(Finding {
                file: "predictions",
                world: Some(key.clone()),
                problem: error.to_string(),
            });
        }
    }
    let message_digest = messages.finish(&mut findings);
    let exchange_digest = exchanges.finish(&mut findings);
    digest_check(
        &mut findings,
        MESSAGES_FILE,
        Some(manifest.files.messages),
        message_digest,
    );
    digest_check(
        &mut findings,
        EXCHANGES_FILE,
        Some(manifest.files.exchanges),
        exchange_digest,
    );
    if let Some(labels) = labels.as_mut() {
        let digest = labels.finish(&mut findings);
        digest_check(&mut findings, LABELS_FILE, manifest.files.labels, digest);
    }
    if let Some(predictions) = predictions.as_mut() {
        predictions.finish(&mut findings);
    }
    check_world_list(&manifest, &seen, &mut findings);
    report.findings = findings;
    Ok(report)
}

/// The manifest's world entries against the worlds the files hold.
fn check_world_list(
    manifest: &Manifest,
    seen: &[(WorldKey, u64, Option<u64>)],
    findings: &mut Vec<Finding>,
) {
    if manifest.worlds.len() != seen.len() {
        findings.push(Finding {
            file: "manifest.json",
            world: None,
            problem: format!(
                "the manifest lists {} worlds, the files hold {}",
                manifest.worlds.len(),
                seen.len()
            ),
        });
    }
    for (entry, (key, exchanges, labels)) in manifest.worlds.iter().zip(seen) {
        let mut problem = |text: String| {
            findings.push(Finding {
                file: "manifest.json",
                world: Some(key.clone()),
                problem: text,
            });
        };
        if &entry.key != key {
            problem(format!("the manifest lists world {} here", entry.key));
            continue;
        }
        if entry.exchanges != *exchanges {
            problem(format!(
                "the manifest counts {} exchanges, the file holds {exchanges}",
                entry.exchanges
            ));
        }
        if let (Some(listed), Some(held)) = (entry.labels, labels)
            && listed != *held
        {
            problem(format!(
                "the manifest counts {listed} label rows, the file holds {held}"
            ));
        }
    }
}
