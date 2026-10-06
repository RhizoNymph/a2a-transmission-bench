//! One session as a trajectory, and worlds mixed from sessions.

use a2a_bench_corpus::clock::EPOCH_MICROS;
use a2a_bench_corpus::helpers::background::{BackgroundWorld, Call, Trajectory};
use a2a_bench_corpus::helpers::chat::{ChatMessage, convert};
use a2a_bench_corpus::world::{CorpusError, StopReason, World};
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::ids::{DatasetId, SourceRef, WorldKey};
use a2a_bench_format::message::Message;
use a2a_bench_format::time::Timestamp;

use crate::DATASET;
use crate::error::LmcacheError;
use crate::sessions::Session;

/// The repository (SWE-bench) or task a session works on:
/// `swebench__django__django-15695__claude` is `swebench/django`,
/// `gaia__L3_…__claude` is `gaia/L3_…`.
pub fn group(session: &str) -> String {
    let parts: Vec<&str> = session.split("__").collect();
    match parts.as_slice() {
        ["swebench", repo, ..] => format!("swebench/{repo}"),
        [kind, task, ..] => format!("{kind}/{task}"),
        _ => session.to_owned(),
    }
}

fn convert_row(
    session: &Session,
    row: usize,
    input: &[ChatMessage],
) -> Result<Vec<Message>, LmcacheError> {
    convert(input).map_err(|source| LmcacheError::Chat {
        file: session.file.clone(),
        row,
        source,
    })
}

/// The session's calls: every request but the last.
pub fn calls(session: &Session) -> Result<Vec<Call>, LmcacheError> {
    let mut calls = Vec::new();
    let mut elapsed = 0.0f64;
    let mut last: Option<Timestamp> = None;
    let mut current = match session.rows.first() {
        Some((row, first)) => convert_row(session, *row, &first.input)?,
        None => return Ok(calls),
    };
    for pair in session.rows.windows(2) {
        let [(row, request), (next_row, next)] = pair else {
            continue;
        };
        let following = convert_row(session, *next_row, &next.input)?;
        let messages = std::mem::replace(&mut current, following);
        elapsed += request.pre_gap.unwrap_or(0.0).max(0.0);
        let sent = request.input.len();
        let Some(offset) = next
            .input
            .iter()
            .skip(sent)
            .position(ChatMessage::is_assistant)
        else {
            tracing::debug!(file = %session.file, row, "next request appends no assistant message; call dropped");
            continue;
        };
        let reply = sent + offset;
        let (Some(response), Some(replied)) = (current.get(reply).cloned(), next.input.get(reply))
        else {
            continue;
        };
        let extends = next.input.get(..sent) == Some(request.input.as_slice());
        if !extends {
            tracing::debug!(file = %session.file, row, "history rewritten between requests; call is synthetic");
        }
        // Seconds to microseconds, as ct-eval: rounded, saturating.
        let micros = (elapsed * 1_000_000.0).round() as u64;
        let mut at = EPOCH_MICROS
            .checked_add(micros)
            .map(Timestamp::from_micros)
            .ok_or_else(|| LmcacheError::Time {
                file: session.file.clone(),
                row: *row,
            })?;
        if let Some(previous) = last
            && at <= previous
        {
            at = previous
                .as_micros()
                .checked_add(1)
                .map(Timestamp::from_micros)
                .ok_or_else(|| LmcacheError::Time {
                    file: session.file.clone(),
                    row: *row,
                })?;
        }
        last = Some(at);
        calls.push(Call {
            at,
            request: messages,
            response,
            stop: StopReason::for_calls(!replied.calls().is_empty()),
            fidelity: if extends {
                Fidelity::Reconstructed
            } else {
                Fidelity::Synthetic
            },
            source: SourceRef::new(session.file.clone(), format!("/rows/{row}")),
        });
    }
    Ok(calls)
}

/// One session as a trajectory: named by its session id, the model of its
/// first row (`unknown` without rows).
pub fn trajectory(session: &Session) -> Result<Trajectory, LmcacheError> {
    let model = session
        .rows
        .first()
        .map_or_else(|| "unknown".to_owned(), |(_, row)| row.model.clone());
    Ok(Trajectory {
        name: session.id.clone(),
        model,
        group: group(&session.id),
        calls: calls(session)?,
    })
}

/// One world from sessions.
pub fn mix(key: &str, sessions: &[Session]) -> Result<World, LmcacheError> {
    let dataset = DatasetId::new(DATASET).map_err(CorpusError::from)?;
    let key = WorldKey::new(key).map_err(CorpusError::from)?;
    let mut world = BackgroundWorld::new(dataset, key);
    for session in sessions {
        world.add(trajectory(session)?)?;
    }
    Ok(world.finish()?)
}
