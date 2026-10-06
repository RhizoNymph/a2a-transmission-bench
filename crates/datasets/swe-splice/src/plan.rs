//! Planning splice `n`: its pair, write, insertion point and variant.

use a2a_bench_corpus::helpers::rng::SplitMix64;

use crate::error::SpliceError;
use crate::pool::{Pooled, insertion_points};
use crate::variant::Variant;

/// One planned splice: indices into the pool and the sender's writes.
#[derive(Debug, Clone)]
pub struct Plan {
    pub number: usize,
    pub variant: Variant,
    pub sender: usize,
    pub write: usize,
    pub reader: usize,
    pub insert_at: usize,
    pub call_id: String,
}

/// Plans splice `number` over `pool`, from the stream of `seed` and
/// `splice/<number>`: a sender with a splicable write, one of its writes, a
/// reader of another repository with an insertion point, one of those
/// points, and the read's call id; variant `number mod 4`.
pub fn plan(pool: &[Pooled], number: usize, seed: u64) -> Result<Plan, SpliceError> {
    let mut rng = SplitMix64::derived(seed, &format!("splice/{number}"));
    let writers: Vec<(usize, usize)> = pool
        .iter()
        .enumerate()
        .map(|(at, pooled)| (at, pooled.writes().len()))
        .filter(|(_, writes)| *writes > 0)
        .collect();
    let &(sender, writes) = rng.pick(&writers).ok_or(SpliceError::NoWriter(number))?;
    let write = rng.index(writes).ok_or(SpliceError::NoWriter(number))?;
    let sender_repo = &pool
        .get(sender)
        .ok_or(SpliceError::NoWriter(number))?
        .record
        .repo;
    let readers: Vec<usize> = pool
        .iter()
        .enumerate()
        .filter(|(at, pooled)| {
            *at != sender
                && pooled.record.repo != *sender_repo
                && !insertion_points(&pooled.record.messages).is_empty()
        })
        .map(|(at, _)| at)
        .collect();
    let &reader = rng.pick(&readers).ok_or(SpliceError::NoReader(number))?;
    let points = insertion_points(
        &pool
            .get(reader)
            .ok_or(SpliceError::NoReader(number))?
            .record
            .messages,
    );
    let &insert_at = rng.pick(&points).ok_or(SpliceError::NoReader(number))?;
    let call_id = format!("chatcmpl-tool-{:016x}", rng.next_u64());
    Ok(Plan {
        number,
        variant: Variant::ALL[number % Variant::ALL.len()],
        sender,
        write,
        reader,
        insert_at,
        call_id,
    })
}
