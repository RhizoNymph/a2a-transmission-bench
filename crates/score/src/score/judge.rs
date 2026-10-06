//! Judging one prediction against one world's labels, through the
//! alignment rule.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::ExchangeId;
use a2a_bench_format::labels::{
    Exemption, ExpectedTransmission, Label, NegativeControl, NegativeReason, Tier,
};

use super::align::{aligns, exempts, specificity, violates};
use crate::canon::Canonicalize;
use crate::class::EvidenceClass;
use crate::predict::Prediction;
use crate::world::World;

/// What a prediction turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// It aligns with the label at this index of the world's positives.
    Correct { expectation: usize, tier: Tier },
    /// It aligns with no label and the world's labels say it is wrong:
    /// either it falls under a negative control or the world's coverage is
    /// complete.
    False {
        violated: Option<NegativeReason>,
        tier: Tier,
    },
    /// It aligns with no label and the world's labels are a sample, or it
    /// falls under an exemption.
    Unjudged,
    /// A discarded co-access that aligns with no label: the detector
    /// opened it and decided it was not a transmission. Never a false
    /// positive and never charged to a negative control.
    Dismissed,
}

/// What evidence a positive label expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expects {
    /// A content match (a `transmission` label).
    Content,
    /// Access evidence only (an `access_only` label).
    Access,
}

/// One positive label and the evidence it expects.
#[derive(Debug, Clone, Copy)]
pub struct Positive<'w> {
    pub expected: &'w ExpectedTransmission,
    pub expects: Expects,
}

/// One world's labels, indexed for judging.
pub struct Judge<'w> {
    coverage: Coverage,
    canon: &'w dyn Canonicalize,
    positives: Vec<Positive<'w>>,
    /// Positives by reader exchange (alignment then checks the reader).
    by_reader: BTreeMap<ExchangeId, Vec<usize>>,
    negatives: Vec<&'w NegativeControl>,
    exemptions: Vec<&'w Exemption>,
}

impl<'w> Judge<'w> {
    /// `canon` canonicalises channel resources before they are compared.
    pub fn new(world: &World<'w>, canon: &'w dyn Canonicalize) -> Self {
        let mut positives = Vec::new();
        let mut negatives = Vec::new();
        let mut exemptions = Vec::new();
        for label in world.labels {
            match label {
                Label::Transmission(expected) => positives.push(Positive {
                    expected,
                    expects: Expects::Content,
                }),
                Label::AccessOnly(expected) => positives.push(Positive {
                    expected: expected.transmission(),
                    expects: Expects::Access,
                }),
                Label::NegativeControl(control) => negatives.push(control),
                Label::Exemption(exemption) => exemptions.push(exemption),
                Label::ExchangeAgent(_) | Label::AgentCluster(_) => {}
            }
        }
        negatives.sort_by_key(|control| specificity(control));
        let mut by_reader: BTreeMap<ExchangeId, Vec<usize>> = BTreeMap::new();
        for (at, positive) in positives.iter().enumerate() {
            by_reader
                .entry(positive.expected.fields().reader_exchange)
                .or_default()
                .push(at);
        }
        Self {
            coverage: world.coverage,
            canon,
            positives,
            by_reader,
            negatives,
            exemptions,
        }
    }

    /// Every positive label, content and access-only alike.
    pub fn positives(&self) -> &[Positive<'w>] {
        &self.positives
    }

    pub fn negatives(&self) -> &[&'w NegativeControl] {
        &self.negatives
    }

    fn candidates(&self, prediction: &Prediction) -> &[usize] {
        self.by_reader
            .get(&prediction.reader_exchange)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn aligns_with(&self, prediction: &Prediction, at: usize) -> bool {
        self.positives
            .get(at)
            .is_some_and(|positive| aligns(prediction, positive.expected, self.canon))
    }

    /// Every label `prediction` aligns with, by index into
    /// [`Judge::positives`]. One prediction can find several labels: a
    /// match whose read range covers two adjacent labelled texts of one
    /// sender aligns with both.
    pub fn aligned<'p>(&'p self, prediction: &'p Prediction) -> impl Iterator<Item = usize> + 'p {
        self.candidates(prediction)
            .iter()
            .copied()
            .filter(move |&at| self.aligns_with(prediction, at))
    }

    /// The outcome of `prediction`: the first label it aligns with, else
    /// dismissed when it is a discarded co-access, else unjudged when an
    /// exemption covers it, else the most specific negative control it
    /// violates, else what the world's coverage makes of an unlabelled
    /// prediction.
    ///
    /// The control is the one the outcome is about: the violated one of a
    /// `False`, or, for a `Dismissed` prediction, the most specific control
    /// it falls under (recorded, never charged).
    pub fn judge(&self, prediction: &Prediction) -> (Outcome, Option<&'w NegativeControl>) {
        if let Some((at, positive)) = self
            .aligned(prediction)
            .next()
            .and_then(|at| self.positives.get(at).map(|positive| (at, positive)))
        {
            // Any evidence aligned with an access-only label is right about
            // the pair and the place, so it is correct; only access evidence
            // finds that label (the scorer).
            return (
                Outcome::Correct {
                    expectation: at,
                    tier: positive.expected.fields().tier,
                },
                None,
            );
        }
        if prediction.class == EvidenceClass::Discarded {
            return (Outcome::Dismissed, self.control_over(prediction));
        }
        if self
            .exemptions
            .iter()
            .any(|exemption| exempts(prediction, exemption))
        {
            return (Outcome::Unjudged, None);
        }
        if let Some(control) = self.control_over(prediction) {
            let label = control.fields();
            return (
                Outcome::False {
                    violated: Some(label.reason),
                    tier: label.tier,
                },
                Some(control),
            );
        }
        match self.coverage {
            Coverage::Complete { tier } => (
                Outcome::False {
                    violated: None,
                    tier,
                },
                None,
            ),
            Coverage::Partial => (Outcome::Unjudged, None),
        }
    }

    /// The most specific negative control `prediction` falls under.
    fn control_over(&self, prediction: &Prediction) -> Option<&'w NegativeControl> {
        self.negatives
            .iter()
            .copied()
            .find(|control| violates(prediction, control))
    }
}
