//! A world: agents that only talk to each other, their exchanges in time
//! order, the messages those exchanges name, and the world's labels.
//!
//! A [`World`] exists only once it passed the format's cross-file checks
//! ([`WorldInputs::new`] and [`check_labels`]), so a converter can never
//! hand the export writer an invalid world. Converters build one with
//! [`WorldBuilder`].

mod builder;
pub mod client;
mod stop;

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_format::check::{InputError, LabelError, WorldInputs, check_labels};
use a2a_bench_format::exchange::{Exchange, WorldDecl};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, DatasetId, ExchangeId, InvalidKey, MessageId, WorldKey};
use a2a_bench_format::labels::{InvalidLabel, Label};
use a2a_bench_format::message::{InvalidMessage, Message};
use a2a_bench_format::time::Timestamp;

pub use builder::{ExchangeDraft, WorldAgent, WorldBuilder};
pub use client::{Credential, credential_digest, vendor_of};
pub use stop::StopReason;

use crate::clock::ClockError;

/// Why a world (or one of its parts) could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorpusError {
    #[error(transparent)]
    Key(#[from] InvalidKey),
    #[error(transparent)]
    Message(#[from] InvalidMessage),
    #[error(transparent)]
    Label(#[from] InvalidLabel),
    #[error(transparent)]
    Clock(#[from] ClockError),
    #[error("agent {0} is declared twice")]
    DuplicateAgent(AgentKey),
    #[error("agent {0} is not declared in its world")]
    UnknownAgent(AgentKey),
    #[error("agent {0} is scripted and makes no exchanges")]
    ScriptedAgent(AgentKey),
    #[error("agent {agent} belongs to world {agent_world}, not {world}")]
    ForeignAgent {
        agent: AgentKey,
        agent_world: WorldKey,
        world: WorldKey,
    },
    #[error("agent {agent} already has an exchange at or after {at:?}")]
    OutOfOrder { agent: AgentKey, at: Timestamp },
    #[error("exchange {0} is added twice")]
    DuplicateExchange(ExchangeId),
    #[error("exchange_agent rows are written by the builder, not added")]
    ExchangeAgentLabel,
    #[error("world {world}: {source}")]
    Inputs { world: WorldKey, source: InputError },
    #[error("world {world}: {source}")]
    Labels { world: WorldKey, source: LabelError },
}

/// One checked world: its inputs (declaration, messages, exchanges in time
/// order), its labels (`exchange_agent` rows first, in exchange order) and
/// its coverage.
#[derive(Debug, Clone)]
pub struct World {
    dataset: DatasetId,
    inputs: WorldInputs,
    labels: Vec<Label>,
    agent_of: BTreeMap<ExchangeId, AgentKey>,
    coverage: Coverage,
    notes: BTreeMap<String, u64>,
}

impl World {
    /// A world from its parts, checked as a reader checks an export:
    /// [`WorldInputs::new`] (every message once and used, exchanges once
    /// each and in time order) and [`check_labels`] (each exchange assigned
    /// to one declared model-driven agent, each agent's exchanges strictly
    /// increasing, labels resolving). [`WorldBuilder::finish`] calls it.
    pub fn new(
        dataset: DatasetId,
        decl: WorldDecl,
        messages: Vec<Message>,
        exchanges: Vec<Exchange>,
        labels: Vec<Label>,
        coverage: Coverage,
    ) -> Result<Self, CorpusError> {
        let world = decl.key.clone();
        let inputs = WorldInputs::new(&world, messages, decl, exchanges).map_err(|source| {
            CorpusError::Inputs {
                world: world.clone(),
                source,
            }
        })?;
        check_labels(&inputs, &labels).map_err(|source| CorpusError::Labels {
            world: world.clone(),
            source,
        })?;
        let agent_of = labels
            .iter()
            .filter_map(|label| match label {
                Label::ExchangeAgent(row) => Some((row.exchange, row.agent.clone())),
                _ => None,
            })
            .collect();
        Ok(Self {
            dataset,
            inputs,
            labels,
            agent_of,
            coverage,
            notes: BTreeMap::new(),
        })
    }

    pub fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    pub fn key(&self) -> &WorldKey {
        &self.inputs.decl().key
    }

    pub fn decl(&self) -> &WorldDecl {
        self.inputs.decl()
    }

    /// The checked inputs, as a detector would read them.
    pub fn inputs(&self) -> &WorldInputs {
        &self.inputs
    }

    /// Exchanges in time order.
    pub fn exchanges(&self) -> &[Exchange] {
        self.inputs.exchanges()
    }

    pub fn exchange(&self, id: ExchangeId) -> Option<&Exchange> {
        self.inputs.exchange(id)
    }

    pub fn message(&self, id: MessageId) -> Option<&Message> {
        self.inputs.message(id)
    }

    /// Each message once, in first-use order: exchanges in time order,
    /// request before response, each list in order. This is the order of
    /// the world's section in `messages.jsonl`.
    pub fn messages_in_order(&self) -> Vec<&Message> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for exchange in self.exchanges() {
            for id in exchange
                .request
                .messages
                .iter()
                .chain(&exchange.response.messages)
            {
                if seen.insert(*id)
                    && let Some(message) = self.message(*id)
                {
                    out.push(message);
                }
            }
        }
        out
    }

    /// The labels: `exchange_agent` rows first, then the converter's, in the
    /// order it added them.
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    /// The agent that made `exchange` (truth).
    pub fn agent_of(&self, exchange: ExchangeId) -> Option<&AgentKey> {
        self.agent_of.get(&exchange)
    }

    pub fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// Counts the converter reports for this world (things it dropped or
    /// did not label), by name; the export writes them into the manifest's
    /// world entry as `notes`.
    pub fn notes(&self) -> &BTreeMap<String, u64> {
        &self.notes
    }

    /// Adds `n` to note `name`. A note that stays at 0 is not recorded.
    pub fn add_note(&mut self, name: &str, n: u64) {
        add_note(&mut self.notes, name, n);
    }
}

/// Adds `n` to note `name` in `notes`, recording nothing for 0.
pub(crate) fn add_note(notes: &mut BTreeMap<String, u64>, name: &str, n: u64) {
    if n == 0 {
        return;
    }
    let count = notes.entry(name.to_owned()).or_insert(0);
    *count = count.saturating_add(n);
}
