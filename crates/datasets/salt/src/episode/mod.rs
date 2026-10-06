//! Exchange reconstruction for one agent in one episode.
//!
//! An agent's message list only grows within an episode, and each assistant
//! message is one call's response whose request is everything before it.
//! The episode's own calls are the assistant messages after the last
//! `## Episode N: task phase` user turn: in carried-over lists (main, cross
//! model, warm-up) that skips earlier episodes, and in rewritten ones
//! (memory length and scope, sliding-window truncation) it skips the
//! replayed memory. The count is checked against the episode's accepted
//! `llm_usage` calls for the agent: equal means `reconstructed`, else
//! `synthetic` (a scripted agent is always `reconstructed`; it makes no
//! exchanges anyway). Tool calls are matched to the agent's events in
//! order; times are [`clock`]'s.

pub mod clock;

use std::collections::BTreeMap;

use a2a_bench_corpus::world::StopReason;
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::message::Message;
use a2a_bench_format::time::Timestamp;

pub use clock::{EpisodeClock, episode_steps};

use crate::SaltError;
use crate::messages::{content_text, convert};
use crate::schema::{Delivery, Episode, RawMessage, Usage};

/// A peer message as the harness delivers it: `[round=r/n][from=x][type=t]`,
/// a blank line, then the content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredTurn {
    pub from: String,
    /// Byte offset of the content in the user turn.
    pub content_start: usize,
}

/// Parses a delivered-message header.
pub fn delivered_turn(text: &str) -> Option<DeliveredTurn> {
    let break_at = text.find("\n\n")?;
    let header = text.get(..break_at)?;
    if !header.starts_with("[round=") || !header.ends_with(']') {
        return None;
    }
    let from_at = header.find("[from=")? + "[from=".len();
    let from_end = header.get(from_at..)?.find(']')? + from_at;
    Some(DeliveredTurn {
        from: header.get(from_at..from_end)?.to_owned(),
        content_start: break_at + 2,
    })
}

/// Whether a user turn opens an episode's task phase.
pub fn is_task_marker(text: &str) -> bool {
    let first_line = text.lines().next().unwrap_or_default();
    first_line.starts_with("## Episode ") && first_line.ends_with(": task phase")
}

/// One call of the agent in the episode.
#[derive(Debug, Clone)]
pub struct Turn {
    /// Index of the response in the agent's message list.
    pub index: usize,
    pub at: Timestamp,
    pub usage: Option<Usage>,
}

/// The reconstruction of one agent's episode.
#[derive(Debug, Clone)]
pub struct AgentEpisode {
    pub agent: String,
    pub raw: Vec<RawMessage>,
    pub messages: Vec<Message>,
    /// First message of this episode's own region.
    pub start: usize,
    pub turns: Vec<Turn>,
    pub fidelity: Fidelity,
    /// `(message index, call index)` of each region tool call → its event.
    pub call_events: BTreeMap<(usize, usize), u64>,
    /// Message index of each delivered peer turn in the region → the
    /// transcript entry's event.
    pub delivered: BTreeMap<usize, u64>,
}

impl AgentEpisode {
    /// The position (in `turns`) of the first call after message `index`:
    /// the call whose request first carries it.
    pub fn turn_after(&self, index: usize) -> Option<usize> {
        self.turns.iter().position(|turn| turn.index > index)
    }

    /// The position of the turn whose response is message `index`.
    pub fn turn_of_response(&self, index: usize) -> Option<usize> {
        self.turns.iter().position(|turn| turn.index == index)
    }

    /// The message index and call index of the tool call that made `event`.
    pub fn call_of_event(&self, event: u64) -> Option<(usize, usize)> {
        self.call_events
            .iter()
            .find(|(_, e)| **e == event)
            .map(|(key, _)| *key)
    }
}

/// The stop of an OpenAI-style `finish_reason`.
pub fn stop_reason(finish: Option<&str>) -> StopReason {
    match finish {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") | Some("function_call") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some("content_filter") => StopReason::Refusal,
        _ => StopReason::Other,
    }
}

/// Reconstructs `agent`'s calls in `episode`.
pub fn reconstruct(
    agent: &str,
    episode: &Episode,
    file: &str,
    scripted: bool,
    clock_at: EpisodeClock,
) -> Result<AgentEpisode, SaltError> {
    let raw = episode
        .agents
        .get(agent)
        .map(|record| record.messages.clone())
        .unwrap_or_default();
    let messages = raw.iter().map(convert).collect::<Result<Vec<_>, _>>()?;
    let start = raw
        .iter()
        .rposition(|message| {
            message.role == "user"
                && content_text(&message.content)
                    .lines()
                    .next()
                    .is_some_and(is_task_marker)
        })
        .unwrap_or(0);

    let usage: Vec<Usage> = episode
        .llm_usage
        .iter()
        .filter(|usage| usage.actor == agent && usage.accepted())
        .cloned()
        .collect();
    let responses: Vec<usize> = raw
        .iter()
        .enumerate()
        .skip(start)
        .filter(|(_, message)| message.role == "assistant")
        .map(|(at, _)| at)
        .collect();
    let fidelity = if scripted || responses.len() == usage.len() {
        Fidelity::Reconstructed
    } else {
        tracing::warn!(
            file,
            episode = episode.episode_index,
            agent,
            responses = responses.len(),
            accepted_calls = usage.len(),
            "assistant messages do not match accepted calls; exchanges are synthetic"
        );
        Fidelity::Synthetic
    };

    let mut events: Vec<u64> = episode
        .events
        .iter()
        .filter(|event| event.actor == agent)
        .map(|event| event.event_id)
        .collect();
    events.sort_unstable();
    let calls: Vec<(usize, usize)> = responses
        .iter()
        .flat_map(|&at| {
            let count = raw
                .get(at)
                .and_then(|message| message.tool_calls.as_ref())
                .map_or(0, Vec::len);
            (0..count).map(move |call| (at, call))
        })
        .collect();
    if calls.len() != events.len() {
        tracing::debug!(
            file,
            episode = episode.episode_index,
            agent,
            calls = calls.len(),
            events = events.len(),
            "tool calls and events differ in number; matched in order"
        );
    }
    let call_events: BTreeMap<(usize, usize), u64> = calls.into_iter().zip(events).collect();
    let delivered = delivered_turns(agent, &raw, start, &episode.channel_transcript);
    let turns = clock::clock(
        clock::ClockInputs {
            raw: &raw,
            start,
            call_events: &call_events,
            delivered: &delivered,
            usage,
        },
        clock_at,
    )?;
    Ok(AgentEpisode {
        agent: agent.to_owned(),
        raw,
        messages,
        start,
        turns,
        fidelity,
        call_events,
        delivered,
    })
}

/// Each delivered peer turn in the region, matched to its transcript entry
/// (same receiver, sender and content, each entry used once).
fn delivered_turns(
    agent: &str,
    raw: &[RawMessage],
    start: usize,
    transcript: &[Delivery],
) -> BTreeMap<usize, u64> {
    let mut used = vec![false; transcript.len()];
    let mut out = BTreeMap::new();
    for (at, message) in raw.iter().enumerate().skip(start) {
        if message.role != "user" {
            continue;
        }
        let text = content_text(&message.content);
        let Some(turn) = delivered_turn(&text) else {
            continue;
        };
        let Some(content) = text.get(turn.content_start..) else {
            continue;
        };
        let found = transcript.iter().enumerate().position(|(i, delivery)| {
            !used.get(i).copied().unwrap_or(true)
                && delivery.receiver == agent
                && delivery.sender == turn.from
                && delivery.content == content
        });
        if let Some(i) = found
            && let (Some(flag), Some(delivery)) = (used.get_mut(i), transcript.get(i))
        {
            *flag = true;
            out.insert(at, delivery.event_id);
        }
    }
    out
}
