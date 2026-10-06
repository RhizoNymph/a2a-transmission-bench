//! Accumulating a score world by world.

use std::collections::BTreeMap;

use a2a_bench_format::ids::{DatasetId, TransmissionRef};
use a2a_bench_format::labels::{NegativeReason, RouteKind};
use a2a_bench_format::predictions::Quality;

use super::Score;
use super::judge::{Expects, Judge, Outcome};
use super::rows::{
    AccessOnlyControlRow, Counts, EXCERPT_CHARS, FalsePositive, Miss, Row, RowKey, Totals,
    TransmissionCounts, TransmissionKey, TransmissionRow, ViolationRow,
};
use super::sources::SourceTally;
use crate::canon::{AsGiven, Canonicalize};
use crate::class::{EvidenceClass, need_class};
use crate::predict::Prediction;
use crate::world::World;

/// What a transmission's predictions were judged: genuine when any is
/// correct, a false detection when none is and some is false, unlabeled
/// otherwise.
#[derive(Debug, Clone, Copy, Default)]
struct Verdicts {
    correct: bool,
    wrong: bool,
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn excerpt(world: &World<'_>, prediction: &Prediction) -> Option<String> {
    let text = world.inputs.text_at(&prediction.read_at).ok()?;
    Some(text.chars().take(EXCERPT_CHARS).collect())
}

/// Accumulates scores world by world.
pub struct Scorer {
    example_cap: usize,
    canon: Box<dyn Canonicalize>,
    totals: Totals,
    rows: BTreeMap<RowKey, Counts>,
    transmissions: BTreeMap<TransmissionKey, TransmissionCounts>,
    violations: BTreeMap<(DatasetId, NegativeReason), u64>,
    access_only_under_controls: BTreeMap<(DatasetId, EvidenceClass, NegativeReason), u64>,
    misses: Vec<Miss>,
    false_positives: Vec<FalsePositive>,
    sources: SourceTally,
}

impl Scorer {
    /// `example_cap` bounds how many misses and false positives are kept for
    /// citing; counts are always complete. Resources are compared as given.
    pub fn new(example_cap: usize) -> Self {
        Self::with_canonicalizer(example_cap, Box::new(AsGiven))
    }

    /// A scorer that compares channel resources after `canon`.
    pub fn with_canonicalizer(example_cap: usize, canon: Box<dyn Canonicalize>) -> Self {
        Self {
            example_cap,
            canon,
            totals: Totals::default(),
            rows: BTreeMap::new(),
            transmissions: BTreeMap::new(),
            violations: BTreeMap::new(),
            access_only_under_controls: BTreeMap::new(),
            misses: Vec::new(),
            false_positives: Vec::new(),
            sources: SourceTally::default(),
        }
    }

    /// Scores one world's predictions.
    pub fn add_world(&mut self, world: &World<'_>, predictions: &[Prediction]) {
        let dataset = world.dataset.clone();
        let judge = Judge::new(world, self.canon.as_ref());
        self.totals.worlds += 1;
        self.totals.agents += count(world.inputs.decl().agents.len());
        self.totals.exchanges += count(world.inputs.exchanges().len());
        self.totals.expectations += count(judge.positives().len());
        self.totals.negative_controls += count(judge.negatives().len());
        self.totals.predictions += count(predictions.len());

        let mut found = vec![false; judge.positives().len()];
        let mut suspected = vec![false; judge.positives().len()];
        let mut by_transmission: BTreeMap<&TransmissionRef, (RouteKind, Quality, Verdicts)> =
            BTreeMap::new();
        for prediction in predictions {
            let (outcome, control) = judge.judge(prediction);
            let tier = match outcome {
                Outcome::Correct { tier, .. } => {
                    // Every label it aligns with is found (or suspected),
                    // not only the first, which decides its row. Content
                    // finds a content label; access evidence finds an
                    // access-only label and marks a content one suspected.
                    for at in judge.aligned(prediction) {
                        let Some(positive) = judge.positives().get(at) else {
                            continue;
                        };
                        let mark = match (positive.expects, prediction.class.is_content()) {
                            (Expects::Content, true) | (Expects::Access, false) => &mut found,
                            (Expects::Content, false) => &mut suspected,
                            (Expects::Access, true) => continue,
                        };
                        if let Some(slot) = mark.get_mut(at) {
                            *slot = true;
                        }
                    }
                    Some(tier)
                }
                Outcome::False { tier, .. } => Some(tier),
                Outcome::Unjudged | Outcome::Dismissed => None,
            };
            let counts = self
                .rows
                .entry(RowKey {
                    dataset: dataset.clone(),
                    route: prediction.route.kind(),
                    carrier: prediction.carrier,
                    class: prediction.class,
                    tier,
                })
                .or_default();
            counts.predicted += 1;
            let entry = by_transmission.entry(&prediction.transmission).or_insert((
                prediction.route.kind(),
                prediction.quality,
                Verdicts::default(),
            ));
            match outcome {
                Outcome::Correct { .. } => {
                    counts.correct += 1;
                    entry.2.correct = true;
                }
                Outcome::False { violated, .. } => {
                    counts.false_positive += 1;
                    entry.2.wrong = true;
                    match violated {
                        // Only content violates a control.
                        Some(reason) if prediction.class.is_content() => {
                            *self
                                .violations
                                .entry((dataset.clone(), reason))
                                .or_default() += 1;
                            self.sources
                                .add(reason, &excerpt(world, prediction).unwrap_or_default());
                        }
                        Some(reason) => {
                            *self
                                .access_only_under_controls
                                .entry((dataset.clone(), prediction.class, reason))
                                .or_default() += 1;
                        }
                        None => {}
                    }
                    if self.false_positives.len() < self.example_cap {
                        self.false_positives.push(FalsePositive {
                            prediction: prediction.clone(),
                            violated,
                            control: control.map(|control| control.fields().source.clone()),
                            excerpt: excerpt(world, prediction),
                        });
                    }
                }
                Outcome::Unjudged => counts.unjudged += 1,
                Outcome::Dismissed => {
                    counts.dismissed += 1;
                    if let Some(control) = control {
                        *self
                            .access_only_under_controls
                            .entry((dataset.clone(), prediction.class, control.fields().reason))
                            .or_default() += 1;
                    }
                }
            }
        }
        for (route, quality, verdicts) in by_transmission.into_values() {
            let counts = self
                .transmissions
                .entry(TransmissionKey {
                    dataset: dataset.clone(),
                    route,
                    quality,
                })
                .or_default();
            match (verdicts.correct, verdicts.wrong) {
                (true, _) => counts.genuine += 1,
                (false, true) => counts.false_detection += 1,
                (false, false) => counts.unlabeled += 1,
            }
        }
        for ((positive, found), suspected) in judge.positives().iter().zip(found).zip(suspected) {
            let label = positive.expected.fields();
            let class = match positive.expects {
                Expects::Content => EvidenceClass::from(need_class(&label.needs)),
                Expects::Access => EvidenceClass::Suspected,
            };
            let counts = self
                .rows
                .entry(RowKey {
                    dataset: dataset.clone(),
                    route: label.route.kind(),
                    carrier: label.carrier,
                    class,
                    tier: Some(label.tier),
                })
                .or_default();
            counts.expected += 1;
            if found {
                counts.found += 1;
            } else {
                counts.missed += 1;
                if suspected {
                    counts.suspected += 1;
                }
                if self.misses.len() < self.example_cap {
                    self.misses.push(Miss {
                        expectation: positive.expected.clone(),
                    });
                }
            }
        }
    }

    pub fn finish(self) -> Score {
        Score {
            totals: self.totals,
            rows: self
                .rows
                .into_iter()
                .map(|(key, counts)| Row { key, counts })
                .collect(),
            transmissions: self
                .transmissions
                .into_iter()
                .map(|(key, counts)| TransmissionRow { key, counts })
                .collect(),
            violations: self
                .violations
                .into_iter()
                .map(|((dataset, reason), count)| ViolationRow {
                    dataset,
                    reason,
                    count,
                })
                .collect(),
            access_only_under_controls: self
                .access_only_under_controls
                .into_iter()
                .map(|((dataset, class, reason), count)| AccessOnlyControlRow {
                    dataset,
                    class,
                    reason,
                    count,
                })
                .collect(),
            misses: self.misses,
            false_positives: self.false_positives,
            sources: self.sources.top(),
        }
    }
}
