//! The resolver: truth rows plus the capture's in-window exchanges become
//! one bench world of labels over the gateway's exchange ids.
//!
//! Join rules (ct-eval's):
//!
//! - **Agents.** The agent key is the truth's agent name. A session id
//!   belongs to the agent its `session` row names; a session with no such
//!   row belongs to the agent the other rows name for it. An exchange
//!   belongs to its session's agent; one in a session no row names is not
//!   the world's.
//! - **Reader.** The session's exchange at ordinal `reader_turn`, checked by
//!   finding a tool result for `content.at.tool_use_id` whose text hashes
//!   to `content.blake3`. When that exchange does not hold the tool result,
//!   the session's first exchange that does is used and the row is
//!   reported (`turn_mismatch`). A tool result whose bytes do not hash right
//!   drops the row (`hash_mismatch`).
//! - **Writer.** The session's exchange at `writer_turn`, checked by its
//!   response's `PUT` tool call `writer_tool_use_id` whose `body` hashes to
//!   `content.blake3`; the same fallback. A writer that cannot be joined
//!   leaves the label without a sender exchange.
//! - **Location.** The whole text of the reader's tool result part.
//!
//! Rows become:
//!
//! | Row | Label |
//! | --- | --- |
//! | `session` | none: maps the session to its agent |
//! | `transmission` | `transmission`: channel route (the page URL, `normalized_url`), `tool_result` carrier, `construction` tier |
//! | `self_read` | `negative_control` `self_read`, writer → itself, at the read |
//! | `reread` | `negative_control` `reread`, writer → reader, at the read |
//! | `miss` | `negative_control` `miss` from every other agent of the world, at the read |
//! | `unattributed_read` | `exemption` `unknown_sender` at the read |
//! | `agent_cluster` | `agent_cluster` of kind `key_group` (agents sharing one API key, not one agent); a group of fewer than two agents is reported (`key_group_not_a_cluster`) |
//!
//! Every label is checked against the world as a reader would
//! ([`check::Checker`]); one the format refuses is reported
//! (`invalid_label`) and dropped, or, for a transmission whose sender
//! exchange is what the format refuses, kept without it.

pub(crate) mod check;
mod join;
mod rows;

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_format::ids::{AgentKey, ExchangeId};
use a2a_bench_format::labels::Label;
use serde::Serialize;

use crate::diagnostics::{Diagnostic, Diagnostics, Effect, JoinFailure, RowKind, Side};
use crate::locate::MessageIndex;
use crate::schema::KeyGroup;
use crate::sessions::Sessions;
use crate::truth_file::{DeliveryKind, Row, TruthFile};
use check::Checker;

/// Which agent each captured exchange and session belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentIndex {
    pub sessions: BTreeMap<String, AgentKey>,
    pub exchanges: BTreeMap<ExchangeId, AgentKey>,
}

impl AgentIndex {
    pub fn exchange(&self, id: ExchangeId) -> Option<&AgentKey> {
        self.exchanges.get(&id)
    }
}

/// How many labels the resolver made: ct-eval's counts, plus the key
/// groups labelled as clusters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ResolveCounts {
    pub rows: u64,
    /// `session` rows.
    pub sessions: u64,
    /// In-window exchanges in the truth's sessions: the world's traffic.
    pub exchanges: u64,
    /// Exchanges of the truth's sessions left out as outside the run
    /// window (`session_reused_outside_run`).
    pub excluded_outside_window: u64,
    pub transmissions: u64,
    pub without_sender: u64,
    pub self_reads: u64,
    pub rereads: u64,
    pub misses: u64,
    /// Miss controls: one per other agent per miss.
    pub miss_controls: u64,
    /// Unattributed reads made exemptions.
    pub unattributed: u64,
    /// `agent_cluster` rows.
    pub key_groups: u64,
    /// Key groups labelled as `agent_cluster` rows (two or more agents).
    pub clusters: u64,
    pub dropped: u64,
}

/// The agents claiming one session: those `session` rows name, and those
/// the other rows name.
#[derive(Debug, Default)]
pub(crate) struct Claims {
    started: BTreeSet<String>,
    named: BTreeSet<String>,
}

impl Claims {
    /// The session's agent: the first its `session` rows name, else the
    /// first the other rows name.
    fn owner(&self) -> Option<&String> {
        self.started.first().or_else(|| self.named.first())
    }

    /// Every agent claiming the session; more than one is a conflict.
    fn all(&self) -> BTreeSet<&String> {
        self.started.iter().chain(&self.named).collect()
    }
}

/// The truth's agents and the agents claiming each session.
pub(crate) fn agents_and_sessions(
    truth: &TruthFile,
) -> (BTreeSet<String>, BTreeMap<String, Claims>) {
    let mut agents = BTreeSet::new();
    let mut sessions: BTreeMap<String, Claims> = BTreeMap::new();
    let mut claim = |session: &str, agent: &str| {
        sessions
            .entry(session.to_owned())
            .or_default()
            .named
            .insert(agent.to_owned());
    };
    for numbered in &truth.rows {
        match &numbered.row {
            Row::Session(_) => {}
            Row::Delivery { row, .. } => {
                agents.insert(row.writer.clone());
                agents.insert(row.reader.clone());
                claim(&row.writer_session, &row.writer);
                claim(&row.reader_session, &row.reader);
            }
            Row::Miss(row) => {
                agents.insert(row.reader.clone());
                claim(&row.reader_session, &row.reader);
            }
            Row::Unattributed(row) => {
                agents.insert(row.reader.clone());
                claim(&row.reader_session, &row.reader);
            }
            Row::Cluster(row) => agents.extend(row.agents.iter().cloned()),
        }
    }
    for numbered in &truth.rows {
        if let Row::Session(row) = &numbered.row {
            agents.insert(row.agent.clone());
            sessions
                .entry(row.session.clone())
                .or_default()
                .started
                .insert(row.agent.clone());
        }
    }
    (agents, sessions)
}

/// Maps each claimed session (and its in-window exchanges) to its owner,
/// reporting sessions claimed by more than one agent.
pub(crate) fn index_agents(
    claims: BTreeMap<String, Claims>,
    sessions: &Sessions,
    diagnostics: &mut Diagnostics,
) -> Result<AgentIndex, a2a_bench_format::ids::InvalidKey> {
    let mut index = AgentIndex::default();
    for (session, claimed) in claims {
        let all = claimed.all();
        if all.len() > 1 {
            diagnostics.push(Diagnostic {
                line: None,
                row: None,
                side: Side::Row,
                failure: JoinFailure::SessionConflict {
                    session: session.clone(),
                    agents: all.into_iter().cloned().collect(),
                },
                effect: Effect::Noted,
            });
        }
        if let Some(owner) = claimed.owner() {
            let key = AgentKey::new(owner.clone())?;
            if let Some(found) = sessions.get(&session) {
                for exchange in &found.exchanges {
                    index.exchanges.insert(exchange.id, key.clone());
                }
            }
            index.sessions.insert(session, key);
        }
    }
    Ok(index)
}

/// What the resolver made of the truth's rows.
pub(crate) struct Made {
    pub labels: Vec<Label>,
    pub key_groups: Vec<KeyGroup>,
    pub counts: ResolveCounts,
}

/// Where the resolver reads from and reports to.
pub(crate) struct Resolver<'a> {
    pub file: &'a str,
    pub sessions: &'a Sessions,
    pub messages: &'a MessageIndex,
    pub checker: &'a mut Checker,
    pub diagnostics: &'a mut Diagnostics,
}

impl Resolver<'_> {
    fn report(
        &mut self,
        line: usize,
        row: RowKind,
        side: Side,
        failure: JoinFailure,
        effect: Effect,
    ) {
        self.diagnostics.push(Diagnostic {
            line: Some(line),
            row: Some(row),
            side,
            failure,
            effect,
        });
    }

    /// Resolves every row of `truth` against the world's agents `names`.
    pub(crate) fn resolve(&mut self, truth: &TruthFile, names: &BTreeSet<String>) -> Made {
        let mut counts = ResolveCounts::default();
        let mut labels = Vec::new();
        let mut key_groups = Vec::new();
        for numbered in &truth.rows {
            counts.rows += 1;
            let line = numbered.line;
            match &numbered.row {
                Row::Session(row) => {
                    counts.sessions += 1;
                    if self.sessions.get(&row.session).is_none() {
                        self.report(
                            line,
                            RowKind::Session,
                            Side::Row,
                            JoinFailure::UnknownSession {
                                session: row.session.clone(),
                            },
                            Effect::Noted,
                        );
                    }
                }
                Row::Delivery { kind, row } => match self.delivery(*kind, row, line) {
                    Some(made) => {
                        match *kind {
                            DeliveryKind::Transmission => {
                                counts.transmissions += 1;
                                if let Label::Transmission(expected) = &made
                                    && expected.fields().sender_exchange.is_none()
                                {
                                    counts.without_sender += 1;
                                }
                            }
                            DeliveryKind::SelfRead => counts.self_reads += 1,
                            DeliveryKind::Reread => counts.rereads += 1,
                        }
                        labels.push(made);
                    }
                    None => counts.dropped += 1,
                },
                Row::Miss(row) => {
                    let made = self.miss(row, line, names);
                    if made.is_empty() {
                        counts.dropped += 1;
                    } else {
                        counts.misses += 1;
                        counts.miss_controls += made.len() as u64;
                        labels.extend(made);
                    }
                }
                Row::Unattributed(row) => match self.unattributed(row, line) {
                    Some(made) => {
                        counts.unattributed += 1;
                        labels.push(made);
                    }
                    None => counts.dropped += 1,
                },
                Row::Cluster(row) => {
                    counts.key_groups += 1;
                    if let Some(made) = self.cluster(row, line) {
                        counts.clusters += 1;
                        labels.push(made);
                    }
                    key_groups.push(row.clone());
                }
            }
        }
        Made {
            labels,
            key_groups,
            counts,
        }
    }
}
