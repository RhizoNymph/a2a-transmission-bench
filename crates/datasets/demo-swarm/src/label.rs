//! Labelling one capture from its truth file: the run window, the session
//! index, the agent map, the resolver, and the checked world.
//!
//! ```text
//! truth ─▶ RunWindow::of ─▶ window::split(capture.exchanges) ─▶ inside ─▶ Sessions::index
//!                                                              └▶ reused (truth sessions) ─▶ session_reused_outside_run
//! truth ─▶ agents_and_sessions ─▶ index_agents(sessions) ─▶ AgentIndex (session, exchange → agent)
//! world: the truth's agents (model-driven), the indexed exchanges sorted by (at_us, agent, id),
//!        the messages they use; exchange_agent rows from the index
//! Resolver(truth rows) ─▶ labels, each checked against the world ─▶ World::new (all checks again)
//! ```

use std::collections::BTreeSet;

use a2a_bench_corpus::world::{CorpusError, World};
use a2a_bench_format::check::{InputError, WorldInputs};
use a2a_bench_format::exchange::{AgentDecl, Driven, Exchange, WorldDecl};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, InvalidKey, MessageId, WorldKey};
use a2a_bench_format::labels::{ExchangeAgent, Label, Tier};
use a2a_bench_format::message::Message;
use serde::Serialize;

use crate::capture::Capture;
use crate::diagnostics::{Diagnostic, Diagnostics, Effect, JoinFailure, Side};
use crate::locate::MessageIndex;
use crate::resolve::check::Checker;
use crate::resolve::{AgentIndex, ResolveCounts, Resolver, agents_and_sessions, index_agents};
use crate::schema::KeyGroup;
use crate::sessions::Sessions;
use crate::truth_file::TruthFile;
use crate::window::{self, Margins, RunWindow};
use crate::{MODEL, Options};

/// Why a capture could not be labelled at all. A row that fails to join
/// is a diagnostic, never this.
#[derive(Debug, thiserror::Error)]
pub enum LabelError {
    #[error("a truth name is not a key: {0}")]
    Key(#[from] InvalidKey),
    #[error("the labelled world's inputs: {0}")]
    Inputs(#[from] InputError),
    #[error(transparent)]
    World(Box<CorpusError>),
}

impl From<CorpusError> for LabelError {
    fn from(error: CorpusError) -> Self {
        Self::World(Box::new(error))
    }
}

/// What the capture held and what of it became the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct CaptureCounts {
    /// Exchanges in the capture.
    pub exchanges: u64,
    /// Those that started outside the run window, whatever their session.
    pub outside_window: u64,
    /// In-window exchanges with no session.
    pub without_session: u64,
    /// In-window exchanges of sessions no truth row names.
    pub other_sessions: u64,
    /// Messages in the capture, and those the world's exchanges use.
    pub messages: u64,
    pub world_messages: u64,
}

/// A labelled run: the checked world and everything reported on the way.
#[derive(Debug)]
pub struct Labelled {
    pub world: World,
    /// The run window the capture was cut to.
    pub window: RunWindow,
    pub resolved: ResolveCounts,
    pub capture: CaptureCounts,
    pub agents: AgentIndex,
    /// Every `agent_cluster` row, labelled or not.
    pub key_groups: Vec<KeyGroup>,
    pub diagnostics: Diagnostics,
}

/// The report written beside the labels (`diagnostics.json`).
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsReport<'a> {
    pub window: RunWindow,
    pub margins: Margins,
    pub resolved: ResolveCounts,
    pub capture: CaptureCounts,
    pub key_groups: usize,
    pub table: Vec<crate::diagnostics::DiagnosticCount>,
    pub diagnostics: &'a [Diagnostic],
}

impl Labelled {
    pub fn report(&self, margins: Margins) -> DiagnosticsReport<'_> {
        DiagnosticsReport {
            window: self.window,
            margins,
            resolved: self.resolved,
            capture: self.capture,
            key_groups: self.key_groups.len(),
            table: self.diagnostics.table(),
            diagnostics: &self.diagnostics.entries,
        }
    }
}

/// Labels `capture` from `truth`. `truth_name` names the truth file in the
/// labels' source references; `dataset` is the truth scenario's.
pub fn label(
    truth: &TruthFile,
    truth_name: &str,
    capture: Capture,
    options: &Options,
) -> Result<Labelled, LabelError> {
    let dataset = truth.header.scenario().dataset()?;
    let world = WorldKey::new(truth.header.world.clone())?;
    let mut diagnostics = Diagnostics::default();
    let mut counts = CaptureCounts {
        exchanges: capture.exchanges.len() as u64,
        messages: capture.messages.len() as u64,
        ..CaptureCounts::default()
    };

    let run_window = RunWindow::of(truth, options.margins());
    let split = window::split(
        capture.exchanges,
        run_window,
        &window::truth_sessions(truth),
    );
    counts.outside_window = split.outside as u64;
    let sessions = Sessions::index(split.inside);
    counts.without_session = sessions.without_session as u64;

    let (names, claims) = agents_and_sessions(truth);
    let index = index_agents(claims, &sessions, &mut diagnostics)?;
    let owned: u64 = sessions
        .iter()
        .filter(|(session, _)| index.sessions.contains_key(*session))
        .map(|(_, session)| session.exchanges.len() as u64)
        .sum();
    let in_sessions: u64 = sessions
        .iter()
        .map(|(_, session)| session.exchanges.len() as u64)
        .sum();
    counts.other_sessions = in_sessions - owned;
    turn_check(&sessions, &index, &mut diagnostics);

    let decl = WorldDecl {
        key: world.clone(),
        agents: names
            .iter()
            .map(|name| {
                Ok(AgentDecl {
                    key: AgentKey::new(name.clone())?,
                    driven: Driven::Model,
                    model: Some(MODEL.to_owned()),
                })
            })
            .collect::<Result<_, InvalidKey>>()?,
    };
    let mut exchanges: Vec<(Exchange, AgentKey)> = sessions
        .iter()
        .flat_map(|(_, session)| session.exchanges.iter())
        .filter_map(|exchange| {
            index
                .exchange(exchange.id)
                .map(|agent| (exchange.clone(), agent.clone()))
        })
        .collect();
    exchanges.sort_by(|(a, a_agent), (b, b_agent)| {
        (a.at_us, a_agent, a.id).cmp(&(b.at_us, b_agent, b.id))
    });
    let agent_rows: Vec<Label> = exchanges
        .iter()
        .map(|(exchange, agent)| {
            Label::ExchangeAgent(ExchangeAgent {
                exchange: exchange.id,
                agent: agent.clone(),
            })
        })
        .collect();
    let exchanges: Vec<Exchange> = exchanges
        .into_iter()
        .map(|(exchange, _)| exchange)
        .collect();
    let messages = used_messages(capture.messages, &exchanges);
    counts.world_messages = messages.len() as u64;
    let message_index: MessageIndex = messages
        .iter()
        .map(|message| (message.id(), message.clone()))
        .collect();

    let inputs = WorldInputs::new(&world, messages.clone(), decl.clone(), exchanges.clone())?;
    let mut checker = Checker::new(inputs, agent_rows);
    let made = Resolver {
        file: truth_name,
        sessions: &sessions,
        messages: &message_index,
        checker: &mut checker,
        diagnostics: &mut diagnostics,
    }
    .resolve(truth, &names);

    let mut resolved = made.counts;
    resolved.exchanges = index.exchanges.len() as u64;
    resolved.excluded_outside_window = split.reused.len() as u64;
    for reused in split.reused {
        diagnostics.push(Diagnostic {
            line: None,
            row: None,
            side: Side::Row,
            failure: JoinFailure::SessionReusedOutsideRun {
                session: reused.session,
                exchange: reused.exchange,
            },
            effect: Effect::Excluded,
        });
    }

    let mut labels = checker.into_rows();
    labels.extend(made.labels);
    let world = World::new(
        dataset,
        decl,
        messages,
        exchanges,
        labels,
        Coverage::Complete {
            tier: Tier::Construction,
        },
    )?;
    tracing::info!(
        dataset = %world.dataset(),
        world = %world.key(),
        exchanges = resolved.exchanges,
        transmissions = resolved.transmissions,
        dropped = resolved.dropped,
        diagnostics = diagnostics.len(),
        "demo-swarm run labelled"
    );
    Ok(Labelled {
        world,
        window: run_window,
        resolved,
        capture: counts,
        agents: index,
        key_groups: made.key_groups,
        diagnostics,
    })
}

/// The messages `exchanges` use, each once, in capture order.
fn used_messages(messages: Vec<Message>, exchanges: &[Exchange]) -> Vec<Message> {
    let used: BTreeSet<MessageId> = exchanges
        .iter()
        .flat_map(|exchange| {
            exchange
                .request
                .messages
                .iter()
                .chain(&exchange.response.messages)
        })
        .copied()
        .collect();
    let mut seen = BTreeSet::new();
    messages
        .into_iter()
        .filter(|message| used.contains(&message.id()) && seen.insert(message.id()))
        .collect()
}

/// Reports every world exchange whose `client.turn` is not its ordinal.
fn turn_check(sessions: &Sessions, index: &AgentIndex, diagnostics: &mut Diagnostics) {
    for (name, session) in sessions.iter() {
        if !index.sessions.contains_key(name) {
            continue;
        }
        for (ordinal, exchange) in session.exchanges.iter().enumerate() {
            let ordinal = u32::try_from(ordinal).unwrap_or(u32::MAX);
            if let Some(client_turn) = exchange.client.turn
                && client_turn != ordinal
            {
                diagnostics.push(Diagnostic {
                    line: None,
                    row: None,
                    side: Side::Row,
                    failure: JoinFailure::ClientTurnMismatch {
                        session: name.clone(),
                        exchange: exchange.id,
                        client_turn,
                        ordinal,
                    },
                    effect: Effect::Noted,
                });
            }
        }
    }
}
