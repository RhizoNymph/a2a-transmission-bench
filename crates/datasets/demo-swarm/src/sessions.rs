//! A capture's exchanges by harness session (`client.session`), each
//! session's exchanges ordered by (`at_us`, id): the position in that
//! order is the session's generation-request ordinal, the truth file's
//! `turn`. ct-eval ordered the gateway's log the same way (`started_at`,
//! id), after the run window cut it.

use std::collections::{BTreeMap, BTreeSet};

use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::ids::ExchangeId;

/// One session's exchanges in ordinal order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub exchanges: Vec<Exchange>,
    /// Every credential fingerprint the session's exchanges carried; one
    /// for an agent on one key.
    pub credentials: BTreeSet<String>,
}

impl Session {
    /// The exchange at generation-request ordinal `turn`.
    pub fn at(&self, turn: u32) -> Option<&Exchange> {
        self.exchanges.get(usize::try_from(turn).ok()?)
    }

    /// The ordinal of exchange `id`.
    pub fn ordinal(&self, id: ExchangeId) -> Option<u32> {
        let at = self.exchanges.iter().position(|ex| ex.id == id)?;
        u32::try_from(at).ok()
    }
}

/// Exchanges by harness session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sessions {
    sessions: BTreeMap<String, Session>,
    /// Exchanges with no session; nothing in the truth can name them.
    pub without_session: usize,
}

impl Sessions {
    /// Groups `exchanges` by session and orders each group by
    /// (`at_us`, id).
    pub fn index(exchanges: Vec<Exchange>) -> Self {
        let mut out = Self::default();
        for exchange in exchanges {
            let Some(session) = exchange.client.session.clone() else {
                out.without_session += 1;
                continue;
            };
            let entry = out.sessions.entry(session).or_default();
            entry.credentials.insert(exchange.client.credential.clone());
            entry.exchanges.push(exchange);
        }
        for session in out.sessions.values_mut() {
            session
                .exchanges
                .sort_by_key(|exchange| (exchange.at_us, exchange.id));
        }
        out
    }

    pub fn get(&self, session: &str) -> Option<&Session> {
        self.sessions.get(session)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Session)> {
        self.sessions.iter()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}
