//! Forwarding: SALT deliveries the sender relayed from its own tool output
//! are `Tier::Forwarding` labels (ported from crosstalk-eval's
//! `tests/forwarding.rs`, converter part, and `datasets/salt/forwarding.rs`'
//! unit tests). Scoring them apart is the scorer's.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_dataset_salt::forwarding::{FORWARD_K, FORWARDED_SHARE, ToolOutput};
use a2a_bench_dataset_salt::{convert_trace, load_world};
use a2a_bench_format::labels::{Codec, MatchNeed, Route, Tier};
use common::{
    MAIN, TraceBuilder, accepted, calls, delivered, positives, result, root, says, system, task,
};
use serde_json::json;

/// Alice's delivery in the fixture's first episode.
const ALICE_SAYS: &str =
    "Alice here: my answer is PR0100 and PR0119 after checking the budget gap filter twice.";

/// Alice's `inspect_database` result in the same episode.
const SCHEMA: &str = r#"{"success": true, "tables": ["adverse_events", "departments", "procurement_requests", "vendors"], "note": "Shared schema text that both agents read from the same database file."}"#;

/// Every string in `value` with `from` replaced by `to`; a string holding
/// `from` that is itself a JSON object (tool-call arguments) is replaced
/// inside and written back. Other strings are left byte for byte.
fn replace(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(text) if text.contains(from) => {
            if let Ok(mut inner @ serde_json::Value::Object(_)) =
                serde_json::from_str::<serde_json::Value>(text)
            {
                replace(&mut inner, from, to);
                *text = inner.to_string();
            } else {
                *text = text.replace(from, to);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| replace(v, from, to)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| replace(v, from, to)),
        _ => {}
    }
}

#[test]
fn a_salt_delivery_pasting_the_senders_tool_result_is_forwarding() {
    let original = std::fs::read_to_string(root().join(MAIN)).unwrap_or_else(|e| panic!("{e}"));
    let mut trace: serde_json::Value =
        serde_json::from_str(&original).unwrap_or_else(|e| panic!("{e}"));
    let pasted = format!("My schema: {SCHEMA}");
    replace(&mut trace, ALICE_SAYS, &pasted);
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let file = dir.path().join(MAIN);
    std::fs::create_dir_all(file.parent().unwrap_or_else(|| panic!("parent")))
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&file, trace.to_string()).unwrap_or_else(|e| panic!("{e}"));

    let before = load_world(&root(), Path::new(MAIN)).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        positives(&before)
            .iter()
            .all(|label| label.tier == Tier::Construction),
        "the fixture forwards nothing"
    );

    let after = load_world(dir.path(), Path::new(MAIN)).unwrap_or_else(|e| panic!("{e}"));
    let labels = positives(&after);
    assert_eq!(labels.len(), positives(&before).len());
    let forwarded: Vec<_> = labels
        .iter()
        .filter(|label| label.tier == Tier::Forwarding)
        .collect();
    assert_eq!(forwarded.len(), 1, "{labels:#?}");
    assert_eq!(forwarded[0].content.text, pasted);
    assert_eq!(forwarded[0].from.as_str(), "alice");
    // Still a construction delivery in every other respect.
    assert_eq!(forwarded[0].route, Route::Direct);
    assert_eq!(forwarded[0].needs, MatchNeed::through_json_string(&pasted));
}

const LOG: &str = "seq 1 read_code src/ledger.py ok; seq 2 query_database SELECT total FROM orders WHERE month = 'march' ok; seq 3 resolve_records vendor 4471 ok";
const NOTE: &str =
    "Bob, the March ledger total is off by exactly forty-two dollars and nine cents.";

/// Alice reads a log through `get_log`, sends it to Bob verbatim, then sends
/// a note she wrote herself.
fn relaying_trace(log_before_send: bool) -> TraceBuilder {
    let mut alice = vec![system("You are Alice."), task(1)];
    if log_before_send {
        alice.push(calls("call_log", "get_log", &json!({})));
        alice.push(result("call_log", LOG));
    }
    alice.extend([
        calls("call_s1", "send_message", &json!({ "content": LOG })),
        result("call_s1", "{\"success\": true}"),
        calls("call_s2", "send_message", &json!({ "content": NOTE })),
        result("call_s2", "{\"success\": true}"),
        says("done"),
    ]);
    if !log_before_send {
        alice.push(calls("call_log", "get_log", &json!({})));
        alice.push(result("call_log", LOG));
        alice.push(says("read it"));
    }
    let bob = vec![
        system("You are Bob."),
        task(1),
        delivered("alice", LOG),
        says("thanks"),
        delivered("alice", NOTE),
        says("noted"),
    ];
    let alice_calls = if log_before_send { 4 } else { 5 };
    let mut events = Vec::new();
    let mut transcript = Vec::new();
    let mut next = 0u64;
    if log_before_send {
        events.push(
            json!({ "event_id": next, "actor": "alice", "tool": "get_log", "success": true }),
        );
        next += 1;
    }
    for content in [LOG, NOTE] {
        events.push(json!({ "event_id": next, "actor": "alice", "tool": "send_message", "success": true, "recipient": "bob", "content": content }));
        transcript.push(
            json!({ "event_id": next, "sender": "alice", "receiver": "bob", "content": content }),
        );
        next += 1;
    }
    if !log_before_send {
        events.push(
            json!({ "event_id": next, "actor": "alice", "tool": "get_log", "success": true }),
        );
    }
    let mut usage = accepted("alice", alice_calls);
    usage.extend(accepted("bob", 2));
    let mut trace = TraceBuilder::new();
    trace.episodes.push(json!({
        "episode_index": 1,
        "agents": { "alice": { "messages": alice }, "bob": { "messages": bob } },
        "channel_transcript": transcript,
        "events": events,
        "llm_usage": usage,
    }));
    trace
}

#[test]
fn a_relayed_tool_result_is_forwarding_and_an_own_note_is_not() {
    let world = convert_trace(
        &relaying_trace(true).trace(),
        "traces/x/y/rep001.json",
        Pace::DEFAULT,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let labels = positives(&world);
    assert_eq!(labels.len(), 2);
    let tier = |text: &str| {
        labels
            .iter()
            .find(|l| l.content.text == text)
            .map(|l| l.tier)
    };
    assert_eq!(tier(LOG), Some(Tier::Forwarding));
    assert_eq!(tier(NOTE), Some(Tier::Construction));
}

#[test]
fn a_tool_result_read_after_the_send_does_not_make_it_forwarding() {
    let world = convert_trace(
        &relaying_trace(false).trace(),
        "traces/x/y/rep001.json",
        Pace::DEFAULT,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let labels = positives(&world);
    assert_eq!(labels.len(), 2);
    assert!(labels.iter().all(|l| l.tier == Tier::Construction));
}

#[test]
fn the_need_is_exact_unless_json_escapes_the_text() {
    assert_eq!(
        MatchNeed::through_json_string("plain text, no escapes"),
        MatchNeed::Exact
    );
    for escaped in [
        "a \"quote\"",
        "back\\slash",
        "line\nbreak",
        "tab\there",
        "\u{1}",
    ] {
        assert_eq!(
            MatchNeed::through_json_string(escaped),
            MatchNeed::Decoded {
                codecs: vec![Codec::JsonString]
            },
            "{escaped:?}"
        );
    }
    // Non-ASCII is written raw in JSON: no escape.
    assert_eq!(MatchNeed::through_json_string("café ✓"), MatchNeed::Exact);
}

// The rule itself (crosstalk-eval's `forwarding.rs` unit tests).

const TOOL_LOG: &str = r#"[{"seq":1,"tool":"read_code","args":{"path":"src/ledger.py"},"ok":true},{"seq":2,"tool":"query_database","args":{"sql":"SELECT * FROM orders"},"ok":true}]"#;

/// Tool results at indexes 0, 1, …; the content is sent after all.
fn output(texts: &[&str]) -> ToolOutput {
    let mut out = ToolOutput::default();
    for (index, text) in texts.iter().enumerate() {
        out.add(index, text);
    }
    out
}

const AFTER: usize = usize::MAX;

#[test]
fn the_constants_are_ct_evals() {
    assert_eq!(FORWARD_K, 24);
    assert_eq!(FORWARDED_SHARE, (1, 2));
}

#[test]
fn a_pasted_tool_result_is_forwarding() {
    let content = format!("Chunk 1 of 6 of my raw log: {TOOL_LOG}");
    assert!(output(&[TOOL_LOG]).forwards(&content, AFTER));
}

#[test]
fn prose_quoting_a_short_line_is_not() {
    let content = "I looked at the ledger and the totals look off for March; \
                   the read_code step went fine, so I will accept your verdict.";
    assert!(!output(&[TOOL_LOG]).forwards(content, AFTER));
}

#[test]
fn half_covered_is_forwarding_and_less_is_not() {
    let pasted = &TOOL_LOG[..80];
    let own = "x".repeat(70);
    let half = format!("{pasted} {}", "y".repeat(78));
    assert!(output(&[TOOL_LOG]).forwards(&half, AFTER));
    let less = format!("{pasted} {own} {own}");
    assert!(!output(&[TOOL_LOG]).forwards(&less, AFTER));
}

#[test]
fn escapes_and_case_are_folded() {
    let escaped = TOOL_LOG.replace('"', "\\\"").to_uppercase();
    assert!(output(&[TOOL_LOG]).forwards(&escaped, AFTER));
}

#[test]
fn only_results_before_the_send_count() {
    let mut out = ToolOutput::default();
    out.add(5, TOOL_LOG);
    assert!(!out.forwards(TOOL_LOG, 5));
    assert!(out.forwards(TOOL_LOG, 6));
}

#[test]
fn nothing_seen_or_too_short_is_not_forwarding() {
    assert!(!output(&[]).forwards(TOOL_LOG, AFTER));
    assert!(!output(&[TOOL_LOG]).forwards(&TOOL_LOG[..FORWARD_K - 1], AFTER));
}

#[test]
fn escaped_newlines_and_unicode_escapes_fold_like_their_characters() {
    let raw = "Total for March: 4471 units\nVendor: Acme Café Ltd, net 30 days";
    let escaped = "total for march: 4471 units\\nvendor: acme caf\\u00e9 ltd, net 30 days";
    assert!(output(&[raw]).forwards(escaped, AFTER));
    let doubly = "Total for March: 4471 units\\\\nVendor: Acme Caf\\\\u00e9 Ltd, net 30 days";
    assert!(output(&[raw]).forwards(doubly, AFTER));
}
