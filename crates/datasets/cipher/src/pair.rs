//! Planning one pair: its payload, key and delivery.

use a2a_bench_corpus::helpers::rng::SplitMix64;

use crate::codec::{Cipher, CipherKind};
use crate::pools::Pool;

/// Where the encoded payload reaches the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    UserTurn,
    ToolResult,
}

/// One planned pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub cipher: Cipher,
    pub index: usize,
    pub pool: String,
    pub line: usize,
    pub payload: String,
    pub delivery: Delivery,
}

impl Pair {
    /// Pair `index` of `kind`: pool `index mod pools`, payload and key drawn
    /// from the stream of `seed` and `<kind>/<index>`; `None` without pools.
    pub fn plan(kind: CipherKind, index: usize, pools: &[Pool], seed: u64) -> Option<Self> {
        let mut rng = SplitMix64::derived(seed, &format!("{kind}/{index}"));
        let pool = pools.get(index % pools.len().max(1))?;
        let line = rng.index(pool.payloads.len())?;
        let cipher = kind.instantiate(&mut rng);
        Some(Self {
            cipher,
            index,
            pool: pool.name.clone(),
            line,
            payload: pool.payloads.get(line)?.clone(),
            delivery: if index.is_multiple_of(2) {
                Delivery::UserTurn
            } else {
                Delivery::ToolResult
            },
        })
    }

    /// `<kind>-<index, three digits>`.
    pub fn world_key(&self) -> String {
        format!("{}-{:03}", self.cipher.kind(), self.index)
    }
}
