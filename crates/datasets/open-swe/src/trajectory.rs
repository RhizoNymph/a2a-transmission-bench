//! One row as a trajectory, and worlds mixed from rows.

use a2a_bench_corpus::clock::{ClockError, Pace};
use a2a_bench_corpus::helpers::background::{BackgroundWorld, Call, Trajectory};
use a2a_bench_corpus::helpers::chat::{ChatMessage, convert};
use a2a_bench_corpus::world::{CorpusError, StopReason, World};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::ids::{DatasetId, SourceRef, WorldKey};
use a2a_bench_format::time::Timestamp;

use crate::DATASET;
use crate::error::OpenSweError;
use crate::files::Shard;
use crate::schema::OpenSweRow;

/// The calls of one trajectory's `messages`, call `i` (counting assistant
/// messages) at `clock(i)`: call `i` sends every message before its
/// assistant message and receives that message.
pub fn calls(
    messages: &[ChatMessage],
    file: &str,
    row: usize,
    clock: impl Fn(u64) -> Result<Timestamp, ClockError>,
) -> Result<Vec<Call>, OpenSweError> {
    let converted = convert(messages).map_err(|source| OpenSweError::Chat {
        file: file.to_owned(),
        row,
        source,
    })?;
    let mut calls = Vec::new();
    for (index, (message, response)) in messages.iter().zip(&converted).enumerate() {
        if !message.is_assistant() {
            continue;
        }
        let ordinal = calls.len() as u64;
        calls.push(Call {
            at: clock(ordinal).map_err(OpenSweError::Clock)?,
            request: converted.get(..index).unwrap_or_default().to_vec(),
            response: response.clone(),
            stop: StopReason::for_calls(!message.calls().is_empty()),
            fidelity: Fidelity::Reconstructed,
            source: SourceRef::new(file, format!("/rows/{row}/messages/{index}")),
        });
    }
    Ok(calls)
}

/// The agent name of a shard's row: unique across shards.
pub fn agent_name(shard: &Shard, row: usize) -> String {
    format!("{}/{}/{}/{row}", shard.harness, shard.model, shard.dataset)
}

/// One row as a trajectory in slot `slot` of its world.
pub fn trajectory(
    shard: &Shard,
    row: usize,
    record: &OpenSweRow,
    slot: u64,
    pace: Pace,
) -> Result<Trajectory, OpenSweError> {
    Ok(Trajectory {
        name: agent_name(shard, row),
        model: shard.model.clone(),
        group: record.repo.clone(),
        calls: calls(&record.messages, &shard.relative, row, |call| {
            pace.at(call, slot, 0)
        })?,
    })
}

/// One world from rows, in slot order, calls `pace` apart.
pub fn mix(
    key: &str,
    rows: &[(Shard, usize, OpenSweRow)],
    pace: Pace,
) -> Result<World, OpenSweError> {
    let dataset = DatasetId::new(DATASET).map_err(CorpusError::from)?;
    let key = WorldKey::new(key).map_err(CorpusError::from)?;
    let mut world = BackgroundWorld::new(dataset, key);
    for (slot, (shard, row, record)) in rows.iter().enumerate() {
        world.add(trajectory(shard, *row, record, slot as u64, pace)?)?;
    }
    Ok(world.finish()?)
}
