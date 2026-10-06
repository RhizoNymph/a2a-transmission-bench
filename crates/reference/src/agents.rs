//! The reference's agents: one per client credential.
//!
//! The reference has no identity inference. It attributes every exchange to
//! the detector agent named by the exchange's `client.credential`, so
//! agents that share a credential (demo-swarm key groups) are one detector
//! agent to it, and what they pass each other is a self-read it never
//! reports. This is the reference's own naive choice, not the bench's rule:
//! another detector attributes however it likes.

use std::collections::BTreeMap;

use a2a_bench_format::check::WorldInputs;
use a2a_bench_format::ids::{DetectorAgent, ExchangeId};
use a2a_bench_format::predictions::Attribution;

use crate::error::ReferenceError;

/// A world's agents, by credential, numbered in order of first exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agents {
    names: Vec<DetectorAgent>,
    exchanges: Vec<Vec<ExchangeId>>,
    /// Each exchange's agent, in world order.
    of_exchange: Vec<usize>,
}

impl Agents {
    /// Numbers the credentials of `inputs`' exchanges.
    pub fn of(inputs: &WorldInputs) -> Result<Self, ReferenceError> {
        let mut by_credential: BTreeMap<&str, usize> = BTreeMap::new();
        let mut agents = Self {
            names: Vec::new(),
            exchanges: Vec::new(),
            of_exchange: Vec::with_capacity(inputs.exchanges().len()),
        };
        for exchange in inputs.exchanges() {
            let credential = exchange.client.credential.as_str();
            let agent = match by_credential.get(credential) {
                Some(agent) => *agent,
                None => {
                    let name = DetectorAgent::new(credential).map_err(|source| {
                        ReferenceError::Credential {
                            credential: credential.to_owned(),
                            source,
                        }
                    })?;
                    let agent = agents.names.len();
                    agents.names.push(name);
                    agents.exchanges.push(Vec::new());
                    by_credential.insert(credential, agent);
                    agent
                }
            };
            if let Some(exchanges) = agents.exchanges.get_mut(agent) {
                exchanges.push(exchange.id);
            }
            agents.of_exchange.push(agent);
        }
        Ok(agents)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Each exchange's agent, in world order.
    pub fn assignments(&self) -> &[usize] {
        &self.of_exchange
    }

    pub fn name(&self, agent: usize) -> Option<&DetectorAgent> {
        self.names.get(agent)
    }

    /// One attribution row per agent, by name; exchanges in world order.
    pub fn attributions(&self) -> Vec<Attribution> {
        let mut rows: Vec<Attribution> = self
            .names
            .iter()
            .zip(&self.exchanges)
            .map(|(agent, exchanges)| Attribution {
                agent: agent.clone(),
                exchanges: exchanges.clone(),
            })
            .collect();
        rows.sort_by(|a, b| a.agent.cmp(&b.agent));
        rows
    }
}
