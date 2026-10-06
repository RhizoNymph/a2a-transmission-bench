//! Worlds of independent agents: the background (negative) corpora.
//!
//! A background world mixes trajectories that never talked to each other
//! (separate runs of separate tasks) into one world, so a detector meets
//! the boilerplate real agents share: harness banners, test-runner headers,
//! interpreter footers, the same repository's source quoted by two agents.
//! Nothing one of them wrote reached another, so:
//!
//! - the world's coverage is complete at the construction tier: every
//!   prediction is a false positive;
//! - every (sender, reader exchange) pair of distinct trajectories gets a
//!   negative control, `shared_source` when the two worked on the same
//!   repository or task (`group`) and `boilerplate` otherwise, so the report
//!   says which kind of shared text a false positive came from. Its id is
//!   `bg/<sender>/<reader exchange>`.
//!
//! Converters hand over [`Trajectory`]s; [`BackgroundWorld`] declares their
//! agents, adds their exchanges and, on [`finish`](BackgroundWorld::finish),
//! the controls. A generator that plants transmissions (the splice corpus)
//! adds its labels with [`label`](BackgroundWorld::label) before finishing;
//! the planted (sender, reader exchange) gets no control, so a prediction
//! there that misses the label (a wrong route, say) is a plain false
//! positive rather than one charged to boilerplate.

use std::collections::BTreeSet;

use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, DatasetId, ExchangeId, LabelId, SourceRef, WorldKey};
use a2a_bench_format::labels::{ControlFields, Label, NegativeControl, NegativeReason, Tier};
use a2a_bench_format::message::Message;
use a2a_bench_format::time::Timestamp;

use crate::world::{CorpusError, ExchangeDraft, StopReason, World, WorldAgent, WorldBuilder};

/// One model call of a trajectory.
#[derive(Debug, Clone)]
pub struct Call {
    pub at: Timestamp,
    pub request: Vec<Message>,
    pub response: Message,
    pub stop: StopReason,
    pub fidelity: Fidelity,
    pub source: SourceRef,
}

/// One independent agent's calls.
#[derive(Debug, Clone)]
pub struct Trajectory {
    /// Unique within its world.
    pub name: String,
    pub model: String,
    /// The repository or task it worked on: two trajectories of one group
    /// share source text without talking.
    pub group: String,
    pub calls: Vec<Call>,
}

struct Added {
    key: AgentKey,
    group: String,
    exchanges: Vec<(ExchangeId, SourceRef)>,
}

/// A world being assembled from independent trajectories.
pub struct BackgroundWorld {
    builder: WorldBuilder,
    added: Vec<Added>,
    planted: BTreeSet<(AgentKey, ExchangeId)>,
}

impl BackgroundWorld {
    pub fn new(dataset: DatasetId, key: WorldKey) -> Self {
        Self {
            builder: WorldBuilder::new(dataset, key),
            added: Vec::new(),
            planted: BTreeSet::new(),
        }
    }

    /// Declares the trajectory's agent and adds its calls; returns the
    /// agent and the exchange id of each call, in call order.
    pub fn add(
        &mut self,
        trajectory: Trajectory,
    ) -> Result<(WorldAgent, Vec<ExchangeId>), CorpusError> {
        let agent = self
            .builder
            .model_agent(&trajectory.name, &trajectory.model)?;
        let mut exchanges = Vec::with_capacity(trajectory.calls.len());
        for call in trajectory.calls {
            let source = call.source.clone();
            let id = self.builder.exchange(ExchangeDraft {
                agent: agent.clone(),
                at: call.at,
                model: Some(trajectory.model.clone()),
                request: call.request,
                response: vec![call.response],
                tools: None,
                stop: Some(call.stop),
                session: None,
                turn: None,
                fidelity: call.fidelity,
                source: call.source,
            })?;
            exchanges.push((id, source));
        }
        let ids = exchanges.iter().map(|(id, _)| *id).collect();
        self.added.push(Added {
            key: agent.key().clone(),
            group: trajectory.group,
            exchanges,
        });
        Ok((agent, ids))
    }

    /// Adds a label; a transmission's (sender, reader exchange) gets no
    /// control.
    pub fn label(&mut self, label: Label) -> Result<(), CorpusError> {
        if let Label::Transmission(expected) = &label {
            let fields = expected.fields();
            self.planted
                .insert((fields.from.clone(), fields.reader_exchange));
        }
        self.builder.label(label)
    }

    /// The world, with a negative control per (sender, reader exchange) of
    /// distinct trajectories, except where a transmission was planted.
    pub fn finish(mut self) -> Result<World, CorpusError> {
        for reader in &self.added {
            for sender in self.added.iter().filter(|other| other.key != reader.key) {
                let reason = if sender.group == reader.group {
                    NegativeReason::SharedSource
                } else {
                    NegativeReason::Boilerplate
                };
                for (exchange, source) in &reader.exchanges {
                    if self.planted.contains(&(sender.key.clone(), *exchange)) {
                        continue;
                    }
                    let control = NegativeControl::new(ControlFields {
                        id: LabelId::new(format!("bg/{}/{exchange}", sender.key))?,
                        from: sender.key.clone(),
                        to: reader.key.clone(),
                        reader_exchange: Some(*exchange),
                        at: None,
                        origin: None,
                        text: None,
                        reason,
                        tier: Tier::Structural,
                        source: source.clone(),
                    })?;
                    self.builder.label(Label::NegativeControl(control))?;
                }
            }
        }
        self.builder.finish(Coverage::Complete {
            tier: Tier::Construction,
        })
    }
}
