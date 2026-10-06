//! The SALT converter on synthetic fixtures shaped like the dataset (ported
//! from crosstalk-eval's `tests/salt.rs`).
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use a2a_bench_corpus::clock::Pace;
use a2a_bench_corpus::source::{TraceSource, WorldFilter};
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_salt::files::{discover, world_name};
use a2a_bench_dataset_salt::schema::Trace;
use a2a_bench_dataset_salt::{DATASET, Options, VERSION, convert_trace, load_world, source};
use a2a_bench_format::exchange::{Driven, Fidelity};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{LabelId, WorldKey};
use a2a_bench_format::labels::{Codec, Label, MatchNeed, NegativeReason, Tier};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::{AssistantPart, Body};
use common::{
    CONTROLLED, MAIN, MEMORY, agent, carries, exchanges_of, negatives, positives, response, root,
    world,
};

#[test]
fn the_crate_names_its_dataset_and_version() {
    assert_eq!(DATASET, "salt");
    assert_eq!(VERSION, 1);
}

#[test]
fn files_are_stratified_filtered_and_limited() {
    let all = discover(&root(), &Options::default()).unwrap_or_else(|e| panic!("{e}"));
    let names: Vec<String> = all
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![CONTROLLED.to_owned(), MAIN.to_owned(), MEMORY.to_owned()]
    );
    let limited = discover(
        &root(),
        &Options {
            limit: Some(1),
            include: vec![],
        },
    )
    .unwrap_or_default();
    assert_eq!(limited.len(), 1);
    let filtered = discover(
        &root(),
        &Options {
            limit: None,
            include: vec!["main/".into()],
        },
    )
    .unwrap_or_default();
    assert_eq!(filtered.len(), 1);
    assert_eq!(
        world_name(Path::new("traces/main/c/rep001.json.gz")),
        "main/c/rep001"
    );
    assert!(discover(Path::new("/nonexistent"), &Options::default()).is_err());
}

#[test]
fn stratified_order_takes_one_file_per_condition_first() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let trace = std::fs::read(root().join(MAIN)).unwrap_or_else(|e| panic!("{e}"));
    let files = [
        "traces/a/a1/rep001.json",
        "traces/a/a1/rep002.json",
        "traces/a/a1/rep003.json",
        "traces/a/a2/rep001.json.gz",
        "traces/b/b1/rep001.json",
        "traces/b/b1/rep002.json",
        "traces/b/b1/notes.txt",
    ];
    for file in files {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("parent")))
            .unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&path, &trace).unwrap_or_else(|e| panic!("{e}"));
    }
    let names = |options: &Options| {
        discover(dir.path(), options)
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&Options::default()),
        [
            "traces/a/a1/rep001.json",
            "traces/a/a2/rep001.json.gz",
            "traces/b/b1/rep001.json",
            "traces/a/a1/rep002.json",
            "traces/b/b1/rep002.json",
            "traces/a/a1/rep003.json",
        ]
    );
    assert_eq!(
        names(&Options {
            limit: Some(3),
            include: vec![],
        }),
        [
            "traces/a/a1/rep001.json",
            "traces/a/a2/rep001.json.gz",
            "traces/b/b1/rep001.json",
        ]
    );
}

#[test]
fn options_are_the_manifest_selection() {
    assert!(Options::default().settings().is_empty());
    let options = Options {
        limit: Some(53),
        include: vec!["main/".into(), "warmup".into()],
    };
    assert_eq!(
        options.settings(),
        BTreeMap::from([
            ("include[0]".to_owned(), Setting::Text("main/".into())),
            ("include[1]".to_owned(), Setting::Text("warmup".into())),
            ("limit".to_owned(), Setting::Int(53)),
        ])
    );
}

#[test]
fn main_world_reconstructs_every_call() {
    let world = world(MAIN);
    assert_eq!(world.key().as_str(), "main/main__fixture-model/rep001");
    assert_eq!(world.dataset().as_str(), "salt");
    assert_eq!(
        world.coverage(),
        Coverage::Complete {
            tier: Tier::Construction
        }
    );
    assert_eq!(
        (
            exchanges_of(&world, "alice").len(),
            exchanges_of(&world, "bob").len()
        ),
        (9, 8)
    );
    assert!(
        world
            .decl()
            .agents
            .iter()
            .all(|a| a.driven == Driven::Model)
    );
    assert!(
        world
            .exchanges()
            .iter()
            .all(|e| e.fidelity == Fidelity::Reconstructed)
    );
    let times: Vec<_> = world.exchanges().iter().map(|e| e.at_us).collect();
    let mut sorted = times.clone();
    sorted.sort();
    assert_eq!(times, sorted);
    for name in ["alice", "bob"] {
        let mine = exchanges_of(&world, name);
        assert!(mine.windows(2).all(|w| w[0].at_us < w[1].at_us));
        for exchange in &mine {
            assert_eq!(exchange.response.messages.len(), 1);
            assert!(matches!(
                response(&world, exchange).body(),
                Body::Assistant(_)
            ));
            assert!(exchange.request.tools.is_none(), "SALT records no schemas");
            assert!(exchange.response.stop.is_some());
        }
    }
}

#[test]
fn exchanges_name_the_requested_model_and_their_source() {
    let world = world(MAIN);
    let alice = exchanges_of(&world, "alice");
    assert_eq!(
        alice[0].source.path(),
        "/results/0/agents/alice/messages/2",
        "the first call answers the task turn"
    );
    assert!(
        alice
            .iter()
            .all(|e| e.source.file() == MAIN && e.client.model.is_some())
    );
    let bob = world
        .decl()
        .agents
        .iter()
        .find(|a| a.key == agent("bob"))
        .unwrap_or_else(|| panic!("bob"));
    assert_eq!(
        bob.model.as_deref(),
        Some("bedrock/converse/anthropic.claude-fixture")
    );
}

#[test]
fn episodes_carry_their_history_but_only_their_own_calls() {
    let world = world(MAIN);
    let alice = exchanges_of(&world, "alice");
    // Episode 2's first call sees episode 1's whole list, then its own turns.
    let first_of_second = alice[6];
    assert_eq!(first_of_second.request.messages.len(), 22);
    assert_eq!(
        first_of_second.source.path(),
        "/results/1/agents/alice/messages/22"
    );
}

#[test]
fn memory_rewritten_contexts_read_their_own_list() {
    let world = world(MEMORY);
    let alice = exchanges_of(&world, "alice");
    assert_eq!(alice.len(), 9);
    let first_of_second = alice[6];
    let request: Vec<String> = first_of_second
        .request
        .messages
        .iter()
        .filter_map(|id| world.message(*id))
        .filter_map(|m| m.part_text(0).ok().map(|t| t.into_owned()))
        .collect();
    assert!(request[1].starts_with("## Episode 1: communication phase"));
    assert!(
        !request
            .iter()
            .any(|t| t.starts_with("## Episode 1: task phase"))
    );
    assert!(
        request
            .last()
            .is_some_and(|t| t.starts_with("## Episode 2: task phase"))
    );
    assert_eq!(positives(&world).len(), 6);
}

#[test]
fn delivered_messages_are_labelled_where_they_arrive() {
    let world = world(MAIN);
    let labels = positives(&world);
    assert_eq!(labels.len(), 6);
    for label in &labels {
        assert_eq!(label.tier, Tier::Construction);
        assert_eq!(label.route, a2a_bench_format::labels::Route::Direct);
        assert_eq!(
            label.carrier,
            a2a_bench_format::labels::CarrierKind::UserTurn
        );
        let reader = world
            .exchange(label.reader_exchange)
            .unwrap_or_else(|| panic!("reader exchange"));
        assert_eq!(world.agent_of(reader.id), Some(&label.to));
        assert_eq!(label.content.at.exchange, reader.id);
        assert_eq!(label.content.at.part, 0);
        let message = world
            .message(label.content.at.message)
            .unwrap_or_else(|| panic!("the message"));
        assert!(carries(reader, message.id()));
        let text = message.part_text(0).unwrap_or_else(|e| panic!("{e}"));
        let range = label.content.at.range;
        assert_eq!(
            &text[range.start() as usize..range.end() as usize],
            label.content.text
        );
        // It is the first call to carry it: the reader's previous call did not.
        let previous = world
            .exchanges()
            .iter()
            .rev()
            .find(|e| world.agent_of(e.id) == Some(&label.to) && e.at_us < reader.at_us)
            .unwrap_or_else(|| panic!("an earlier call"));
        assert!(!carries(previous, message.id()));
        // The sender's exchange came first.
        let sender = world
            .exchange(
                label
                    .sender_exchange
                    .unwrap_or_else(|| panic!("sender exchange")),
            )
            .unwrap_or_else(|| panic!("sender exchange in world"));
        assert_eq!(world.agent_of(sender.id), Some(&label.from));
        assert!(sender.at_us < reader.at_us);
        let out = response(&world, sender);
        let call_text: Vec<String> = (0..out.part_count())
            .filter_map(|p| {
                out.part_text(u16::try_from(p).ok()?)
                    .ok()
                    .map(|t| t.into_owned())
            })
            .collect();
        assert!(
            call_text
                .iter()
                .any(|t| t.contains("send_message") || t.contains("content"))
        );
    }
    let needs: BTreeMap<&str, &MatchNeed> = labels
        .iter()
        .map(|l| (l.content.text.as_str(), &l.needs))
        .collect();
    assert_eq!(
        needs.get("Bob reports the clinical query returned \"21 rows\"\nwith severity three or higher in every row."),
        Some(&&MatchNeed::Decoded {
            codecs: vec![Codec::JsonString]
        }),
        "escaped inside the sender's tool-call arguments: one level of JSON string decoding"
    );
    assert_eq!(needs.get("OK."), Some(&&MatchNeed::Exact));
}

#[test]
fn rejected_sends_point_at_the_failed_call() {
    let world = world(MAIN);
    let rejected = negatives(&world, NegativeReason::RejectedSend);
    assert_eq!(rejected.len(), 1);
    let label = rejected[0];
    assert_eq!((&label.from, &label.to), (&agent("alice"), &agent("bob")));
    assert_eq!(label.tier, Tier::Construction);
    assert!(label.reader_exchange.is_none() && label.at.is_none());
    assert!(
        label
            .text
            .as_deref()
            .is_some_and(|t| t.starts_with("Raw log follows:"))
    );
    let origin = label.origin.unwrap_or_else(|| panic!("origin"));
    // Placed in the sender's first exchange carrying the failed call: the
    // one whose response it is.
    let sender = world
        .exchange(origin.exchange)
        .unwrap_or_else(|| panic!("the origin's exchange"));
    assert_eq!(world.agent_of(sender.id), Some(&agent("alice")));
    assert_eq!(sender.response.messages, vec![origin.message]);
    let message = response(&world, sender);
    assert!(matches!(
        message.body(),
        Body::Assistant(parts) if matches!(parts.get(usize::from(origin.part)), Some(AssistantPart::ToolCall(_)))
    ));
    let text = message
        .part_text(origin.part)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(origin.range.start(), 0);
    assert_eq!(
        origin.range.end() as usize,
        text.len(),
        "the whole arguments"
    );
    assert!(text.contains("Raw log follows:"));
}

#[test]
fn shared_sources_and_boilerplate_are_negative_controls() {
    let world = world(MAIN);
    let shared = negatives(&world, NegativeReason::SharedSource);
    let system: Vec<_> = shared
        .iter()
        .filter(|c| c.reader_exchange.is_none())
        .collect();
    assert_eq!(
        system.len(),
        2,
        "one per reader, deduplicated across episodes"
    );
    for control in &system {
        // Placed in the reader's first exchange: every call carries the
        // system prompt.
        let at = control.at.unwrap_or_else(|| panic!("at"));
        let first = exchanges_of(&world, control.to.as_str())[0];
        assert_eq!(at.exchange, first.id);
        assert_eq!(first.request.messages.first(), Some(&at.message));
    }
    assert_eq!(
        shared.len() - system.len(),
        4,
        "two shared database results per agent"
    );
    assert_eq!(negatives(&world, NegativeReason::Boilerplate).len(), 22);
    assert!(shared.iter().all(|c| c.tier == Tier::Structural));
    for control in negatives(&world, NegativeReason::Boilerplate) {
        assert_eq!(control.tier, Tier::Structural);
        let at = control.at.unwrap_or_else(|| panic!("at"));
        assert_eq!(Some(at.exchange), control.reader_exchange);
        assert_ne!(control.from, control.to);
    }
}

#[test]
fn label_ids_count_the_worlds_truth_in_order() {
    let world = world(MAIN);
    let ids: Vec<LabelId> = world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::ExchangeAgent(_) => None,
            Label::Transmission(row) => Some(row.fields().id.clone()),
            Label::NegativeControl(row) => Some(row.fields().id.clone()),
            other => panic!("SALT writes no {other:?}"),
        })
        .collect();
    let expected: Vec<LabelId> = (0..ids.len())
        .map(|at| LabelId::new(format!("t{at}")).unwrap_or_else(|e| panic!("{e}")))
        .collect();
    assert_eq!(ids, expected);
    // Per episode: deliveries, then rejected sends, then shared sources and
    // boilerplate.
    let first_control = world
        .labels()
        .iter()
        .position(|l| matches!(l, Label::NegativeControl(_)))
        .unwrap_or_default();
    assert!(matches!(
        world.labels().get(first_control - 1),
        Some(Label::Transmission(_))
    ));
}

#[test]
fn scripted_peers_make_no_exchanges_and_send_from_nowhere() {
    let world = world(CONTROLLED);
    let bob = world
        .decl()
        .agents
        .iter()
        .find(|a| a.key == agent("bob"))
        .unwrap_or_else(|| panic!("bob"));
    assert_eq!(bob.driven, Driven::Scripted);
    assert!(bob.model.is_some(), "the run config's route, as ct-eval");
    assert!(
        world
            .exchanges()
            .iter()
            .all(|e| world.agent_of(e.id) == Some(&agent("alice")))
    );
    assert!(positives(&world).is_empty());
    let scripted = negatives(&world, NegativeReason::NoSenderExchange);
    assert_eq!(scripted.len(), 1);
    let label = scripted[0];
    assert_eq!((&label.from, &label.to), (&agent("bob"), &agent("alice")));
    assert_eq!(label.tier, Tier::Construction);
    assert!(label.at.is_some() && label.reader_exchange.is_some());
    assert!(label.text.is_some());
}

#[test]
fn gemini_thought_signatures_stay_opaque() {
    let world = world(MAIN);
    let mut opaque = 0;
    let mut calls = 0;
    for exchange in exchanges_of(&world, "alice") {
        let message = response(&world, exchange);
        let Body::Assistant(parts) = message.body() else {
            continue;
        };
        for (index, part) in parts.iter().enumerate() {
            match part {
                AssistantPart::ReasoningOpaque => opaque += 1,
                AssistantPart::ToolCall(call) => {
                    calls += 1;
                    assert!(call.call_id.contains("__thought__"), "the id is kept whole");
                    let text = message
                        .part_text(u16::try_from(index).unwrap_or_default())
                        .unwrap_or_default();
                    assert!(!text.contains("__thought__"), "no id in part text");
                }
                _ => {}
            }
        }
    }
    assert!(opaque > 0);
    assert!(calls > 0);
}

#[test]
fn signed_thinking_is_reasoning_with_its_text() {
    let world = world(MAIN);
    let mut reasoning = 0;
    for exchange in world.exchanges() {
        let Body::Assistant(parts) = response(&world, exchange).body() else {
            continue;
        };
        for part in parts {
            if let AssistantPart::Reasoning { text } = part {
                assert!(!text.is_empty());
                reasoning += 1;
            }
        }
    }
    assert!(reasoning > 0, "the fixture's signed thinking blocks");
    // No signature reaches a message: none is a field of any part.
    let all = serde_json::to_string(&world.messages_in_order()).unwrap_or_default();
    assert!(!all.contains("\"signature\""));
}

#[test]
fn accepted_call_mismatch_makes_exchanges_synthetic() {
    let text = std::fs::read_to_string(root().join(MAIN)).unwrap_or_default();
    let mut trace: Trace = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    trace.results[0]
        .llm_usage
        .retain(|u| !(u.actor == "bob" && u.accepted()));
    let world = convert_trace(&trace, MAIN, Pace::DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    let bob_first: Vec<_> = exchanges_of(&world, "bob")
        .into_iter()
        .filter(|e| e.source.path().starts_with("/results/0/"))
        .collect();
    assert!(!bob_first.is_empty());
    assert!(bob_first.iter().all(|e| e.fidelity == Fidelity::Synthetic));
    assert!(
        exchanges_of(&world, "bob")
            .iter()
            .filter(|e| e.source.path().starts_with("/results/1/"))
            .all(|e| e.fidelity == Fidelity::Reconstructed)
    );
}

#[test]
fn gzipped_traces_read_the_same() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let target = dir.path().join("traces/main/main__fixture-model");
    std::fs::create_dir_all(&target).unwrap_or_else(|e| panic!("{e}"));
    let raw = std::fs::read(root().join(MAIN)).unwrap_or_default();
    let file =
        std::fs::File::create(target.join("rep001.json.gz")).unwrap_or_else(|e| panic!("{e}"));
    let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    encoder.write_all(&raw).unwrap_or_else(|e| panic!("{e}"));
    encoder.finish().unwrap_or_else(|e| panic!("{e}"));
    let gz = load_world(
        dir.path(),
        Path::new("traces/main/main__fixture-model/rep001.json.gz"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let plain = world(MAIN);
    assert_eq!(gz.key(), plain.key());
    assert_eq!(gz.exchanges().len(), plain.exchanges().len());
    let texts = |w: &World| {
        positives(w)
            .iter()
            .map(|t| t.content.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&gz), texts(&plain));
    // The source reference names the gzipped file, so ids differ.
    assert_ne!(gz.exchanges()[0].id, plain.exchanges()[0].id);
}

#[test]
fn conversion_is_deterministic() {
    let dump = |world: &World| {
        let labels = serde_json::to_string(world.labels()).unwrap_or_else(|e| panic!("{e}"));
        let exchanges = serde_json::to_string(world.exchanges()).unwrap_or_else(|e| panic!("{e}"));
        let messages =
            serde_json::to_string(&world.messages_in_order()).unwrap_or_else(|e| panic!("{e}"));
        (labels, exchanges, messages)
    };
    assert_eq!(dump(&world(MAIN)), dump(&world(MAIN)));
}

#[test]
fn the_source_streams_one_world_per_file_and_records_what_it_read() {
    let mut salt =
        source(&root(), &Options::default(), Pace::DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(salt.dataset().as_str(), "salt");
    assert_eq!(salt.files().len(), 3);
    assert!(salt.files_read().is_empty());
    let worlds: Vec<_> = salt.worlds().collect();
    assert_eq!(worlds.len(), 3);
    assert!(worlds.iter().all(Result::is_ok));
    assert_eq!(salt.files_read().len(), 3);
    assert!(salt.files_read().digest(&root()).is_ok());
    // Every fixture exchange and delivery (ct-eval's pipeline and reference
    // tests count the same).
    let exchanges: usize = worlds.iter().flatten().map(|w| w.exchanges().len()).sum();
    assert_eq!(exchanges, 37);
    let delivered: usize = worlds.iter().flatten().map(|w| positives(w).len()).sum();
    assert_eq!(delivered, 12);
}

#[test]
fn the_source_skips_worlds_the_export_will_not_keep() {
    let mut salt =
        source(&root(), &Options::default(), Pace::DEFAULT).unwrap_or_else(|e| panic!("{e}"));
    let keep = WorldKey::new("main/main__fixture-model/rep001").unwrap_or_else(|e| panic!("{e}"));
    salt.select(&WorldFilter::Only(BTreeSet::from([keep.clone()])));
    let worlds: Vec<_> = salt.worlds().collect();
    assert_eq!(worlds.len(), 1);
    assert_eq!(worlds[0].as_ref().map(|w| w.key().clone()).ok(), Some(keep));
    assert_eq!(salt.files_read().len(), 1, "skipped files are not read");
}

#[test]
fn a_custom_pace_moves_times_and_ids_but_not_labels() {
    let fast = Pace::new(
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(2),
        0,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let paced = |pace: Pace| {
        let mut salt = source(
            &root(),
            &Options {
                limit: None,
                include: vec!["main/".into()],
            },
            pace,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        salt.worlds()
            .next()
            .unwrap_or_else(|| panic!("a world"))
            .unwrap_or_else(|e| panic!("{e}"))
    };
    let (default, other) = (paced(Pace::DEFAULT), paced(fast));
    assert_eq!(default.exchanges().len(), other.exchanges().len());
    assert_ne!(default.exchanges()[1].at_us, other.exchanges()[1].at_us);
    let texts = |w: &World| {
        positives(w)
            .iter()
            .map(|t| t.content.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(&default), texts(&other));
}

#[test]
fn a_malformed_trace_is_a_typed_error() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
    let path = dir.path().join("traces/x/y/rep001.json");
    std::fs::create_dir_all(path.parent().unwrap_or_else(|| panic!("parent")))
        .unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&path, b"{\"results\": 3}").unwrap_or_else(|e| panic!("{e}"));
    let error = load_world(dir.path(), Path::new("traces/x/y/rep001.json"));
    assert!(matches!(
        error,
        Err(a2a_bench_dataset_salt::SaltError::Json { .. })
    ));
    let missing = load_world(dir.path(), Path::new("traces/x/y/rep002.json"));
    assert!(matches!(
        missing,
        Err(a2a_bench_dataset_salt::SaltError::Io { .. })
    ));
}
