//! Regression gates: per-detector, per-dataset bounds a run must meet.
//!
//! A gates file names one detector and holds its gates. A gate selects rows
//! (any of dataset, route, carrier, class, tier; unset means any, except
//! that an unset class means any content class), sums them, and checks one
//! metric:
//!
//! ```toml
//! detector = "crosstalk-live"
//!
//! [[gate]]
//! name = "salt (live): every construction label"
//! variant = "forwarding-off"
//! dataset = "salt"
//! tier = "construction"
//! metric = "recall"
//! min = 0.83
//! ```
//!
//! A gate checks only runs of its file's detector (the predictions
//! header's `detector.name`) and, when it names one, of its `variant`
//! (`detector.variant`; unset means every variant). `recall` and
//! `precision` take a `min`; `violations` (negative controls a content
//! prediction fell under, optionally of one `reason`) takes a whole `max`;
//! `fp_per_1k` (the selected rows' false positives per 1,000 of the run's
//! exchanges) takes a `max`, and is skipped in a run with no exchanges. A
//! gate naming a dataset the run did not score is skipped as
//! `other_dataset` (never a pass on an empty count); a gate whose rows hold
//! no data is skipped, not failed. Thresholds are regression gates on a
//! detector, not invariants of it.

mod parse;
mod search;

use std::path::Path;

use serde::{Deserialize, Serialize};

use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::labels::{CarrierKind, NegativeReason, RouteKind, Tier};

pub use parse::{GateError, InvalidGate};
pub use search::{GATES_ENV, GateSearch, GatesFrom, GatesLocation, REPO_GATES};

use crate::class::EvidenceClass;
use crate::score::{Score, Selector};

/// What a gate checks, and its bound.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Recall {
        min: f64,
    },
    Precision {
        min: f64,
    },
    Violations {
        max: u64,
        reason: Option<NegativeReason>,
    },
    /// False positives of the selected rows per 1,000 exchanges of the run
    /// (`Totals::exchanges`): a ceiling on the background rate.
    FpPer1k {
        max: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    pub name: String,
    /// The detector name whose runs it checks: its file's `detector`.
    pub detector: String,
    /// The detector variant whose runs it checks; `None` checks every one.
    pub variant: Option<String>,
    pub dataset: Option<DatasetId>,
    pub route: Option<RouteKind>,
    pub carrier: Option<CarrierKind>,
    pub class: Option<EvidenceClass>,
    pub tier: Option<Tier>,
    pub check: Check,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gates {
    pub gates: Vec<Gate>,
}

impl Gates {
    /// One gates file's text; `path` names it in errors.
    pub fn parse(text: &str, path: &str) -> Result<Self, GateError> {
        parse::parse(text, path).map(|gates| Self { gates })
    }

    /// A gates file, or every `*.toml` file of a directory in name order.
    pub fn load(path: &Path) -> Result<Self, GateError> {
        parse::load(path).map(|gates| Self { gates })
    }

    pub fn extend(&mut self, other: Self) {
        self.gates.extend(other.gates);
    }

    /// The gates of one run: of detector `name`, and of `variant` or of
    /// every variant.
    pub fn for_run(&self, name: &str, variant: &str) -> Self {
        Self {
            gates: self
                .gates
                .iter()
                .filter(|gate| {
                    gate.detector == name && gate.variant.as_deref().is_none_or(|v| v == variant)
                })
                .cloned()
                .collect(),
        }
    }

    pub fn evaluate(&self, score: &Score) -> Vec<GateOutcome> {
        self.gates.iter().map(|gate| gate.evaluate(score)).collect()
    }
}

/// How a gate came out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GateStatus {
    Pass {
        value: f64,
    },
    Fail {
        value: f64,
        bound: f64,
    },
    /// No row it selects holds data for its metric.
    Skipped,
    /// It names a dataset the run did not score: it says nothing about
    /// this run.
    OtherDataset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateOutcome {
    pub name: String,
    #[serde(flatten)]
    pub status: GateStatus,
}

impl GateOutcome {
    pub fn failed(&self) -> bool {
        matches!(self.status, GateStatus::Fail { .. })
    }
}

impl Gate {
    fn selector(&self) -> Selector {
        Selector {
            dataset: self.dataset.clone(),
            route: self.route,
            carrier: self.carrier,
            class: self.class,
            tier: self.tier,
        }
    }

    pub fn evaluate(&self, score: &Score) -> GateOutcome {
        if let Some(dataset) = &self.dataset
            && !score.scored(dataset)
        {
            return GateOutcome {
                name: self.name.clone(),
                status: GateStatus::OtherDataset,
            };
        }
        let counts = score.total(&self.selector());
        let status = match &self.check {
            Check::Recall { min } => at_least(counts.recall(), *min),
            Check::Precision { min } => at_least(counts.precision(), *min),
            Check::Violations { max, reason } => {
                let value = score.violation_count(self.dataset.as_ref(), *reason);
                // Counts are far below 2^52: exact as f64.
                let (value, bound) = (value as f64, *max as f64);
                if value <= bound {
                    GateStatus::Pass { value }
                } else {
                    GateStatus::Fail { value, bound }
                }
            }
            Check::FpPer1k { max } => {
                fp_rate_at_most(counts.false_positive, score.totals.exchanges, *max)
            }
        };
        GateOutcome {
            name: self.name.clone(),
            status,
        }
    }
}

/// `false_positives` per 1,000 of `exchanges`, against a ceiling; skipped
/// with no exchanges.
fn fp_rate_at_most(false_positives: u64, exchanges: u64, max: f64) -> GateStatus {
    if exchanges == 0 {
        return GateStatus::Skipped;
    }
    // Counts are far below 2^52: exact as f64.
    let value = false_positives as f64 * 1000.0 / exchanges as f64;
    if value <= max {
        GateStatus::Pass { value }
    } else {
        GateStatus::Fail { value, bound: max }
    }
}

fn at_least(value: Option<f64>, min: f64) -> GateStatus {
    match value {
        None => GateStatus::Skipped,
        Some(value) if value >= min => GateStatus::Pass { value },
        Some(value) => GateStatus::Fail { value, bound: min },
    }
}
