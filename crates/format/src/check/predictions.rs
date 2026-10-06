//! A detector's predictions against the world's inputs.

use std::collections::{BTreeMap, BTreeSet};

use super::{LocationError, WorldInputs};
use crate::ids::{DetectorAgent, ExchangeId, TransmissionRef};
use crate::location::Location;
use crate::predictions::Prediction;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PredictionError {
    #[error("detector agent {0} is attributed twice")]
    AgentTwice(DetectorAgent),
    #[error("exchange {exchange} is attributed to both {first} and {second}")]
    ExchangeTwice {
        exchange: ExchangeId,
        first: DetectorAgent,
        second: DetectorAgent,
    },
    #[error("attribution of {agent}: exchange {exchange} is not in the world")]
    UnknownExchange {
        agent: DetectorAgent,
        exchange: ExchangeId,
    },
    #[error(
        "transmission {transmission}: detector agent {agent} has no attribution or unattributed row"
    )]
    UnknownAgent {
        transmission: TransmissionRef,
        agent: DetectorAgent,
    },
    #[error("transmission {transmission}: {source}")]
    Location {
        transmission: TransmissionRef,
        source: LocationError,
    },
    #[error(
        "transmission {transmission}: a location is in {location}, its exchange field says {stated}"
    )]
    ExchangeMismatch {
        transmission: TransmissionRef,
        location: ExchangeId,
        stated: ExchangeId,
    },
    #[error("transmission {0} appears twice")]
    DuplicateTransmission(TransmissionRef),
}

/// Checks `predictions` against `inputs`: attributions name world exchanges,
/// each exchange at most once; every agent evidence names is attributed or
/// declared unattributed; every location resolves and sits in the exchange
/// its row names.
pub fn check_predictions(
    inputs: &WorldInputs,
    predictions: &[Prediction],
) -> Result<(), PredictionError> {
    let mut agents = BTreeSet::new();
    let mut owner: BTreeMap<ExchangeId, &DetectorAgent> = BTreeMap::new();
    for prediction in predictions {
        match prediction {
            Prediction::Attribution(row) => {
                if !agents.insert(&row.agent) {
                    return Err(PredictionError::AgentTwice(row.agent.clone()));
                }
                for exchange in &row.exchanges {
                    if inputs.exchange(*exchange).is_none() {
                        return Err(PredictionError::UnknownExchange {
                            agent: row.agent.clone(),
                            exchange: *exchange,
                        });
                    }
                    if let Some(first) = owner.insert(*exchange, &row.agent) {
                        return Err(PredictionError::ExchangeTwice {
                            exchange: *exchange,
                            first: first.clone(),
                            second: row.agent.clone(),
                        });
                    }
                }
            }
            Prediction::Unattributed(row) => {
                if !agents.insert(&row.agent) {
                    return Err(PredictionError::AgentTwice(row.agent.clone()));
                }
            }
            Prediction::Transmission(_) => {}
        }
    }
    let mut ids = BTreeSet::new();
    for prediction in predictions {
        let Prediction::Transmission(transmission) = prediction else {
            continue;
        };
        let fields = transmission.fields();
        let id = &fields.id;
        if !ids.insert(id) {
            return Err(PredictionError::DuplicateTransmission(id.clone()));
        }
        let known = |agent: &DetectorAgent| {
            if agents.contains(agent) {
                Ok(())
            } else {
                Err(PredictionError::UnknownAgent {
                    transmission: id.clone(),
                    agent: agent.clone(),
                })
            }
        };
        let resolves = |location: &Location, stated: Option<ExchangeId>| {
            if let Some(stated) = stated
                && stated != location.exchange
            {
                return Err(PredictionError::ExchangeMismatch {
                    transmission: id.clone(),
                    location: location.exchange,
                    stated,
                });
            }
            inputs
                .text_at(location)
                .map(|_| ())
                .map_err(|source| PredictionError::Location {
                    transmission: id.clone(),
                    source,
                })
        };
        for evidence in &fields.matches {
            known(&evidence.from)?;
            known(&evidence.to)?;
            resolves(&evidence.read_at, Some(evidence.reader_exchange))?;
            if let Some(origin) = &evidence.origin_at {
                resolves(origin, None)?;
            }
        }
        for access in &fields.co_access {
            known(&access.from)?;
            known(&access.to)?;
            resolves(&access.read_at, Some(access.reader_exchange))?;
            resolves(&access.write_at, Some(access.write_exchange))?;
        }
    }
    Ok(())
}
