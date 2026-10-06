//! What a score holds: counts per row, transmission rows, violations,
//! examples and totals.

use serde::{Deserialize, Serialize};

use a2a_bench_format::ids::{DatasetId, SourceRef};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedTransmission, NegativeReason, RouteKind, Tier,
};
use a2a_bench_format::predictions::Quality;

use crate::class::EvidenceClass;
use crate::predict::Prediction;

/// The breakdown key: dataset × route kind × carrier × class × tier. The
/// derived order (field by field, each enum in declaration order) is the
/// order rows reach output in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RowKey {
    pub dataset: DatasetId,
    pub route: RouteKind,
    pub carrier: CarrierKind,
    pub class: EvidenceClass,
    /// `None` for unjudged and dismissed predictions.
    pub tier: Option<Tier>,
}

/// Counts in one row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Positive labels.
    pub expected: u64,
    /// Labels some content prediction aligned with (for an access-only
    /// label, some suspected or discarded prediction).
    pub found: u64,
    /// Labels not found.
    pub missed: u64,
    /// Missed labels that a suspected or discarded prediction aligned with.
    pub suspected: u64,
    /// Predictions.
    pub predicted: u64,
    /// Predictions that aligned with a label.
    pub correct: u64,
    /// Predictions judged wrong.
    pub false_positive: u64,
    /// Predictions with no label under partial coverage, or exempt.
    pub unjudged: u64,
    /// Discarded predictions that aligned with no label: co-access the
    /// detector itself rejected. Not in precision.
    pub dismissed: u64,
}

impl Counts {
    /// `correct / (correct + false_positive)`; `None` with no judged
    /// prediction.
    pub fn precision(&self) -> Option<f64> {
        ratio(self.correct, self.correct + self.false_positive)
    }

    /// `found / expected`; `None` with no label.
    pub fn recall(&self) -> Option<f64> {
        ratio(self.found, self.expected)
    }

    pub fn add(&mut self, other: &Self) {
        self.expected += other.expected;
        self.found += other.found;
        self.missed += other.missed;
        self.suspected += other.suspected;
        self.predicted += other.predicted;
        self.correct += other.correct;
        self.false_positive += other.false_positive;
        self.unjudged += other.unjudged;
        self.dismissed += other.dismissed;
    }
}

/// `numerator / denominator`; `None` when the denominator is zero.
pub fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        // Counts stay far below 2^52, so the conversion is exact.
        Some(numerator as f64 / denominator as f64)
    }
}

/// One detector transmission's tally key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TransmissionKey {
    pub dataset: DatasetId,
    /// The route of the transmission's first prediction.
    pub route: RouteKind,
    /// The detector's call: its strongest match's class and carrier,
    /// suspected or discarded.
    pub quality: Quality,
}

/// Transmissions by the verdict the labels imply: genuine when any of its
/// predictions is correct, a false detection when none is and some is
/// false, unlabeled otherwise (unjudged or dismissed only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransmissionCounts {
    pub genuine: u64,
    pub false_detection: u64,
    pub unlabeled: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub key: RowKey,
    pub counts: Counts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransmissionRow {
    pub key: TransmissionKey,
    pub counts: TransmissionCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViolationRow {
    pub dataset: DatasetId,
    pub reason: NegativeReason,
    pub count: u64,
}

/// Access-only predictions of one class under one kind of negative
/// control: never violations and never gated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessOnlyControlRow {
    pub dataset: DatasetId,
    pub class: EvidenceClass,
    pub reason: NegativeReason,
    pub count: u64,
}

/// A missed label, for citing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Miss {
    pub expectation: ExpectedTransmission,
}

/// A prediction judged false, for citing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FalsePositive {
    pub prediction: Prediction,
    pub violated: Option<NegativeReason>,
    /// The violated control's source record.
    pub control: Option<SourceRef>,
    /// The reader's text at the prediction's location, cut to
    /// [`EXCERPT_CHARS`] characters.
    pub excerpt: Option<String>,
}

/// How much of a false positive's text a report cites.
pub const EXCERPT_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub worlds: u64,
    pub agents: u64,
    pub exchanges: u64,
    pub expectations: u64,
    pub negative_controls: u64,
    pub predictions: u64,
}
