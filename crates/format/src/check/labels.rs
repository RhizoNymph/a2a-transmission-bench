//! A world's labels against its inputs.

use std::collections::{BTreeMap, BTreeSet};

use super::{LocationError, WorldInputs};
use crate::exchange::Driven;
use crate::ids::{AgentKey, ExchangeId, LabelId};
use crate::labels::Label;
use crate::location::Location;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelError {
    #[error("label {label}: agent {agent} is not declared")]
    UnknownAgent { label: String, agent: AgentKey },
    #[error("exchange {0} has no exchange_agent row")]
    Unassigned(ExchangeId),
    #[error("exchange {0} has two exchange_agent rows")]
    AssignedTwice(ExchangeId),
    #[error("exchange {exchange} is assigned to scripted agent {agent}")]
    Scripted {
        exchange: ExchangeId,
        agent: AgentKey,
    },
    #[error("agent {agent}'s exchange {exchange} is not later than its previous one")]
    AgentOutOfOrder {
        agent: AgentKey,
        exchange: ExchangeId,
    },
    #[error("label {label}: exchange {exchange} is {actual}'s, the label says {expected}'s")]
    WrongAgent {
        label: LabelId,
        exchange: ExchangeId,
        expected: AgentKey,
        actual: AgentKey,
    },
    #[error("label {label}: the sender's exchange {sender} is not before the reader's {reader}")]
    SenderAfterReader {
        label: LabelId,
        sender: ExchangeId,
        reader: ExchangeId,
    },
    #[error("label {label}: {source}")]
    Location {
        label: LabelId,
        source: LocationError,
    },
    #[error("label {label}: its content text is not the text at its location")]
    ContentMismatch { label: LabelId },
    #[error("label id {0} appears twice")]
    DuplicateId(LabelId),
}

struct Ctx<'a> {
    inputs: &'a WorldInputs,
    agent_of: BTreeMap<ExchangeId, AgentKey>,
    declared: BTreeSet<&'a AgentKey>,
}

impl Ctx<'_> {
    fn declared(&self, label: &LabelId, agent: &AgentKey) -> Result<(), LabelError> {
        if self.declared.contains(agent) {
            Ok(())
        } else {
            Err(LabelError::UnknownAgent {
                label: label.to_string(),
                agent: agent.clone(),
            })
        }
    }

    fn owned_by(
        &self,
        label: &LabelId,
        exchange: ExchangeId,
        agent: &AgentKey,
    ) -> Result<(), LabelError> {
        let actual = self
            .agent_of
            .get(&exchange)
            .ok_or_else(|| LabelError::Location {
                label: label.clone(),
                source: LocationError::UnknownExchange(exchange),
            })?;
        if actual == agent {
            Ok(())
        } else {
            Err(LabelError::WrongAgent {
                label: label.clone(),
                exchange,
                expected: agent.clone(),
                actual: actual.clone(),
            })
        }
    }

    fn resolves(&self, label: &LabelId, location: &Location) -> Result<String, LabelError> {
        self.inputs
            .text_at(location)
            .map(|text| text.into_owned())
            .map_err(|source| LabelError::Location {
                label: label.clone(),
                source,
            })
    }
}

/// Checks `labels` against `inputs`: every exchange belongs to exactly one
/// declared, model-driven agent and each agent's exchanges strictly
/// increase in time; every label names declared agents, exchanges of the
/// right agents, locations that resolve, and content text equal to the text
/// at its location.
pub fn check_labels(inputs: &WorldInputs, labels: &[Label]) -> Result<(), LabelError> {
    let declared: BTreeSet<&AgentKey> = inputs
        .decl()
        .agents
        .iter()
        .map(|agent| &agent.key)
        .collect();
    let scripted: BTreeSet<&AgentKey> = inputs
        .decl()
        .agents
        .iter()
        .filter(|agent| agent.driven == Driven::Scripted)
        .map(|agent| &agent.key)
        .collect();
    let mut agent_of = BTreeMap::new();
    for label in labels {
        if let Label::ExchangeAgent(row) = label {
            if !declared.contains(&row.agent) {
                return Err(LabelError::UnknownAgent {
                    label: "exchange_agent".to_owned(),
                    agent: row.agent.clone(),
                });
            }
            if scripted.contains(&row.agent) {
                return Err(LabelError::Scripted {
                    exchange: row.exchange,
                    agent: row.agent.clone(),
                });
            }
            if inputs.exchange(row.exchange).is_none() {
                return Err(LabelError::Location {
                    label: LabelId::new("exchange_agent")
                        .map_err(|_| LabelError::Unassigned(row.exchange))?,
                    source: LocationError::UnknownExchange(row.exchange),
                });
            }
            if agent_of.insert(row.exchange, row.agent.clone()).is_some() {
                return Err(LabelError::AssignedTwice(row.exchange));
            }
        }
    }
    let mut last: BTreeMap<&AgentKey, crate::time::Timestamp> = BTreeMap::new();
    for exchange in inputs.exchanges() {
        let agent = agent_of
            .get(&exchange.id)
            .ok_or(LabelError::Unassigned(exchange.id))?;
        if last
            .get(agent)
            .is_some_and(|before| exchange.at_us <= *before)
        {
            return Err(LabelError::AgentOutOfOrder {
                agent: agent.clone(),
                exchange: exchange.id,
            });
        }
        last.insert(agent, exchange.at_us);
    }
    let ctx = Ctx {
        inputs,
        agent_of,
        declared,
    };
    let mut ids = BTreeSet::new();
    for label in labels {
        let id = match label {
            Label::ExchangeAgent(_) => continue,
            Label::Transmission(row) => check_transmission(&ctx, row.fields())?,
            Label::AccessOnly(row) => check_transmission(&ctx, row.fields())?,
            Label::NegativeControl(row) => {
                let fields = row.fields();
                ctx.declared(&fields.id, &fields.from)?;
                ctx.declared(&fields.id, &fields.to)?;
                if let Some(reader) = fields.reader_exchange {
                    ctx.owned_by(&fields.id, reader, &fields.to)?;
                }
                if let Some(at) = &fields.at {
                    ctx.resolves(&fields.id, at)?;
                    ctx.owned_by(&fields.id, at.exchange, &fields.to)?;
                }
                if let Some(origin) = &fields.origin {
                    ctx.resolves(&fields.id, origin)?;
                }
                &fields.id
            }
            Label::Exemption(row) => {
                let fields = row.fields();
                ctx.declared(&fields.id, &fields.to)?;
                ctx.owned_by(&fields.id, fields.reader_exchange, &fields.to)?;
                ctx.resolves(&fields.id, &fields.at)?;
                &fields.id
            }
            Label::AgentCluster(row) => {
                let fields = row.fields();
                for agent in &fields.agents {
                    ctx.declared(&fields.id, agent)?;
                }
                &fields.id
            }
        };
        if !ids.insert(id.clone()) {
            return Err(LabelError::DuplicateId(id.clone()));
        }
    }
    Ok(())
}

fn check_transmission<'a>(
    ctx: &Ctx<'_>,
    fields: &'a crate::labels::TransmissionFields,
) -> Result<&'a LabelId, LabelError> {
    let id = &fields.id;
    ctx.declared(id, &fields.from)?;
    ctx.declared(id, &fields.to)?;
    ctx.owned_by(id, fields.reader_exchange, &fields.to)?;
    if let Some(sender) = fields.sender_exchange {
        ctx.owned_by(id, sender, &fields.from)?;
        let at = |exchange| ctx.inputs.exchange(exchange).map(|e| e.at_us);
        if at(sender) >= at(fields.reader_exchange) {
            return Err(LabelError::SenderAfterReader {
                label: id.clone(),
                sender,
                reader: fields.reader_exchange,
            });
        }
    }
    let text = ctx.resolves(id, &fields.content.at)?;
    if text != fields.content.text {
        return Err(LabelError::ContentMismatch { label: id.clone() });
    }
    Ok(id)
}
