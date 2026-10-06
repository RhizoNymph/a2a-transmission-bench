//! Reports: the score as JSON (`report.json`) and as a table, with gate
//! outcomes. Field names and structure are crosstalk-eval's wherever they
//! still apply, so the parity check can compare counts row by row.
//!
//! A [`Disclosure::Holdout`] report is aggregate only (design §9.2): the
//! overall, per-tier and per-route rows, the transmission and violation
//! rows and the gate outcomes, and no per-label, per-world or example
//! output (no misses, false-positive examples, source texts, failed world
//! names or unknown-agent transmissions).

pub mod gates;
pub mod table;

use std::path::Path;

use serde::{Deserialize, Serialize};

use a2a_bench_format::files::DetectorInfo;
use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::labels::Tier;

use crate::class::EvidenceClass;
use crate::run::{FailedWorld, RunSummary, UnknownDetected, Unscored};
use crate::score::{
    AccessOnlyControlRow, Counts, FalsePositive, Miss, RowKey, Selector, SourceCount, Totals,
    TransmissionRow, ViolationRow, rows::ratio,
};
pub use gates::{GateOutcome, GateStatus};

/// The process exit code of a run with a failed gate (ct-eval's).
pub const GATE_FAILURE_EXIT: u8 = 2;

/// How much a report may say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    /// Everything: rows, examples, sources, failed worlds.
    #[default]
    Full,
    /// Aggregates only, for a holdout run.
    Holdout,
}

/// One row with its derived rates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRow {
    #[serde(flatten)]
    pub key: RowKey,
    #[serde(flatten)]
    pub counts: Counts,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

/// Counts summed over rows, with their rates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    #[serde(flatten)]
    pub counts: Counts,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
}

impl Summary {
    pub fn of(counts: Counts) -> Self {
        Self {
            precision: counts.precision(),
            recall: counts.recall(),
            counts,
        }
    }
}

/// What access evidence (suspected or discarded predictions) found.
///
/// - `labels` of `expected`: in-reach content labels that only a suspected
///   or discarded prediction aligned with. They are missed in `overall`.
/// - `found_access` of `expected_access`: in-reach access-only labels,
///   which only access evidence finds and `overall` never counts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AccessOnly {
    /// Missed content labels a suspected or discarded prediction aligned
    /// with.
    pub labels: u64,
    /// Every in-reach content label (`overall`'s expected).
    pub expected: u64,
    /// `labels / expected`; `None` with no label.
    pub recall: Option<f64>,
    /// In-reach access-only labels.
    pub expected_access: u64,
    /// Access-only labels a suspected or discarded prediction found.
    pub found_access: u64,
    /// `found_access / expected_access`; `None` with no such label.
    pub access_recall: Option<f64>,
}

impl AccessOnly {
    /// From `overall` (content rows) and `access` (the in-reach rows of
    /// access-only labels).
    pub fn of(overall: &Counts, access: &Counts) -> Self {
        Self {
            labels: overall.suspected,
            expected: overall.expected,
            recall: ratio(overall.suspected, overall.expected),
            expected_access: access.expected,
            found_access: access.found,
            access_recall: access.recall(),
        }
    }
}

/// What a run's negative controls say: how often the detector reported a
/// transmission per exchange it read, and the shared texts it fell on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Background {
    /// Every content false positive, out-of-reach and forwarding rows
    /// included.
    pub false_positives: u64,
    pub exchanges: u64,
    pub per_1k_exchanges: f64,
    /// Left out of a holdout report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceCount>>,
}

/// What only a full report holds: per-label, per-world and example output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    /// Worlds that could not be scored, with why.
    pub failures: Vec<FailedWorld>,
    /// Transmissions left out for naming a detector agent with no true
    /// agent.
    pub unknown_detected_agents: Vec<UnknownDetected>,
    pub misses: Vec<Miss>,
    pub false_positives: Vec<FalsePositive>,
}

/// Everything a run produced, ready to write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub dataset: DatasetId,
    pub detector: DetectorInfo,
    pub disclosure: Disclosure,
    pub totals: Totals,
    /// Every content row but the out-of-reach and forwarding ones.
    pub overall: Summary,
    /// Rows of out-of-reach labels: expected, but missed by design.
    pub out_of_reach: Summary,
    /// Rows of forwarding labels: the sender relayed its own tool output.
    /// Kept apart from `overall`.
    pub forwarding: Summary,
    /// Labels only access evidence lines up with, kept apart from
    /// `overall`.
    pub access_only: AccessOnly,
    pub rows: Vec<ReportRow>,
    pub transmissions: Vec<TransmissionRow>,
    /// Negative-control violations by content-class predictions: what the
    /// violation gates check.
    pub violations: Vec<ViolationRow>,
    /// Access-only predictions under a negative control, by class: reported
    /// apart, never gated.
    pub access_only_under_controls: Vec<AccessOnlyControlRow>,
    pub gates: Vec<GateOutcome>,
    /// How many worlds failed (`failures` names them in a full report).
    pub failed_worlds: u64,
    /// Worlds the detector took in but could not detect in.
    pub unscored: Unscored,
    /// How many transmissions were left out for naming a detector agent
    /// with no true agent (ct-eval's `unknown_detected_agent`).
    pub unknown_detected_agent: u64,
    /// The false-positive rate and its sources, when the run had negative
    /// controls.
    pub background: Option<Background>,
    /// Absent from a holdout report.
    #[serde(flatten)]
    pub detail: Option<Detail>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("encoding the report: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("writing {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

impl Report {
    pub fn new(summary: RunSummary, gates: Vec<GateOutcome>, disclosure: Disclosure) -> Self {
        let RunSummary {
            dataset,
            detector,
            score,
            failures,
            unscored,
            unknown_detected_agents,
        } = summary;
        let content = Selector::default();
        let mut overall = Counts::default();
        let mut out_of_reach = Counts::default();
        let mut forwarding = Counts::default();
        for row in score.rows.iter().filter(|row| content.matches(&row.key)) {
            match row.key.tier {
                Some(Tier::OutOfReach) => out_of_reach.add(&row.counts),
                Some(Tier::Forwarding) => forwarding.add(&row.counts),
                _ => overall.add(&row.counts),
            }
        }
        // Access-only labels sit in the `suspected` rows (their predictions'
        // counts there are not labels and are left out).
        let mut access = Counts::default();
        for row in score.rows.iter().filter(|row| {
            row.key.class == EvidenceClass::Suspected && row.key.tier.is_none_or(Tier::in_overall)
        }) {
            access.expected += row.counts.expected;
            access.found += row.counts.found;
            access.missed += row.counts.missed;
        }
        let false_positives =
            overall.false_positive + out_of_reach.false_positive + forwarding.false_positive;
        let full = disclosure == Disclosure::Full;
        let background =
            (score.totals.negative_controls > 0 && score.totals.exchanges > 0).then(|| {
                Background {
                    false_positives,
                    exchanges: score.totals.exchanges,
                    // Counts stay far below 2^52: exact as f64.
                    per_1k_exchanges: false_positives as f64 * 1000.0
                        / score.totals.exchanges as f64,
                    sources: full.then(|| score.sources.clone()),
                }
            });
        let rows = score
            .rows
            .into_iter()
            .map(|row| ReportRow {
                precision: row.counts.precision(),
                recall: row.counts.recall(),
                key: row.key,
                counts: row.counts,
            })
            .collect();
        let failed_worlds = count(failures.len());
        let unknown_detected_agent = count(unknown_detected_agents.len());
        let detail = full.then_some(Detail {
            failures,
            unknown_detected_agents,
            misses: score.misses,
            false_positives: score.false_positives,
        });
        Self {
            access_only: AccessOnly::of(&overall, &access),
            overall: Summary::of(overall),
            out_of_reach: Summary::of(out_of_reach),
            forwarding: Summary::of(forwarding),
            dataset,
            detector,
            disclosure,
            totals: score.totals,
            rows,
            transmissions: score.transmissions,
            violations: score.violations,
            access_only_under_controls: score.access_only_under_controls,
            gates,
            failed_worlds,
            unscored,
            unknown_detected_agent,
            background,
            detail,
        }
    }

    /// Whether any gate failed.
    pub fn gates_failed(&self) -> bool {
        self.gates.iter().any(GateOutcome::failed)
    }

    /// 0, or [`GATE_FAILURE_EXIT`] when a gate failed.
    pub fn exit_code(&self) -> u8 {
        if self.gates_failed() {
            GATE_FAILURE_EXIT
        } else {
            0
        }
    }

    /// `report.json`'s text: pretty JSON and a final newline. Every list is
    /// in key order, so equal runs give equal bytes.
    pub fn to_json(&self) -> Result<String, ReportError> {
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }

    /// Writes `report.json` and `report.txt` (the table) into `dir`.
    pub fn write(&self, dir: &Path) -> Result<(), ReportError> {
        let write = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).map_err(|source| ReportError::Write {
                path: path.display().to_string(),
                source,
            })
        };
        write("report.json", &self.to_json()?)?;
        write("report.txt", &table::render(self))
    }
}
