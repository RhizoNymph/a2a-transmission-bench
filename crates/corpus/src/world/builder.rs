//! [`WorldBuilder`]: how a converter assembles a world.
//!
//! A converter declares its agents, hands each exchange over as an
//! [`ExchangeDraft`] (messages built with `Message::new`, a time, a source
//! reference), adds labels, and finishes the world. The builder derives
//! exchange ids, gives each agent its client (a synthetic credential unless
//! the dataset recorded one), stores each message once, writes the
//! `exchange_agent` rows, and on [`WorldBuilder::finish`] runs the format's
//! checks over the result.

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_format::exchange::{
    AgentDecl, Client, Driven, Exchange, Fidelity, Request, Response, ToolDecl, WorldDecl,
};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::exchange_id;
use a2a_bench_format::ids::{AgentKey, DatasetId, ExchangeId, MessageId, SourceRef, WorldKey};
use a2a_bench_format::labels::{ExchangeAgent, Label};
use a2a_bench_format::message::Message;
use a2a_bench_format::time::Timestamp;

use super::client::{Credential, vendor_of};
use super::{CorpusError, StopReason, World};

/// An agent of one world: the handle [`WorldBuilder`]'s declarations return
/// and drafts name. It carries its world, so a draft naming another
/// world's agent is refused.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldAgent {
    world: WorldKey,
    key: AgentKey,
}

impl WorldAgent {
    pub fn new(world: WorldKey, key: AgentKey) -> Self {
        Self { world, key }
    }

    pub fn world(&self) -> &WorldKey {
        &self.world
    }

    /// The agent's key, as labels name it.
    pub fn key(&self) -> &AgentKey {
        &self.key
    }
}

/// Everything a converter knows about one exchange.
#[derive(Debug, Clone)]
pub struct ExchangeDraft {
    pub agent: WorldAgent,
    pub at: Timestamp,
    /// The model the request named; the agent's declared model when `None`.
    pub model: Option<String>,
    /// The request's messages, in order, system messages inline.
    pub request: Vec<Message>,
    pub response: Vec<Message>,
    /// The tools offered, when the dataset records them.
    pub tools: Option<Vec<ToolDecl>>,
    pub stop: Option<StopReason>,
    /// A session or conversation header, when the dataset has one.
    pub session: Option<String>,
    /// The request's 0-based ordinal in `session`.
    pub turn: Option<u32>,
    pub fidelity: Fidelity,
    /// The record the exchange came from; a derived id derives from it.
    pub source: SourceRef,
}

#[derive(Debug, Clone)]
struct AgentState {
    driven: Driven,
    model: Option<String>,
    credential: Credential,
    last_at: Option<Timestamp>,
}

pub struct WorldBuilder {
    dataset: DatasetId,
    key: WorldKey,
    agents: BTreeMap<AgentKey, AgentState>,
    messages: BTreeMap<MessageId, Message>,
    exchanges: Vec<(Exchange, AgentKey)>,
    ids: BTreeSet<ExchangeId>,
    labels: Vec<Label>,
    notes: BTreeMap<String, u64>,
}

/// Where an exchange's client comes from.
enum ClientFrom {
    /// The agent's credential and model, the draft's session, turn and model.
    Agent,
    /// A client the dataset recorded, kept as it is.
    Recorded(Client),
}

impl WorldBuilder {
    pub fn new(dataset: DatasetId, key: WorldKey) -> Self {
        Self {
            dataset,
            key,
            agents: BTreeMap::new(),
            messages: BTreeMap::new(),
            exchanges: Vec::new(),
            ids: BTreeSet::new(),
            labels: Vec::new(),
            notes: BTreeMap::new(),
        }
    }

    pub fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    pub fn key(&self) -> &WorldKey {
        &self.key
    }

    fn declare(
        &mut self,
        name: &str,
        driven: Driven,
        model: Option<&str>,
        credential: Option<Credential>,
    ) -> Result<WorldAgent, CorpusError> {
        let key = AgentKey::new(name)?;
        if self.agents.contains_key(&key) {
            return Err(CorpusError::DuplicateAgent(key));
        }
        let credential =
            credential.unwrap_or_else(|| Credential::synthetic(&self.dataset, &self.key, &key));
        self.agents.insert(
            key.clone(),
            AgentState {
                driven,
                model: model.map(str::to_owned),
                credential,
                last_at: None,
            },
        );
        Ok(WorldAgent::new(self.key.clone(), key))
    }

    /// Declares model-driven agent `name`, running `model`, with its
    /// synthetic credential.
    pub fn model_agent(&mut self, name: &str, model: &str) -> Result<WorldAgent, CorpusError> {
        self.declare(name, Driven::Model, Some(model), None)
    }

    /// Declares model-driven agent `name` with a credential the dataset
    /// recorded.
    pub fn model_agent_with(
        &mut self,
        name: &str,
        model: &str,
        credential: Credential,
    ) -> Result<WorldAgent, CorpusError> {
        self.declare(name, Driven::Model, Some(model), Some(credential))
    }

    /// Declares scripted agent `name`: part of the world, never an exchange.
    pub fn scripted_agent(&mut self, name: &str) -> Result<WorldAgent, CorpusError> {
        self.declare(name, Driven::Scripted, None, None)
    }

    /// Declares scripted agent `name` with the model its dataset names for
    /// it (a scripted role a run configured with a model it never called).
    pub fn scripted_agent_with_model(
        &mut self,
        name: &str,
        model: &str,
    ) -> Result<WorldAgent, CorpusError> {
        self.declare(name, Driven::Scripted, Some(model), None)
    }

    /// Adds one exchange, its id derived from the dataset, its source and
    /// its time. Its agent must be of this world, declared and model-driven,
    /// and its time later than the agent's previous exchange.
    pub fn exchange(&mut self, draft: ExchangeDraft) -> Result<ExchangeId, CorpusError> {
        let id = exchange_id(&self.dataset, &draft.source, draft.at);
        self.add(id, draft, ClientFrom::Agent)
    }

    /// Adds one exchange under an id the dataset recorded (a gateway's
    /// minted ULID), with the same checks as [`WorldBuilder::exchange`].
    pub fn recorded_exchange(
        &mut self,
        id: ExchangeId,
        draft: ExchangeDraft,
    ) -> Result<ExchangeId, CorpusError> {
        self.add(id, draft, ClientFrom::Agent)
    }

    /// Adds one exchange under an id and with a client the dataset
    /// recorded, both kept as they are: the client's credential, vendor,
    /// model, session and turn replace the agent's and the draft's (the
    /// draft's `model`, `session` and `turn` are not used). The checks are
    /// [`WorldBuilder::exchange`]'s.
    pub fn recorded_exchange_with_client(
        &mut self,
        id: ExchangeId,
        client: Client,
        draft: ExchangeDraft,
    ) -> Result<ExchangeId, CorpusError> {
        self.add(id, draft, ClientFrom::Recorded(client))
    }

    fn add(
        &mut self,
        id: ExchangeId,
        draft: ExchangeDraft,
        from: ClientFrom,
    ) -> Result<ExchangeId, CorpusError> {
        if draft.agent.world != self.key {
            return Err(CorpusError::ForeignAgent {
                agent: draft.agent.key,
                agent_world: draft.agent.world,
                world: self.key.clone(),
            });
        }
        let agent_key = draft.agent.key;
        let agent = self
            .agents
            .get_mut(&agent_key)
            .ok_or_else(|| CorpusError::UnknownAgent(agent_key.clone()))?;
        if agent.driven == Driven::Scripted {
            return Err(CorpusError::ScriptedAgent(agent_key));
        }
        if let Some(last) = agent.last_at
            && last >= draft.at
        {
            return Err(CorpusError::OutOfOrder {
                agent: agent_key,
                at: last,
            });
        }
        if self.ids.contains(&id) {
            return Err(CorpusError::DuplicateExchange(id));
        }
        let client = match from {
            ClientFrom::Recorded(client) => client,
            ClientFrom::Agent => Client {
                credential: agent.credential.as_str().to_owned(),
                session: draft.session,
                turn: draft.turn,
                vendor: agent
                    .model
                    .as_deref()
                    .map(vendor_of)
                    .filter(|vendor| !vendor.is_empty()),
                model: draft.model.or_else(|| agent.model.clone()),
            },
        };
        agent.last_at = Some(draft.at);
        self.ids.insert(id);
        let request = draft.request.iter().map(Message::id).collect();
        let response = draft.response.iter().map(Message::id).collect();
        for message in draft.request.into_iter().chain(draft.response) {
            self.messages.entry(message.id()).or_insert(message);
        }
        let exchange = Exchange {
            id,
            at_us: draft.at,
            client,
            request: Request {
                messages: request,
                tools: draft.tools,
            },
            response: Response {
                messages: response,
                stop: draft.stop.map(|stop| stop.as_str().to_owned()),
                error: None,
            },
            fidelity: draft.fidelity,
            source: draft.source,
        };
        self.exchanges.push((exchange, agent_key));
        Ok(id)
    }

    /// Adds a label. `exchange_agent` rows are the builder's: one is written
    /// per exchange on [`WorldBuilder::finish`], so adding one is refused.
    pub fn label(&mut self, label: Label) -> Result<(), CorpusError> {
        if matches!(label, Label::ExchangeAgent(_)) {
            return Err(CorpusError::ExchangeAgentLabel);
        }
        self.labels.push(label);
        Ok(())
    }

    /// Adds `n` to note `name`: a count the converter reports for this
    /// world (labels it dropped, groups it did not label), written into the
    /// manifest's world entry. A note that stays at 0 is not recorded.
    pub fn add_note(&mut self, name: &str, n: u64) {
        super::add_note(&mut self.notes, name, n);
    }

    /// The world: exchanges ordered by time (ties by agent, then id), an
    /// `exchange_agent` row per exchange in that order before the added
    /// labels, all checked by [`World::new`].
    pub fn finish(mut self, coverage: Coverage) -> Result<World, CorpusError> {
        self.exchanges.sort_by(|(a, a_agent), (b, b_agent)| {
            (a.at_us, a_agent, a.id).cmp(&(b.at_us, b_agent, b.id))
        });
        let mut labels = Vec::with_capacity(self.exchanges.len() + self.labels.len());
        let mut exchanges = Vec::with_capacity(self.exchanges.len());
        for (exchange, agent) in self.exchanges {
            labels.push(Label::ExchangeAgent(ExchangeAgent {
                exchange: exchange.id,
                agent,
            }));
            exchanges.push(exchange);
        }
        labels.extend(self.labels);
        let decl = WorldDecl {
            key: self.key,
            agents: self
                .agents
                .into_iter()
                .map(|(key, agent)| AgentDecl {
                    key,
                    driven: agent.driven,
                    model: agent.model,
                })
                .collect(),
        };
        let mut world = World::new(
            self.dataset,
            decl,
            self.messages.into_values().collect(),
            exchanges,
            labels,
            coverage,
        )?;
        world.notes = self.notes;
        Ok(world)
    }
}
