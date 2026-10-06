//! A detector's agents as the world's true agents.

use std::collections::BTreeMap;

use a2a_bench_format::ids::{AgentKey, DetectorAgent, ExchangeId};
use a2a_bench_format::labels::Label;
use a2a_bench_format::predictions::Prediction as Row;

/// Why a detector's attribution cannot be read as the world's agents.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentMapError {
    /// One detector agent holds exchanges of two true agents: its evidence
    /// could not be told apart, so the world fails.
    #[error("detector agent {agent} holds exchanges of agents {first} and {second}")]
    Merged {
        agent: DetectorAgent,
        first: AgentKey,
        second: AgentKey,
    },
    /// An attributed exchange has no true agent (the world's labels assign
    /// every exchange, so the predictions are not this world's).
    #[error("detector agent {agent} holds exchange {exchange}, which no agent made")]
    UnknownExchange {
        agent: DetectorAgent,
        exchange: ExchangeId,
    },
}

/// Which agent made each exchange: the world's `exchange_agent` rows.
pub fn exchange_agents(labels: &[Label]) -> BTreeMap<ExchangeId, AgentKey> {
    labels
        .iter()
        .filter_map(|label| match label {
            Label::ExchangeAgent(row) => Some((row.exchange, row.agent.clone())),
            _ => None,
        })
        .collect()
}

/// Each detector agent's true agent. A split (several detector agents for
/// one true agent) is fine; a merge is refused. Unattributed agents, and
/// attributed ones holding no exchange, have none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentMap {
    agents: BTreeMap<DetectorAgent, AgentKey>,
}

impl AgentMap {
    /// From the world's exchange owners and a detector's rows (its
    /// `attribution` rows; others are ignored), in row order.
    pub fn from_rows(
        owners: &BTreeMap<ExchangeId, AgentKey>,
        rows: &[Row],
    ) -> Result<Self, AgentMapError> {
        let mut agents: BTreeMap<DetectorAgent, AgentKey> = BTreeMap::new();
        for row in rows {
            let Row::Attribution(attribution) = row else {
                continue;
            };
            for exchange in &attribution.exchanges {
                let owner = owners
                    .get(exchange)
                    .ok_or_else(|| AgentMapError::UnknownExchange {
                        agent: attribution.agent.clone(),
                        exchange: *exchange,
                    })?;
                match agents.get(&attribution.agent) {
                    Some(known) if known != owner => {
                        return Err(AgentMapError::Merged {
                            agent: attribution.agent.clone(),
                            first: known.clone(),
                            second: owner.clone(),
                        });
                    }
                    Some(_) => {}
                    None => {
                        agents.insert(attribution.agent.clone(), owner.clone());
                    }
                }
            }
        }
        Ok(Self { agents })
    }

    /// The true agent behind `agent`, if the detector tied it to exchanges.
    pub fn get(&self, agent: &DetectorAgent) -> Option<&AgentKey> {
        self.agents.get(agent)
    }
}
