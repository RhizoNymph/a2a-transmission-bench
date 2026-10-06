//! Scoring predictions against labels.
//!
//! Every prediction is judged through the alignment rule ([`align`]); every
//! positive label is found (some prediction aligns with it) or missed.
//! Counts are kept per [`RowKey`]: dataset × route kind × carrier × class ×
//! tier.
//!
//! - A label is counted in the row of its own route, carrier, the class it
//!   needs, and its tier: `expected`, then `found` or `missed`. Recall comes
//!   from these.
//! - A prediction is counted in the row of its own route, carrier and class,
//!   with the tier of the label it aligned with (or of the control it
//!   violated, or of the world's coverage): `correct`, `false_positive`,
//!   `unjudged` or `dismissed`. Precision comes from these. Unjudged and
//!   dismissed predictions have no tier.
//! - Only content evidence finds a label. A suspected or discarded
//!   prediction is counted in its own row (class `suspected` or
//!   `discarded`), and a content label it aligns with but no content
//!   prediction does is `missed` and also `suspected`. Selectors and the
//!   overall summary read content rows unless they name an access class.
//! - A discarded prediction that aligns with no label is `dismissed`:
//!   neither a false positive nor a negative-control violation. Its
//!   transmission has no verdict.
//! - An access-only label is counted in the `suspected` row of its route,
//!   carrier and tier, and only a suspected or discarded prediction finds
//!   it. A content prediction aligned with it is correct but does not find
//!   it.
//! - Only content violates a control: an access-class prediction under a
//!   control is recorded in `access_only_under_controls`, never in
//!   `violations`.

pub mod align;
pub mod judge;
pub mod rows;
mod scorer;
pub mod sources;

use serde::{Deserialize, Serialize};

use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::labels::{CarrierKind, NegativeReason, RouteKind, Tier};

pub use judge::{Expects, Judge, Outcome, Positive};
pub use rows::{
    AccessOnlyControlRow, Counts, EXCERPT_CHARS, FalsePositive, Miss, Row, RowKey, Totals,
    TransmissionCounts, TransmissionKey, TransmissionRow, ViolationRow,
};
pub use scorer::Scorer;
pub use sources::SourceCount;

use crate::class::EvidenceClass;

/// The finished score. Every list is in key order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub totals: Totals,
    pub rows: Vec<Row>,
    pub transmissions: Vec<TransmissionRow>,
    /// Negative-control violations by content-class predictions. Only
    /// these are violations: gates and `sources` count them.
    pub violations: Vec<ViolationRow>,
    /// Access-only predictions that fall under a negative control, by
    /// class: never violations and never gated. A `discarded` one is
    /// dismissed (these rows say which controls the dismissed predictions
    /// fell under); a `suspected` one is still a false positive in its own
    /// access-class row, but not charged to the control.
    pub access_only_under_controls: Vec<AccessOnlyControlRow>,
    /// At most the scorer's example cap of each.
    pub misses: Vec<Miss>,
    pub false_positives: Vec<FalsePositive>,
    /// The shared texts negative-control violations fell on, largest
    /// first, tallied over every violation ([`sources`]).
    pub sources: Vec<SourceCount>,
}

/// What a row selector picks; `None` matches anything, except that an
/// unset `class` matches content classes only: access-only rows
/// (`suspected`, `discarded`) are selected by naming them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selector {
    pub dataset: Option<DatasetId>,
    pub route: Option<RouteKind>,
    pub carrier: Option<CarrierKind>,
    pub class: Option<EvidenceClass>,
    pub tier: Option<Tier>,
}

impl Selector {
    pub fn matches(&self, key: &RowKey) -> bool {
        self.dataset.as_ref().is_none_or(|d| *d == key.dataset)
            && self.route.is_none_or(|r| r == key.route)
            && self.carrier.is_none_or(|c| c == key.carrier)
            && self
                .class
                .map_or(key.class.is_content(), |c| c == key.class)
            && self.tier.is_none_or(|t| Some(t) == key.tier)
    }
}

impl Score {
    /// The sum of every row `selector` matches.
    pub fn total(&self, selector: &Selector) -> Counts {
        let mut sum = Counts::default();
        for row in self.rows.iter().filter(|row| selector.matches(&row.key)) {
            sum.add(&row.counts);
        }
        sum
    }

    /// Whether the run scored `dataset`: any row or violation of it.
    pub fn scored(&self, dataset: &DatasetId) -> bool {
        self.rows.iter().any(|row| row.key.dataset == *dataset)
            || self.violations.iter().any(|row| row.dataset == *dataset)
    }

    /// Negative-control violations matching `dataset` (any when `None`).
    pub fn violation_count(
        &self,
        dataset: Option<&DatasetId>,
        reason: Option<NegativeReason>,
    ) -> u64 {
        self.violations
            .iter()
            .filter(|row| dataset.is_none_or(|d| *d == row.dataset))
            .filter(|row| reason.is_none_or(|r| r == row.reason))
            .map(|row| row.count)
            .sum()
    }
}
