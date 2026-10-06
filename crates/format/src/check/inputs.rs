//! A world's inputs: its declaration, messages and exchanges, checked
//! together.

use std::collections::{BTreeMap, BTreeSet};

use crate::exchange::{Exchange, WorldDecl};
use crate::ids::{AgentKey, ExchangeId, MessageId, WorldKey};
use crate::message::Message;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("the messages section is world {messages}, the exchanges section {exchanges}")]
    WorldMismatch {
        messages: WorldKey,
        exchanges: WorldKey,
    },
    #[error("agent {0} is declared twice")]
    DuplicateAgent(AgentKey),
    #[error("message {0} appears twice")]
    DuplicateMessage(MessageId),
    #[error("exchange {0} appears twice")]
    DuplicateExchange(ExchangeId),
    #[error("exchange {exchange} names message {message}, which the world does not hold")]
    MissingMessage {
        exchange: ExchangeId,
        message: MessageId,
    },
    #[error("message {0} is in no exchange")]
    UnusedMessage(MessageId),
    #[error("exchange {exchange} is earlier than the exchange before it")]
    OutOfOrder { exchange: ExchangeId },
}

/// One world's inputs, checked: every message once and used, every exchange
/// once, in time order, naming only messages the world holds.
#[derive(Debug, Clone)]
pub struct WorldInputs {
    decl: WorldDecl,
    messages: BTreeMap<MessageId, Message>,
    exchanges: Vec<Exchange>,
    index: BTreeMap<ExchangeId, usize>,
}

impl WorldInputs {
    pub fn new(
        messages_world: &WorldKey,
        messages: Vec<Message>,
        decl: WorldDecl,
        exchanges: Vec<Exchange>,
    ) -> Result<Self, InputError> {
        if messages_world != &decl.key {
            return Err(InputError::WorldMismatch {
                messages: messages_world.clone(),
                exchanges: decl.key.clone(),
            });
        }
        let mut agents = BTreeSet::new();
        for agent in &decl.agents {
            if !agents.insert(&agent.key) {
                return Err(InputError::DuplicateAgent(agent.key.clone()));
            }
        }
        let mut by_id = BTreeMap::new();
        for message in messages {
            let id = message.id();
            if by_id.insert(id, message).is_some() {
                return Err(InputError::DuplicateMessage(id));
            }
        }
        let mut index = BTreeMap::new();
        let mut used = BTreeSet::new();
        let mut previous = None;
        for (at, exchange) in exchanges.iter().enumerate() {
            if index.insert(exchange.id, at).is_some() {
                return Err(InputError::DuplicateExchange(exchange.id));
            }
            if previous.is_some_and(|before| exchange.at_us < before) {
                return Err(InputError::OutOfOrder {
                    exchange: exchange.id,
                });
            }
            previous = Some(exchange.at_us);
            for message in exchange
                .request
                .messages
                .iter()
                .chain(&exchange.response.messages)
            {
                if !by_id.contains_key(message) {
                    return Err(InputError::MissingMessage {
                        exchange: exchange.id,
                        message: *message,
                    });
                }
                used.insert(*message);
            }
        }
        if let Some(unused) = by_id.keys().find(|id| !used.contains(*id)) {
            return Err(InputError::UnusedMessage(*unused));
        }
        Ok(Self {
            decl,
            messages: by_id,
            exchanges,
            index,
        })
    }

    pub fn decl(&self) -> &WorldDecl {
        &self.decl
    }

    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    pub fn exchange(&self, id: ExchangeId) -> Option<&Exchange> {
        self.index.get(&id).and_then(|at| self.exchanges.get(*at))
    }

    pub fn message(&self, id: MessageId) -> Option<&Message> {
        self.messages.get(&id)
    }

    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.messages.values()
    }
}
