//! Scoring a run: an export's messages, exchanges and labels and a
//! detector's predictions, streamed world by world in lockstep.
//!
//! Per world: the four files' next sections must name the same world (the
//! exchanges file's order is the export's); the inputs are checked
//! (`WorldInputs::new`) and the labels against them (`check_labels`), and a
//! failure there fails the run (the export is broken). Then the world's
//! predictions status decides: `no_consumers` is counted unscored,
//! `failed` is a failed world, and `scored` worlds have their predictions
//! checked (`check_predictions`), mapped to true agents and scored. A
//! prediction check or a merged attribution fails the world, not the run.
//! Memory is bounded by the largest world.

mod error;
mod export;

use std::io::BufRead;

use serde::{Deserialize, Serialize};

use a2a_bench_format::check::{WorldInputs, check_labels, check_predictions};
use a2a_bench_format::files::{
    DetectorInfo, ExchangeRow, Exchanges, Labels, MessageRow, Messages, Predictions,
};
use a2a_bench_format::ids::{
    AgentKey, DatasetId, DetectorAgent, Digest, TransmissionRef, WorldKey,
};
use a2a_bench_format::jsonl::{FileKind, FileReader, HeaderFields, Keyed, WorldSection};
use a2a_bench_format::predictions::WorldStatus;

pub use error::{FileName, RunError};
pub use export::score_export;

use crate::canon::{AsGiven, Canonicalize};
use crate::predict::{AgentMapError, world_predictions};
use crate::score::{Score, Scorer};
use crate::world::World;

/// How many misses and false positives a run keeps as examples by default
/// (ct-eval's `--examples`).
pub const DEFAULT_EXAMPLES: usize = 50;

/// How to score a run.
pub struct ScoreOptions {
    /// How many misses and false positives to keep as examples.
    pub example_cap: usize,
    /// How channel resources are canonicalised before they are compared.
    pub canonicalizer: Box<dyn Canonicalize>,
    /// The manifest digest the predictions must name; `None` checks
    /// nothing (the caller has no manifest).
    pub manifest_digest: Option<Digest>,
}

impl Default for ScoreOptions {
    fn default() -> Self {
        Self {
            example_cap: DEFAULT_EXAMPLES,
            canonicalizer: Box::new(AsGiven),
            manifest_digest: None,
        }
    }
}

/// The four files of a run, as readers.
pub struct Streams<M, E, L, P> {
    pub messages: M,
    pub exchanges: E,
    pub labels: L,
    pub predictions: P,
}

/// Worlds the detector took in but could not detect in (`no_consumers`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unscored {
    pub worlds: u64,
    /// Exchanges the detector took in.
    pub ingested: u64,
}

/// Why one world was not scored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorldFailure {
    /// The detector reported it could not process the world.
    Detector { reason: String },
    /// One detector agent holds exchanges of two true agents.
    MergedAgents {
        agent: DetectorAgent,
        first: AgentKey,
        second: AgentKey,
    },
    /// The world's predictions fail `check_predictions`, or attribute an
    /// exchange no agent made.
    InvalidPredictions { reason: String },
}

impl From<AgentMapError> for WorldFailure {
    fn from(error: AgentMapError) -> Self {
        match error {
            AgentMapError::Merged {
                agent,
                first,
                second,
            } => Self::MergedAgents {
                agent,
                first,
                second,
            },
            other @ AgentMapError::UnknownExchange { .. } => Self::InvalidPredictions {
                reason: other.to_string(),
            },
        }
    }
}

/// A world that was not scored, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedWorld {
    pub world: WorldKey,
    pub failure: WorldFailure,
}

/// A transmission left out of the score for naming a detector agent with
/// no true agent (ct-eval's `unknown_detected_agent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownDetected {
    pub world: WorldKey,
    pub transmission: TransmissionRef,
    pub agent: DetectorAgent,
}

/// A finished run: the score, and what was left out of it.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub dataset: DatasetId,
    pub detector: DetectorInfo,
    pub score: Score,
    pub failures: Vec<FailedWorld>,
    pub unscored: Unscored,
    pub unknown_detected_agents: Vec<UnknownDetected>,
}

impl RunSummary {
    /// A run that scored every world it read.
    pub fn new(dataset: DatasetId, detector: DetectorInfo, score: Score) -> Self {
        Self {
            dataset,
            detector,
            score,
            failures: Vec::new(),
            unscored: Unscored::default(),
            unknown_detected_agents: Vec::new(),
        }
    }
}

fn open<K: FileKind, R: BufRead>(input: R, file: FileName) -> Result<FileReader<K, R>, RunError> {
    FileReader::open(input).map_err(|source| RunError::Read {
        file,
        source: Box::new(source),
    })
}

fn next<K: FileKind, R: BufRead>(
    reader: &mut FileReader<K, R>,
    file: FileName,
) -> Result<Option<WorldSection<K>>, RunError> {
    reader.next_world().map_err(|source| RunError::Read {
        file,
        source: Box::new(source),
    })
}

/// `section` must be world `expected`.
fn same_world<K: FileKind>(
    section: Option<WorldSection<K>>,
    file: FileName,
    expected: &WorldKey,
) -> Result<WorldSection<K>, RunError> {
    let section = section.ok_or_else(|| RunError::MissingWorld {
        file,
        expected: expected.clone(),
    })?;
    if section.world.key() != expected {
        return Err(RunError::WorldOrder {
            file,
            expected: expected.clone(),
            got: section.world.key().clone(),
        });
    }
    Ok(section)
}

/// After the export's last world, `section` must be none.
fn no_more<K: FileKind>(section: Option<WorldSection<K>>, file: FileName) -> Result<(), RunError> {
    match section {
        None => Ok(()),
        Some(section) => Err(RunError::ExtraWorld {
            file,
            got: section.world.key().clone(),
        }),
    }
}

fn same_dataset(file: FileName, expected: &DatasetId, got: &DatasetId) -> Result<(), RunError> {
    if expected == got {
        Ok(())
    } else {
        Err(RunError::DatasetMismatch {
            file,
            expected: expected.clone(),
            got: got.clone(),
        })
    }
}

/// Scores a run read from `streams` (module docs). Every file is read to
/// its trailer, so a truncated or altered file is an error, never a smaller
/// score.
pub fn score_streams<M: BufRead, E: BufRead, L: BufRead, P: BufRead>(
    streams: Streams<M, E, L, P>,
    options: ScoreOptions,
) -> Result<RunSummary, RunError> {
    let mut messages = open::<Messages, _>(streams.messages, FileName::Messages)?;
    let mut exchanges = open::<Exchanges, _>(streams.exchanges, FileName::Exchanges)?;
    let mut labels = open::<Labels, _>(streams.labels, FileName::Labels)?;
    let mut predictions = open::<Predictions, _>(streams.predictions, FileName::Predictions)?;
    let dataset = exchanges.header().dataset().clone();
    same_dataset(FileName::Messages, &dataset, messages.header().dataset())?;
    same_dataset(FileName::Labels, &dataset, labels.header().dataset())?;
    same_dataset(
        FileName::Predictions,
        &dataset,
        predictions.header().dataset(),
    )?;
    if let Some(export) = options.manifest_digest
        && export != predictions.header().manifest_digest
    {
        return Err(RunError::ManifestDigest {
            export,
            predictions: predictions.header().manifest_digest,
        });
    }
    let detector = predictions.header().detector.clone();
    let mut scorer = Scorer::with_canonicalizer(options.example_cap, options.canonicalizer);
    let mut summary = RunSummary::new(dataset.clone(), detector, Scorer::new(0).finish());
    while let Some(exchange_section) = next(&mut exchanges, FileName::Exchanges)? {
        let key = exchange_section.world.key.clone();
        let message_section = same_world(
            next(&mut messages, FileName::Messages)?,
            FileName::Messages,
            &key,
        )?;
        let label_section =
            same_world(next(&mut labels, FileName::Labels)?, FileName::Labels, &key)?;
        let prediction_section = same_world(
            next(&mut predictions, FileName::Predictions)?,
            FileName::Predictions,
            &key,
        )?;
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
        )
        .map_err(|source| RunError::Inputs {
            world: key.clone(),
            source: Box::new(source),
        })?;
        check_labels(&inputs, &label_section.rows).map_err(|source| RunError::Labels {
            world: key.clone(),
            source: Box::new(source),
        })?;
        let rows = prediction_section.rows;
        let failure = match prediction_section.world.status {
            WorldStatus::NoConsumers { ingested } => {
                summary.unscored.worlds += 1;
                summary.unscored.ingested += ingested;
                continue;
            }
            WorldStatus::Failed { reason } => WorldFailure::Detector { reason },
            WorldStatus::Scored => {
                let made = check_predictions(&inputs, &rows)
                    .map_err(|error| WorldFailure::InvalidPredictions {
                        reason: error.to_string(),
                    })
                    .and_then(|()| {
                        world_predictions(&label_section.rows, &rows).map_err(WorldFailure::from)
                    });
                match made {
                    Ok(made) => {
                        let world = World {
                            dataset: &dataset,
                            inputs: &inputs,
                            labels: &label_section.rows,
                            coverage: label_section.world.coverage,
                        };
                        scorer.add_world(&world, &made.predictions);
                        summary
                            .unknown_detected_agents
                            .extend(made.unknown.into_iter().map(|unknown| UnknownDetected {
                                world: key.clone(),
                                transmission: unknown.transmission,
                                agent: unknown.agent,
                            }));
                        continue;
                    }
                    Err(failure) => failure,
                }
            }
        };
        summary.failures.push(FailedWorld {
            world: key,
            failure,
        });
    }
    no_more(next(&mut messages, FileName::Messages)?, FileName::Messages)?;
    no_more(next(&mut labels, FileName::Labels)?, FileName::Labels)?;
    no_more(
        next(&mut predictions, FileName::Predictions)?,
        FileName::Predictions,
    )?;
    summary.score = scorer.finish();
    Ok(summary)
}
