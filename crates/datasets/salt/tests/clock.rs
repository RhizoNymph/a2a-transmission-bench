//! SALT on the virtual clock: one paced step per episode-global event,
//! episodes back to back (`episode_steps`), a calling exchange at its
//! event's step, any other after its latest input.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_dataset_salt::convert_trace;
use a2a_bench_dataset_salt::episode::episode_steps;
use a2a_bench_dataset_salt::schema::Episode;
use a2a_bench_format::time::Timestamp;
use common::{
    MAIN, TraceBuilder, accepted, calls, delivered, exchanges_of, positives, result, says, system,
    task, world,
};
use serde_json::{Value, json};

fn episode(value: Value) -> Episode {
    serde_json::from_value(value).unwrap_or_else(|e| panic!("{e}"))
}

fn at(pace: &Pace, major: u64, sub: u64) -> Timestamp {
    pace.at(major, 0, sub).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn an_episode_takes_its_last_event_plus_three_steps() {
    let empty = episode(json!({ "episode_index": 1, "agents": {} }));
    assert_eq!(episode_steps(&empty), 2);
    let events = episode(json!({
        "episode_index": 1,
        "agents": {},
        "events": [{ "event_id": 4, "actor": "alice" }, { "event_id": 9, "actor": "bob" }],
        "channel_transcript": [{ "event_id": 11, "sender": "alice", "receiver": "bob", "content": "x" }],
    }));
    assert_eq!(episode_steps(&events), 14);
}

/// Alice sends "Hello Bob" (event 0) and ends; Bob reads it and answers.
/// A second episode has Alice speak without calling.
fn two_episodes() -> TraceBuilder {
    let mut trace = TraceBuilder::new();
    let alice = vec![
        system("You are Alice."),
        task(1),
        calls("c1", "send_message", &json!({ "content": "Hello Bob" })),
        result("c1", "{\"success\": true}"),
        says("done"),
    ];
    let bob = vec![
        system("You are Bob."),
        task(1),
        delivered("alice", "Hello Bob"),
        says("ok"),
    ];
    let mut usage = accepted("alice", 2);
    usage.extend(accepted("bob", 1));
    trace.episodes.push(json!({
        "episode_index": 1,
        "agents": { "alice": { "messages": alice.clone() }, "bob": { "messages": bob } },
        "channel_transcript": [{ "event_id": 0, "sender": "alice", "receiver": "bob", "content": "Hello Bob" }],
        "events": [{ "event_id": 0, "actor": "alice", "tool": "send_message", "success": true, "recipient": "bob" }],
        "llm_usage": usage,
    }));
    let mut alice_two = alice;
    alice_two.extend([task(2), says("second")]);
    trace.episodes.push(json!({
        "episode_index": 2,
        "agents": { "alice": { "messages": alice_two }, "bob": { "messages": [] } },
        "llm_usage": accepted("alice", 1),
    }));
    trace
}

#[test]
fn calls_step_at_their_events_and_readers_after_their_inputs() {
    let pace = Pace::DEFAULT;
    let world = convert_trace(&two_episodes().trace(), "traces/x/y/rep001.json", pace)
        .unwrap_or_else(|e| panic!("{e}"));
    let alice: Vec<Timestamp> = exchanges_of(&world, "alice")
        .iter()
        .map(|e| e.at_us)
        .collect();
    let bob: Vec<Timestamp> = exchanges_of(&world, "bob")
        .iter()
        .map(|e| e.at_us)
        .collect();
    // Episode 1: Alice's send is event 0, so her calling exchange is step 1,
    // sub 0; her closing one comes after the send's result (step 1, sub 1).
    // Bob reads the delivery of event 0: step 1, sub 1.
    // Episode 2 starts at step 0 + 3 = 3; Alice's call has no input: sub 1.
    assert_eq!(
        alice,
        vec![at(&pace, 1, 0), at(&pace, 1, 1), at(&pace, 3, 1)]
    );
    assert_eq!(bob, vec![at(&pace, 1, 1)]);
    let labels = positives(&world);
    assert_eq!(labels.len(), 1);
    let sender = world
        .exchange(
            labels[0]
                .sender_exchange
                .unwrap_or_else(|| panic!("sender")),
        )
        .unwrap_or_else(|| panic!("in world"));
    let reader = world
        .exchange(labels[0].reader_exchange)
        .unwrap_or_else(|| panic!("in world"));
    assert!(sender.at_us < reader.at_us);
}

#[test]
fn a_call_never_steps_back_behind_the_agents_previous_one() {
    // Alice reads Bob's delivery of event 5, then calls a tool whose event
    // is 1 (recorded earlier): the call cannot go back to step 2, so it
    // lands after the floor, sub 1.
    let mut trace = TraceBuilder::new();
    let alice = vec![
        system("You are Alice."),
        task(1),
        delivered("bob", "Bob speaks first."),
        calls("c1", "lookup", &json!({ "q": "x" })),
        result("c1", "found"),
        says("done"),
    ];
    let mut usage = accepted("alice", 2);
    usage.extend(accepted("bob", 0));
    trace.episodes.push(json!({
        "episode_index": 1,
        "agents": { "alice": { "messages": alice }, "bob": { "messages": [system("You are Bob.")] } },
        "channel_transcript": [{ "event_id": 5, "sender": "bob", "receiver": "alice", "content": "Bob speaks first." }],
        "events": [{ "event_id": 1, "actor": "alice", "tool": "lookup", "success": true }],
        "llm_usage": usage,
    }));
    let pace = Pace::DEFAULT;
    let world = convert_trace(&trace.trace(), "traces/x/y/rep001.json", pace)
        .unwrap_or_else(|e| panic!("{e}"));
    let alice: Vec<Timestamp> = exchanges_of(&world, "alice")
        .iter()
        .map(|e| e.at_us)
        .collect();
    assert_eq!(alice, vec![at(&pace, 6, 1), at(&pace, 6, 1).plus_one()]);
}

trait PlusOne {
    fn plus_one(self) -> Self;
}

impl PlusOne for Timestamp {
    fn plus_one(self) -> Self {
        Timestamp::from_micros(self.as_micros() + 1)
    }
}

#[test]
fn every_sender_exchange_precedes_its_reader_on_the_fixture() {
    let world = world(MAIN);
    for label in positives(&world) {
        let sender = label
            .sender_exchange
            .and_then(|id| world.exchange(id))
            .unwrap_or_else(|| panic!("sender"));
        let reader = world
            .exchange(label.reader_exchange)
            .unwrap_or_else(|| panic!("reader"));
        assert!(sender.at_us < reader.at_us);
    }
    // Every time is on the default pace's grid: (step, 0, sub ≤ 1), or one
    // microsecond after an agent's previous call.
    let epoch = a2a_bench_corpus::clock::EPOCH_MICROS;
    assert!(
        world
            .exchanges()
            .iter()
            .all(|e| e.at_us.as_micros() > epoch)
    );
}
