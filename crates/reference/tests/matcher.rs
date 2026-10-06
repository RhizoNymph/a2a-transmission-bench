//! The reference matcher on hand-built worlds (ported from crosstalk-eval's
//! `tests/reference.rs`, on bench types).
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;

use a2a_bench_format::check::WorldInputs;
use a2a_bench_format::labels::{CarrierKind, Codec, MatchClass, Route};
use a2a_bench_format::message::Message;
use a2a_bench_format::predictions::MatchKind;
use a2a_bench_format::resource::Resource;
use a2a_bench_reference::{MAX_POSTINGS, ReferenceConfig, WorldOutput};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{
    Found, WorldBuilder, agent_of, calls, found, matched, result, says, system, transmissions, user,
};

const SENTENCE: &str = "The vendor table has eleven overdue approvals in March";

/// Alice and Bob.
struct Pair {
    builder: WorldBuilder,
}

fn pair() -> Pair {
    Pair {
        builder: WorldBuilder::new(&["alice", "bob"]),
    }
}

impl Pair {
    /// Alice's exchange at `at`, answering with `response` to a task prompt.
    fn alice_writes(&mut self, at: u64, response: Message) {
        let request = vec![system("You are Alice."), user("Do the task and report.")];
        self.builder.exchange("alice", at, request, vec![response]);
    }

    /// Bob's exchange at `at` with `inputs` after his system prompt.
    fn bob_reads(&mut self, at: u64, inputs: Vec<Message>) {
        let mut request = vec![system("You are Bob.")];
        request.extend(inputs);
        self.builder
            .exchange("bob", at, request, vec![says("Noted.")]);
    }

    fn finish(self) -> WorldInputs {
        self.builder.finish()
    }
}

fn send(content: &str) -> Message {
    let arguments = serde_json::json!({ "content": content, "message_type": "status" }).to_string();
    calls("call_1", "send_message", &arguments)
}

fn delivered(content: &str) -> Message {
    user(&format!(
        "[round=1/5][from=alice][type=status]\n\n{content}"
    ))
}

/// Every match kind the matcher reported.
fn kinds(found: &[Found]) -> Vec<MatchKind> {
    found.iter().map(|f| f.evidence.kind.clone()).collect()
}

fn class(found: &Found) -> MatchClass {
    found.evidence.kind.class()
}

fn json_string() -> MatchKind {
    MatchKind::Decoded {
        codecs: vec![Codec::JsonString],
    }
}

#[test]
fn verbatim_delivery_is_an_exact_match() {
    let mut pair = pair();
    pair.alice_writes(1, send(SENTENCE));
    let message = delivered(SENTENCE);
    pair.bob_reads(2, vec![message.clone()]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(transmissions(&output), 1);
    assert!(!found.is_empty());
    for f in &found {
        let p = &f.evidence;
        assert_eq!((&p.from, &p.to), (&agent_of("alice"), &agent_of("bob")));
        assert_eq!(class(f), MatchClass::Exact);
        assert_eq!(p.carrier, CarrierKind::UserTurn);
        assert_eq!(p.route, Route::Direct);
        assert_eq!(p.read_at.message, message.id());
        let text = inputs.text_at(&p.read_at).unwrap();
        assert!(SENTENCE.contains(text.as_ref()), "{text:?}");
        let origin = p.origin_at.expect("the reference knows its spans");
        assert!(inputs.text_at(&origin).is_ok());
    }
}

#[test]
fn escaped_content_is_a_json_string_match() {
    let content = "First line of the \"audit\" result\nsecond line names the owner Omar";
    let mut pair = pair();
    pair.alice_writes(1, send(content));
    pair.bob_reads(2, vec![delivered(content)]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert!(!found.is_empty());
    assert!(found.iter().any(|f| class(f) == MatchClass::Decoded));
    assert!(
        kinds(&found).contains(&json_string()),
        "{:?}",
        kinds(&found)
    );
    assert!(!kinds(&found).contains(&MatchKind::Normalized));
}

#[test]
fn case_and_whitespace_differences_are_normalized() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    pair.bob_reads(2, vec![user(&SENTENCE.to_uppercase().replace(' ', "   "))]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert_eq!(found.len(), 1);
    assert_eq!(class(&found[0]), MatchClass::Normalized);
}

#[test]
fn double_escaped_relays_and_yaml_continuations_are_string_decoded() {
    let content = "Quote: \"the budget gap is 30000\" and the vendor risk is high today";
    let once = serde_json::to_string(content).unwrap_or_default();
    let twice = serde_json::to_string(&once).unwrap_or_default();
    let mut pair = pair();
    pair.alice_writes(1, send(content));
    pair.bob_reads(
        2,
        vec![result("call_9", &format!("{{\"raw_log\": {twice}}}"))],
    );
    let yaml = "note: \"Quote: \\\"the budget gap is 30000\\\" and the vendor \\\n    risk is high today\"";
    pair.bob_reads(3, vec![user(yaml)]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    let exchanges: BTreeSet<_> = found.iter().map(|f| f.evidence.reader_exchange).collect();
    assert_eq!(
        exchanges.len(),
        2,
        "found in the relayed log and in the YAML"
    );
    assert!(found.iter().all(|f| class(f) == MatchClass::Decoded));
    let kinds = kinds(&found);
    assert!(kinds.contains(&json_string()));
    assert!(kinds.contains(&MatchKind::Decoded {
        codecs: vec![Codec::YamlString]
    }));
}

#[test]
fn encoded_content_is_a_decoded_match() {
    let base64 = STANDARD.encode(SENTENCE);
    let hex: String = SENTENCE.bytes().map(|b| format!("{b:02x}")).collect();
    let url: String = SENTENCE.replace(' ', "%20");
    for (encoded, codec) in [
        (base64, Codec::Base64),
        (hex, Codec::Hex),
        (url, Codec::UrlEncoding),
    ] {
        let mut pair = pair();
        pair.alice_writes(1, says(SENTENCE));
        pair.bob_reads(2, vec![user(&format!("payload: {encoded} end"))]);
        let inputs = pair.finish();
        let (output, found) = matched(&inputs);
        assert_eq!(found.len(), 1, "{codec:?}");
        assert_eq!(transmissions(&output), 1, "{codec:?}");
        assert_eq!(
            found[0].evidence.kind,
            MatchKind::Decoded {
                codecs: vec![codec]
            },
            "{codec:?}"
        );
    }
}

#[test]
fn text_the_writer_read_is_not_originated() {
    let shared = "SELECT request_id FROM procurement_requests WHERE amount >= 30000";
    let mut pair = pair();
    let request = vec![
        system("You are Alice."),
        user("go"),
        result("call_0", shared),
    ];
    pair.builder.exchange(
        "alice",
        1,
        request,
        vec![says(&format!("I ran {shared} and it worked."))],
    );
    pair.bob_reads(2, vec![result("call_5", shared)]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert!(found.is_empty(), "{found:?}");
    assert!(
        output.summary.spans > 0,
        "the novel framing is still a span"
    );
}

#[test]
fn an_agent_never_matches_itself() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let request = vec![
        system("You are Alice."),
        user("Do the task and report."),
        says(SENTENCE),
        user(&format!("You said: {SENTENCE}")),
    ];
    pair.builder.exchange("alice", 3, request, vec![says("ok")]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert!(found.is_empty());
}

#[test]
fn short_messages_are_below_the_minimum() {
    let mut pair = pair();
    pair.alice_writes(1, send("OK, accept."));
    pair.bob_reads(2, vec![delivered("OK, accept.")]);
    let inputs = pair.finish();
    assert!(matched(&inputs).1.is_empty());
}

#[test]
fn only_new_inputs_are_scanned() {
    let mut pair = pair();
    pair.alice_writes(1, send(SENTENCE));
    let message = delivered(SENTENCE);
    pair.bob_reads(2, vec![message.clone()]);
    let request = vec![
        system("You are Bob."),
        message,
        says("Noted."),
        user("next round"),
    ];
    pair.builder.exchange("bob", 3, request, vec![says("done")]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(transmissions(&output), 1);
    let readers: BTreeSet<_> = found.iter().map(|f| f.evidence.reader_exchange).collect();
    assert_eq!(readers.len(), 1);
}

#[test]
fn thought_ids_and_signatures_are_ignored() {
    let id = "call_419236__thought__EsQGCsEGARFNMg9O9j8kD2jcYfVRHLjz4WqB7ShRIyNClWpHxs+Be8XJTaBDb8T34Vq1IBnFWZJEjEUKOVsQ0ODxIsE";
    let signature = STANDARD.encode(SENTENCE);
    let mut pair = pair();
    pair.alice_writes(1, says(&format!("tool ids seen: {id} and {id}")));
    pair.builder
        .exchange("alice", 2, vec![user("x")], vec![says(SENTENCE)]);
    pair.bob_reads(
        3,
        vec![result(
            "call_7",
            &format!("{{\"id\": \"{id}\", \"thought_signature\": \"{signature}\"}}"),
        )],
    );
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert!(found.is_empty(), "{found:?}");
}

fn read_notes(call: &str, path: &str) -> Message {
    calls(call, "read_file", &format!(r#"{{"path": "{path}"}}"#))
}

#[test]
fn channel_reads_route_through_the_resource() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let request = vec![
        system("You are Bob."),
        user("check the notes"),
        read_notes("call_r", "/shared/./notes/../notes.md"),
        result("call_r", &format!("# notes\n{SENTENCE}\n")),
    ];
    pair.builder
        .exchange("bob", 2, request, vec![says("Noted.")]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].evidence.carrier, CarrierKind::ToolResult);
    assert_eq!(
        found[0].evidence.route,
        Route::Channel {
            resource: Resource::File {
                path: "/shared/notes.md".into()
            }
        }
    );
}

#[test]
fn url_reads_route_through_the_canonical_url() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let request = vec![
        system("You are Bob."),
        calls(
            "call_g",
            "http_request",
            r#"{"method":"GET","url":"HTTPS://Wiki.Example:443/Plan?b=2&a=1#top"}"#,
        ),
        result("call_g", SENTENCE),
    ];
    pair.builder
        .exchange("bob", 2, request, vec![says("Noted.")]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].evidence.route,
        Route::Channel {
            resource: Resource::Url("https://wiki.example/Plan?a=1&b=2".into())
        }
    );
}

#[test]
fn tool_results_without_a_resource_are_direct() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let request = vec![
        system("You are Bob."),
        calls("call_q", "query_database", r#"{"sql": "SELECT 1"}"#),
        result("call_q", SENTENCE),
    ];
    pair.builder
        .exchange("bob", 2, request, vec![says("Noted.")]);
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].evidence.route, Route::Direct);
    assert_eq!(found[0].evidence.carrier, CarrierKind::ToolResult);
}

#[test]
fn system_prompts_are_their_own_carrier() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    pair.builder.exchange(
        "bob",
        2,
        vec![system(&format!("Context: {SENTENCE}")), user("go")],
        vec![says("ok")],
    );
    let inputs = pair.finish();
    let (_, found) = matched(&inputs);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].evidence.carrier, CarrierKind::SystemPrompt);
    assert_eq!(found[0].evidence.route, Route::Direct);
}

#[test]
fn hits_group_into_one_transmission_per_sender_and_route() {
    let other = "A second independent sentence about quarterly risk reviews";
    let mut pair = pair();
    pair.alice_writes(1, says(&format!("{SENTENCE}. {other}.")));
    pair.bob_reads(2, vec![user(SENTENCE), user(other)]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(transmissions(&output), 1);
    assert_eq!(found.len(), 2);
    assert!(
        found
            .iter()
            .all(|f| f.transmission == found[0].transmission)
    );
}

#[test]
fn tool_results_of_different_tools_are_different_transmissions() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let request = vec![
        system("You are Bob."),
        calls("call_q", "query_database", r#"{"sql": "SELECT 1"}"#),
        result("call_q", SENTENCE),
        calls("call_s", "search", r#"{"q": "vendor"}"#),
        result("call_s", SENTENCE),
    ];
    pair.builder
        .exchange("bob", 2, request, vec![says("Noted.")]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(found.len(), 2);
    assert_eq!(transmissions(&output), 2, "routes are keyed by tool name");
}

#[test]
fn runs_are_deterministic() {
    let build = || {
        let mut pair = pair();
        pair.alice_writes(1, send(SENTENCE));
        pair.bob_reads(2, vec![delivered(SENTENCE)]);
        pair.finish()
    };
    let (a, b) = (build(), build());
    assert_eq!(matched(&a).0, matched(&b).0);
}

// --- rereads ---

#[test]
fn a_channel_reread_is_reported_at_the_first_read_only() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let first = vec![
        system("You are Bob."),
        read_notes("call_1", "/shared/notes.md"),
        result("call_1", SENTENCE),
    ];
    pair.builder
        .exchange("bob", 2, first.clone(), vec![says("Noted.")]);
    let mut second = first;
    second.push(says("Noted."));
    second.push(read_notes("call_2", "/shared/notes.md"));
    second.push(result("call_2", &format!("{SENTENCE}\n")));
    pair.builder
        .exchange("bob", 3, second, vec![says("Still noted.")]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    let readers: BTreeSet<_> = found.iter().map(|f| f.evidence.reader_exchange).collect();
    assert_eq!(readers.len(), 1, "{found:?}");
    assert_eq!(found.len(), 1);
    assert_eq!(output.summary.rereads, 1);
}

#[test]
fn a_reread_through_another_channel_is_a_first_read() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let first = vec![
        system("You are Bob."),
        read_notes("call_1", "/shared/notes.md"),
        result("call_1", SENTENCE),
    ];
    pair.builder
        .exchange("bob", 2, first.clone(), vec![says("Noted.")]);
    let mut second = first;
    second.push(says("Noted."));
    second.push(read_notes("call_2", "/shared/copy.md"));
    second.push(result("call_2", SENTENCE));
    pair.builder
        .exchange("bob", 3, second, vec![says("Still noted.")]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(found.len(), 2);
    assert_eq!(output.summary.rereads, 0);
}

#[test]
fn hits_on_one_span_in_one_exchange_all_count() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    let request = vec![
        system("You are Bob."),
        read_notes("call_1", "/shared/notes.md"),
        result("call_1", SENTENCE),
        read_notes("call_2", "/shared/notes.md"),
        result("call_2", &format!("again: {SENTENCE}")),
    ];
    pair.builder
        .exchange("bob", 2, request, vec![says("Noted.")]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert_eq!(found.len(), 2);
    assert_eq!(transmissions(&output), 1);
    assert_eq!(output.summary.rereads, 0);
}

#[test]
fn direct_routes_are_never_rereads() {
    let mut pair = pair();
    pair.alice_writes(1, says(SENTENCE));
    pair.bob_reads(2, vec![user(SENTENCE)]);
    pair.builder.exchange(
        "bob",
        3,
        vec![
            system("You are Bob."),
            user(SENTENCE),
            says("Noted."),
            user(&format!("Once more: {SENTENCE}")),
        ],
        vec![says("ok")],
    );
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    let readers: BTreeSet<_> = found.iter().map(|f| f.evidence.reader_exchange).collect();
    assert_eq!(readers.len(), 2);
    assert_eq!(output.summary.rereads, 0);
}

// --- boilerplate: text many agents originate independently ---

const TEMPLATE: &str = "Describe the new page here and add your notes below";

/// A sentence no other writer shares a 24-byte window with: every word
/// carries the writer's own tag.
fn unique_sentence(writer: usize) -> String {
    let tag: String = [writer / 26 % 26, writer % 26]
        .iter()
        .map(|&d| char::from(b'a' + u8::try_from(d).unwrap_or(0)))
        .collect();
    (0..8)
        .map(|word| format!("{tag}note{word}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn writer(at: usize) -> String {
    format!("writer{at:03}")
}

/// A world where `originators` agents each say `TEMPLATE` plus a unique
/// sentence, then a reader reads one tool result holding `copies` copies of
/// `TEMPLATE` and the first originator's unique sentence.
fn shared_template_world(originators: usize, copies: usize) -> WorldInputs {
    let names: Vec<String> = (0..originators).map(writer).collect();
    let mut builder = WorldBuilder::new(&[]);
    for name in &names {
        builder.agent(name);
    }
    builder.agent("reader");
    for (at, name) in names.iter().enumerate() {
        let text = format!("{TEMPLATE}\n{}", unique_sentence(at));
        let request = vec![system("You are a writer."), user("Write the page.")];
        builder.exchange(name, at as u64 + 1, request, vec![says(&text)]);
    }
    let mut body = vec![TEMPLATE; copies].join("\n");
    body.push('\n');
    body.push_str(&unique_sentence(0));
    let request = vec![
        system("You are the reader."),
        calls(
            "read_1",
            "http_request",
            r#"{"method":"GET","url":"https://wiki.example/Hub"}"#,
        ),
        result("read_1", &body),
    ];
    builder.exchange(
        "reader",
        originators as u64 + 1,
        request,
        vec![says("Read it.")],
    );
    builder.finish()
}

#[test]
fn text_many_agents_originate_is_boilerplate() {
    // Without a cutoff every copy matches every originator's span:
    // 74 originators x 400 copies = 29,600 matches in one read.
    let originators = ReferenceConfig::default().max_postings + 24;
    let inputs = shared_template_world(originators, 400);
    let (output, found) = matched(&inputs);
    assert!(
        output.summary.matches <= 2,
        "boilerplate must not fan out: {} matches",
        output.summary.matches
    );
    // The one sentence only the first writer originated is still found.
    assert_eq!(transmissions(&output), 1);
    assert!(found.iter().all(
        |f| (&f.evidence.from, &f.evidence.to) == (&agent_of(&writer(0)), &agent_of("reader"))
    ));
}

#[test]
fn text_a_few_agents_originate_still_matches_each() {
    // At or under the cutoff, each originator's span is still a candidate.
    let originators = ReferenceConfig::default().max_postings;
    let inputs = shared_template_world(originators, 1);
    let (output, _) = matched(&inputs);
    assert_eq!(transmissions(&output), originators);
}

#[test]
fn matches_grow_linearly_with_the_read_body() {
    // A large read body of boilerplate costs no more matches than a small one.
    let originators = ReferenceConfig::default().max_postings + 24;
    let small = shared_template_world(originators, 10);
    let large = shared_template_world(originators, 4_000);
    let (small, _) = matched(&small);
    let (large, _) = matched(&large);
    assert_eq!(small.summary.matches, large.summary.matches);
}

#[test]
fn the_cutoff_is_configurable() {
    let originators = 10;
    let inputs = shared_template_world(originators, 1);
    let config = ReferenceConfig {
        max_postings: 4,
        ..ReferenceConfig::default()
    };
    let (output, _) = common::matched_with(&inputs, config);
    assert_eq!(transmissions(&output), 1, "only the unique sentence");
}

#[test]
fn the_default_boilerplate_cutoff_is_l4s() {
    // L4's `IndexSettings::default().cutoff()` (crosstalk's
    // crates/provenance): a fingerprint in more than 50 live texts is
    // boilerplate.
    assert_eq!(ReferenceConfig::default().max_postings, 50);
    assert_eq!(MAX_POSTINGS, 50);
}

// --- string levels ---

#[test]
fn the_matcher_reports_no_match_two_string_levels_apart() {
    // Quotes every few words, so every 24-byte window holds an escape.
    let content = r#""alpha" "bravo" "charlie" "delta" "echo" "foxtrot" "golf" "hotel""#;
    let once = serde_json::to_string(content).unwrap_or_default();
    let twice = serde_json::to_string(&once).unwrap_or_default();
    let mut pair = pair();
    pair.alice_writes(1, says(content));
    pair.bob_reads(2, vec![result("call_9", &format!("{{\"log\": {twice}}}"))]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert!(found.is_empty(), "{found:?}");
    assert!(
        output.summary.out_of_reach > 0,
        "the fold found it; the spec cannot"
    );

    // One level apart, the same text is a JSON string match.
    let mut pair = self::pair();
    pair.alice_writes(1, says(content));
    pair.bob_reads(2, vec![result("call_9", &format!("{{\"log\": {once}}}"))]);
    let inputs = pair.finish();
    let (output, found) = matched(&inputs);
    assert!(!found.is_empty());
    assert_eq!(output.summary.out_of_reach, 0);
    assert!(
        kinds(&found).iter().all(|kind| *kind == json_string()),
        "{:?}",
        kinds(&found)
    );
}

fn summary_matches(output: &WorldOutput) -> usize {
    found(output).len()
}

#[test]
fn the_summary_counts_what_the_predictions_hold() {
    let mut pair = pair();
    pair.alice_writes(1, send(SENTENCE));
    pair.bob_reads(2, vec![delivered(SENTENCE)]);
    let inputs = pair.finish();
    let (output, _) = matched(&inputs);
    assert_eq!(output.summary.matches, summary_matches(&output));
    assert_eq!(output.summary.transmissions, transmissions(&output));
    assert_eq!(output.summary.exchanges, 2);
    assert_eq!(output.summary.agents, 2);
}
