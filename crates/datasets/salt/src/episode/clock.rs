//! The times of one agent's calls in one episode.
//!
//! Nothing in SALT has a time, so the virtual clock orders exchanges by
//! episode-global event ids, one paced call step per event ([`Pace`], 1 to
//! 5 s by default). Episodes follow each other: each starts at the step
//! after the previous one's last ([`episode_steps`]), and within it:
//!
//! - an exchange whose response makes a tool call happens just before that
//!   call's event `e`: step `e + 1`, sub 0;
//! - any other exchange happens after every input it saw: step `floor`,
//!   sub 1, where `floor` is one past the latest event among the delivered
//!   peer messages and tool results before it, and never before the agent's
//!   previous exchange.
//!
//! A time not after the agent's previous one is moved to one microsecond
//! past it. So a sender's exchange (at its send event) always precedes the
//! reader's exchange that first carries the delivered message.

use std::collections::BTreeMap;

use a2a_bench_corpus::clock::{ClockError, Pace};
use a2a_bench_format::time::Timestamp;

use super::Turn;
use crate::SaltError;
use crate::schema::{Episode, RawMessage, Usage};

/// Where an episode's calls fall: the pace of a step and the step the
/// episode starts at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpisodeClock {
    pub pace: Pace,
    pub start: u64,
}

/// The call steps an episode takes: every time within it is at most one
/// past its last event (step `e + 1`), and one more step separates it from
/// the next episode. An episode without events takes 2.
pub fn episode_steps(episode: &Episode) -> u64 {
    let events = episode.events.iter().map(|event| event.event_id);
    let deliveries = episode
        .channel_transcript
        .iter()
        .map(|delivery| delivery.event_id);
    events
        .chain(deliveries)
        .max()
        .map_or(2, |last| last.saturating_add(3))
}

/// What the clock reads of one agent's reconstruction.
pub struct ClockInputs<'a> {
    pub raw: &'a [RawMessage],
    /// First message of the episode's own region.
    pub start: usize,
    /// `(message index, call index)` of each region tool call → its event.
    pub call_events: &'a BTreeMap<(usize, usize), u64>,
    /// Message index of each delivered peer turn → its event.
    pub delivered: &'a BTreeMap<usize, u64>,
    /// The agent's accepted calls, in order; paired with the turns in order.
    pub usage: Vec<Usage>,
}

/// One turn per assistant message of the region, timed.
pub fn clock(inputs: ClockInputs<'_>, clock_at: EpisodeClock) -> Result<Vec<Turn>, SaltError> {
    let ClockInputs {
        raw,
        start,
        call_events,
        delivered,
        usage,
    } = inputs;
    let EpisodeClock {
        pace,
        start: episode_start,
    } = clock_at;
    let mut call_event_by_id: BTreeMap<&str, u64> = BTreeMap::new();
    for ((at, call), event) in call_events {
        if let Some(tool_call) = raw
            .get(*at)
            .and_then(|message| message.tool_calls.as_ref())
            .and_then(|calls| calls.get(*call))
        {
            call_event_by_id.insert(tool_call.id.as_str(), *event);
        }
    }
    let mut usage = usage.into_iter();
    let mut floor: u64 = 0;
    let mut last: Option<Timestamp> = None;
    let mut turns = Vec::new();
    for (at, message) in raw.iter().enumerate().skip(start) {
        if let Some(event) = delivered.get(&at) {
            floor = floor.max(event.saturating_add(1));
        }
        if message.role == "tool"
            && let Some(event) = message
                .tool_call_id
                .as_deref()
                .and_then(|id| call_event_by_id.get(id))
        {
            floor = floor.max(event.saturating_add(1));
        }
        if message.role != "assistant" {
            continue;
        }
        let own = call_events.get(&(at, 0)).copied();
        let (minor, sub) = match own {
            Some(event) if event.saturating_add(1) >= floor => (event.saturating_add(1), 0),
            _ => (floor, 1),
        };
        let step = episode_start
            .checked_add(minor)
            .ok_or(SaltError::Clock(ClockError::Major(minor)))?;
        let mut time = pace.at(step, 0, sub).map_err(SaltError::Clock)?;
        if let Some(previous) = last
            && time <= previous
        {
            time = Timestamp::from_micros(previous.as_micros().saturating_add(1));
        }
        last = Some(time);
        floor = floor.max(minor);
        turns.push(Turn {
            index: at,
            at: time,
            usage: usage.next(),
        });
    }
    Ok(turns)
}
