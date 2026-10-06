//! Open-SWE as a background corpus, on synthetic Parquet shards shaped like
//! the dataset (`tests/fixtures/open_swe`, rebuilt by its `make.sql`), and
//! the OpenAI-chat conversion and Parquet reading it rests on. Ported from
//! crosstalk-eval's `tests/open_swe.rs` at 7f8a2fb.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use a2a_bench_corpus::clock::{Pace, compose};
use a2a_bench_corpus::helpers::chat::{
    ChatFunction, ChatMessage, ChatToolCall, bodies, synthetic_call_id,
};
use a2a_bench_corpus::helpers::parquet_rows::{ParquetError, ParquetRows, row_groups};
use a2a_bench_corpus::source::TraceSource;
use a2a_bench_corpus::world::World;
use a2a_bench_dataset_open_swe::{
    AGENTS_PER_WORLD, COLUMNS, DATASET, OpenSweRow, Options, Shard, VERSION, discover, source,
};
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, ExchangeId};
use a2a_bench_format::labels::{Label, NegativeReason, Tier};
use a2a_bench_format::manifest::Setting;
use a2a_bench_format::message::{AssistantPart, Body, Message, ToolPart};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/open_swe")
}

const OPENHANDS: &str = "data/openhands/fixture_model/fixture-set/train-00000-of-00001.parquet";
const SWEAGENT: &str = "data/sweagent/fixture_model/fixture-set/train-00000-of-00001.parquet";
const MINI: &str = "data/minisweagent/fixture_model/fixture-set/train-00000-of-00001.parquet";

fn options(agents_per_world: usize) -> Options {
    Options {
        agents_per_world,
        ..Options::default()
    }
}

fn worlds(options: Options) -> Vec<World> {
    let mut source = source(&root(), &options, Pace::DEFAULT).unwrap();
    source.worlds().map(|world| world.unwrap()).collect()
}

fn rows(relative: &str) -> Vec<(usize, OpenSweRow)> {
    ParquetRows::<OpenSweRow>::open(&root().join(relative), COLUMNS)
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

fn assistant(text: &str, calls: &[(&str, &str)]) -> ChatMessage {
    ChatMessage {
        role: "assistant".into(),
        content: Some(text.into()),
        reasoning_content: None,
        tool_calls: (!calls.is_empty()).then(|| {
            calls
                .iter()
                .map(|(id, name)| ChatToolCall {
                    id: (!id.is_empty()).then(|| (*id).to_owned()),
                    function: ChatFunction {
                        name: (*name).into(),
                        arguments: Some("{\"command\": \"ls\"}".into()),
                    },
                })
                .collect()
        }),
        tool_call_id: None,
    }
}

fn tool(text: &str, id: Option<&str>) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: Some(text.into()),
        tool_call_id: id.map(str::to_owned),
        ..ChatMessage::default()
    }
}

fn result_id(body: &Body) -> Option<String> {
    match body {
        Body::Tool(results) => results
            .first()
            .map(|ToolPart::ToolResult(r)| r.call_id.clone()),
        _ => None,
    }
}

fn result_ids(bodies: &[Body]) -> Vec<String> {
    bodies.iter().filter_map(result_id).collect()
}

fn agents(world: &World) -> Vec<String> {
    world
        .decl()
        .agents
        .iter()
        .map(|agent| agent.key.as_str().to_owned())
        .collect()
}

fn exchanges_of<'a>(world: &'a World, name: &str) -> Vec<&'a a2a_bench_format::exchange::Exchange> {
    world
        .exchanges()
        .iter()
        .filter(|e| world.agent_of(e.id).map(AgentKey::as_str) == Some(name))
        .collect()
}

fn message(world: &World, id: a2a_bench_format::ids::MessageId) -> &Message {
    world.message(id).unwrap()
}

#[test]
fn the_dataset_is_ct_evals() {
    assert_eq!(DATASET, "open_swe");
    assert_eq!(VERSION, 1);
    assert_eq!(AGENTS_PER_WORLD, 16);
    let defaults = Options::default();
    assert_eq!(defaults.agents_per_world, 16);
    assert_eq!(defaults.count, None);
    assert_eq!(
        defaults.settings(),
        BTreeMap::from([("agents_per_world".to_owned(), Setting::Int(16))])
    );
    let set = Options {
        limit: Some(13),
        include: vec!["openhands".into(), "qwen".into()],
        count: Some(16),
        agents_per_world: 4,
    };
    assert_eq!(
        set.settings(),
        BTreeMap::from([
            ("agents_per_world".to_owned(), Setting::Int(4)),
            ("count".to_owned(), Setting::Int(16)),
            (
                "include".to_owned(),
                Setting::List(vec![
                    Setting::Text("openhands".into()),
                    Setting::Text("qwen".into())
                ])
            ),
            ("limit".to_owned(), Setting::Int(13)),
        ])
    );
}

#[test]
fn shards_are_found_filtered_and_limited() {
    let all = discover(&root(), None, &[]).unwrap();
    let relative: Vec<&str> = all.iter().map(|s| s.relative.as_str()).collect();
    assert_eq!(relative, vec![MINI, OPENHANDS, SWEAGENT]);
    assert_eq!(
        all[1],
        Shard {
            relative: OPENHANDS.into(),
            harness: "openhands".into(),
            model: "fixture_model".into(),
            dataset: "fixture-set".into(),
        }
    );
    // `sweagent` is a substring of `minisweagent` too.
    let harness = discover(&root(), None, &["sweagent".into()]).unwrap();
    assert_eq!(harness.len(), 2);
    let limited = discover(&root(), Some(1), &[]).unwrap();
    assert_eq!(limited.len(), 1);
    assert!(discover(Path::new("/nonexistent"), None, &[]).is_err());
    assert_eq!(Shard::parse("data/a/b/rows.jsonl"), None);
}

#[test]
fn parquet_rows_decode_the_projected_columns_with_row_numbers() {
    let read = rows(OPENHANDS);
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].0, 0);
    assert_eq!(read[1].0, 1);
    assert_eq!(read[0].1.repo, "acme/widgets");
    assert_eq!(read[0].1.messages.len(), 11);
    let call = &read[0].1.messages[2].calls()[0];
    assert_eq!(call.function.name, "execute_bash");
    assert_eq!(call.id.as_deref(), Some("chatcmpl-tool-0a0000000000001"));
    assert_eq!(read[0].1.messages[0].tool_calls, None);
    assert_eq!(row_groups(&root().join(OPENHANDS)).unwrap(), vec![2]);
    let missing = ParquetRows::<OpenSweRow>::open(&root().join(OPENHANDS), &["no_such_column"]);
    assert!(matches!(missing, Err(ParquetError::MissingColumn { .. })));
    let absent = ParquetRows::<OpenSweRow>::open(&root().join("data/none.parquet"), COLUMNS);
    assert!(matches!(absent, Err(ParquetError::Open { .. })));
}

#[test]
fn tool_messages_without_ids_answer_calls_in_order() {
    let messages = vec![
        assistant("two at once", &[("call-a", "bash"), ("call-b", "bash")]),
        tool("first", None),
        tool("second", None),
        assistant("", &[("", "bash")]),
        tool("third", None),
        tool("nobody asked", None),
    ];
    let converted = bodies(&messages).unwrap();
    assert_eq!(
        result_ids(&converted),
        vec![
            "call-a".to_owned(),
            "call-b".to_owned(),
            synthetic_call_id(3, 0),
            "unpaired-5".to_owned(),
        ]
    );
}

#[test]
fn explicit_tool_call_ids_win_over_position() {
    let messages = vec![
        assistant("", &[("x", "bash"), ("y", "bash")]),
        tool("answers y", Some("y")),
        tool("answers the oldest open call", None),
    ];
    let converted = bodies(&messages).unwrap();
    assert_eq!(result_ids(&converted), vec!["y".to_owned(), "x".to_owned()]);
}

#[test]
fn assistant_parts_are_reasoning_text_then_calls() {
    let mut message = assistant("visible", &[("c1", "bash")]);
    message.reasoning_content = Some("thinking".into());
    let converted = bodies(&[message]).unwrap();
    let Body::Assistant(parts) = &converted[0] else {
        panic!("not an assistant message");
    };
    assert!(matches!(&parts[0], AssistantPart::Reasoning { text } if text == "thinking"));
    assert!(matches!(&parts[1], AssistantPart::Text { text } if text == "visible"));
    assert!(matches!(&parts[2], AssistantPart::ToolCall(call) if call.call_id == "c1"));
    let empty = bodies(&[assistant("", &[])]).unwrap();
    assert_eq!(empty[0], Body::Assistant(vec![]));
    let unknown = ChatMessage {
        role: "developer".into(),
        ..ChatMessage::default()
    };
    assert!(bodies(&[unknown]).is_err());
}

#[test]
fn trajectories_mix_round_robin_into_worlds() {
    let mixed = worlds(options(4));
    assert_eq!(mixed.len(), 2);
    assert_eq!(
        agents(&mixed[0]),
        vec![
            "minisweagent/fixture_model/fixture-set/0",
            "minisweagent/fixture_model/fixture-set/1",
            "openhands/fixture_model/fixture-set/0",
            "sweagent/fixture_model/fixture-set/0",
        ]
    );
    assert_eq!(
        agents(&mixed[1]),
        vec![
            "openhands/fixture_model/fixture-set/1",
            "sweagent/fixture_model/fixture-set/1",
        ]
    );
    assert_eq!(mixed[0].key().as_str(), "mix-00000");
    assert_eq!(mixed[0].dataset().as_str(), "open_swe");
    let capped = worlds(Options {
        count: Some(1),
        ..Options::default()
    });
    assert_eq!(capped.len(), 1);
    assert_eq!(capped[0].decl().agents.len(), 3);
}

#[test]
fn every_assistant_message_is_one_call_on_a_shared_clock() {
    let world = &worlds(options(16))[0];
    assert_eq!(world.decl().agents.len(), 6);
    let openhands = exchanges_of(world, "openhands/fixture_model/fixture-set/0");
    // Eleven messages, five of them the assistant's.
    assert_eq!(openhands.len(), 5);
    for (call, exchange) in openhands.iter().enumerate() {
        assert_eq!(exchange.response.messages.len(), 1);
        assert_eq!(exchange.source.file(), OPENHANDS);
        assert!(exchange.source.path().starts_with("/rows/0/messages/"));
        // Shards are taken in turn: minisweagent (slot 0), openhands (1), …
        let slot = 1;
        assert_eq!(
            exchange.at_us,
            compose(call as u64, slot, 0).unwrap(),
            "call {call}"
        );
        assert!(exchange.request.messages.len() >= 2);
        assert_eq!(exchange.client.model.as_deref(), Some("fixture_model"));
    }
    // Parallel calls are answered in order.
    let mini = exchanges_of(world, "minisweagent/fixture_model/fixture-set/0");
    let ids: Vec<String> = mini[1]
        .request
        .messages
        .iter()
        .filter_map(|id| result_id(message(world, *id).body()))
        .collect();
    assert_eq!(
        ids,
        vec![
            "chatcmpl-tool-0e0000000000001".to_owned(),
            "chatcmpl-tool-0e0000000000002".to_owned()
        ]
    );
}

#[test]
fn a_pace_moves_every_call() {
    let pace = Pace::new(
        std::time::Duration::from_millis(2_000),
        std::time::Duration::from_millis(8_000),
        5,
    )
    .unwrap();
    let mut paced = source(&root(), &options(16), pace).unwrap();
    let world = paced.worlds().next().unwrap().unwrap();
    for (call, exchange) in exchanges_of(&world, "sweagent/fixture_model/fixture-set/0")
        .iter()
        .enumerate()
    {
        assert_eq!(exchange.at_us, pace.at(call as u64, 2, 0).unwrap());
    }
}

#[test]
fn background_worlds_label_every_pair_negative() {
    let world = &worlds(options(16))[0];
    assert_eq!(
        world.coverage(),
        Coverage::Complete {
            tier: Tier::Construction
        }
    );
    let mut reasons: BTreeMap<NegativeReason, usize> = BTreeMap::new();
    for label in world.labels() {
        match label {
            Label::ExchangeAgent(_) => {}
            Label::NegativeControl(control) => {
                let fields = control.fields();
                assert!(fields.reader_exchange.is_some());
                assert_eq!(fields.tier, Tier::Structural);
                *reasons.entry(fields.reason).or_default() += 1;
            }
            other => panic!("a background world has only negatives, got {other:?}"),
        }
    }
    let exchanges = world.exchanges().len();
    // One control per reader exchange per other trajectory.
    assert_eq!(reasons.values().sum::<usize>(), exchanges * 5);
    // acme/gadgets is worked on by an OpenHands and a SWE-agent trajectory.
    let shared = exchanges_of(world, "openhands/fixture_model/fixture-set/1").len()
        + exchanges_of(world, "sweagent/fixture_model/fixture-set/0").len();
    assert_eq!(reasons.get(&NegativeReason::SharedSource), Some(&shared));
}

/// ct-eval's reference-matcher test over this fixture found a false
/// positive where minisweagent row 0's text reaches OpenHands row 1's tool
/// result. The matcher is another crate; here the fixture's shared phrase
/// and the control it falls on are checked.
#[test]
fn the_shared_phrase_falls_on_a_boilerplate_control() {
    const PHRASE: &str = "stream buffer flushes before the final chunk arrives";
    let world = &worlds(options(16))[0];
    let says = |ids: &[a2a_bench_format::ids::MessageId]| {
        ids.iter().any(|id| {
            let message = message(world, *id);
            (0..message.part_count()).any(|part| {
                u16::try_from(part)
                    .ok()
                    .and_then(|part| message.part_text(part).ok())
                    .is_some_and(|text| text.contains(PHRASE))
            })
        })
    };
    let sender = "minisweagent/fixture_model/fixture-set/0";
    let reader = "openhands/fixture_model/fixture-set/1";
    assert!(
        exchanges_of(world, sender)
            .iter()
            .any(|e| says(&e.response.messages))
    );
    let reads: Vec<ExchangeId> = exchanges_of(world, reader)
        .iter()
        .filter(|e| says(&e.request.messages))
        .map(|e| e.id)
        .collect();
    assert!(!reads.is_empty());
    for read in reads {
        assert!(world.labels().iter().any(|label| matches!(label,
            Label::NegativeControl(c)
                if c.fields().from.as_str() == sender
                    && c.fields().reader_exchange == Some(read)
                    && c.fields().reason == NegativeReason::Boilerplate)));
    }
}

#[test]
fn the_files_read_are_the_shards_opened() {
    let mut source = source(&root(), &options(16), Pace::DEFAULT).unwrap();
    assert!(source.files_read().is_empty());
    let count = source.worlds().count();
    assert_eq!(count, 1);
    assert_eq!(source.files_read().len(), 3);
    let digest = source.files_read().digest(&root()).unwrap();
    let all = a2a_bench_corpus::export::source_digest(
        &root(),
        [MINI, OPENHANDS, SWEAGENT].map(PathBuf::from),
    )
    .unwrap();
    assert_eq!(digest, all);
}

#[test]
fn a_rerun_is_identical() {
    let summary = |worlds: Vec<World>| -> Vec<_> {
        worlds
            .iter()
            .map(|w| (w.key().clone(), w.exchanges().to_vec(), w.labels().to_vec()))
            .collect()
    };
    assert_eq!(summary(worlds(options(3))), summary(worlds(options(3))));
}
