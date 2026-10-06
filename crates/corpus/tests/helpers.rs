//! Converter helpers: OpenAI-chat conversion, background worlds, the seeded
//! generator, and the source digest.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_corpus::clock;
use a2a_bench_corpus::export::{FilesRead, source_digest};
use a2a_bench_corpus::helpers::background::{BackgroundWorld, Call, Trajectory};
use a2a_bench_corpus::helpers::chat::{
    ChatError, ChatFunction, ChatMessage, ChatToolCall, bodies, convert,
};
use a2a_bench_corpus::helpers::media::media_kind;
use a2a_bench_corpus::helpers::rng::SplitMix64;
use a2a_bench_corpus::world::StopReason;
use a2a_bench_format::exchange::Fidelity;
use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::SourceRef;
use a2a_bench_format::labels::{Label, NegativeReason, Tier};
use a2a_bench_format::message::{AssistantPart, Body, MediaKind, ToolArguments, ToolPart};
use common::{dataset, ok, says, user, world_key};

fn chat(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: Some(content.into()),
        ..ChatMessage::default()
    }
}

fn call(id: Option<&str>, name: &str, arguments: &str) -> ChatToolCall {
    ChatToolCall {
        id: id.map(str::to_owned),
        function: ChatFunction {
            name: name.into(),
            arguments: Some(arguments.into()),
        },
    }
}

#[test]
fn chat_messages_pair_results_with_calls() {
    let mut assistant = chat("assistant", "");
    assistant.reasoning_content = Some("thinking".into());
    assistant.tool_calls = Some(vec![
        call(None, "ls", r#"{"b": 1, "a": 2}"#),
        call(Some("x"), "cat", "not json"),
    ]);
    let mut named = chat("tool", "file");
    named.tool_call_id = Some("x".into());
    let messages = vec![
        chat("system", "sys"),
        chat("user", "do it"),
        assistant,
        named,
        chat("tool", "listing"),
        chat("tool", "stray"),
    ];
    let out = ok(bodies(&messages));
    let Body::Assistant(parts) = &out[2] else {
        panic!("assistant")
    };
    assert!(matches!(&parts[0], AssistantPart::Reasoning { text } if text == "thinking"));
    let AssistantPart::ToolCall(first) = &parts[1] else {
        panic!("call")
    };
    assert_eq!(first.call_id, "call-2-0");
    assert!(
        matches!(&first.arguments, ToolArguments::Json(json) if json.as_str() == r#"{"a":2,"b":1}"#)
    );
    let AssistantPart::ToolCall(second) = &parts[2] else {
        panic!("call")
    };
    assert!(matches!(&second.arguments, ToolArguments::Invalid(text) if text == "not json"));
    let call_of = |body: &Body| match body {
        Body::Tool(parts) => match &parts[0] {
            ToolPart::ToolResult(result) => result.call_id.clone(),
        },
        other => panic!("not a tool message: {other:?}"),
    };
    assert_eq!(call_of(&out[3]), "x");
    assert_eq!(call_of(&out[4]), "call-2-0");
    assert_eq!(call_of(&out[5]), "unpaired-5");
    assert_eq!(ok(convert(&messages)).len(), messages.len());
    assert!(matches!(
        bodies(&[chat("narrator", "x")]),
        Err(ChatError::UnknownRole { index: 0, .. })
    ));
}

fn trajectory(name: &str, group: &str, start: u64) -> Trajectory {
    Trajectory {
        name: name.into(),
        model: "openai/gpt-x".into(),
        group: group.into(),
        calls: (0..2)
            .map(|i| Call {
                at: ok(clock::compose(start + i, 0, 0)),
                request: vec![user(&format!("{name} step {i}"))],
                response: says(&format!("{name} done {i}")),
                stop: StopReason::EndTurn,
                fidelity: Fidelity::Reconstructed,
                source: SourceRef::new(format!("{name}.json"), format!("/{i}")),
            })
            .collect(),
    }
}

#[test]
fn background_worlds_control_every_pair() {
    let mut world = BackgroundWorld::new(dataset(), world_key("bg"));
    ok(world.add(trajectory("a", "repo-1", 0)));
    ok(world.add(trajectory("b", "repo-1", 1)));
    ok(world.add(trajectory("c", "repo-2", 2)));
    let world = ok(world.finish());
    assert_eq!(
        world.coverage(),
        Coverage::Complete {
            tier: Tier::Construction
        }
    );
    let controls: Vec<_> = world
        .labels()
        .iter()
        .filter_map(|label| match label {
            Label::NegativeControl(control) => Some(control.fields().clone()),
            _ => None,
        })
        .collect();
    // 3 readers × 2 senders × 2 exchanges.
    assert_eq!(controls.len(), 12);
    let reason = |from: &str, to: &str| {
        controls
            .iter()
            .find(|c| c.from.as_str() == from && c.to.as_str() == to)
            .map(|c| c.reason)
    };
    assert_eq!(reason("a", "b"), Some(NegativeReason::SharedSource));
    assert_eq!(reason("a", "c"), Some(NegativeReason::Boilerplate));
    assert!(controls.iter().all(|c| c.id.as_str().starts_with("bg/")));
}

#[test]
fn the_generator_is_seeded_and_stable() {
    let draw = |seed| {
        let mut rng = SplitMix64::new(seed);
        (0..4).map(|_| rng.next_u64()).collect::<Vec<_>>()
    };
    assert_eq!(draw(1), draw(1));
    assert_ne!(draw(1), draw(2));
    // SplitMix64's published first output for seed 0.
    assert_eq!(SplitMix64::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    let mut a = SplitMix64::derived(7, "pairs");
    let mut b = SplitMix64::derived(7, "payloads");
    assert_ne!(a.next_u64(), b.next_u64());
    let mut rng = SplitMix64::new(3);
    assert_eq!(rng.below(0), None);
    assert!((0..100).all(|_| rng.below(5).is_some_and(|x| x < 5)));
    let mut items = [1, 2, 3, 4, 5];
    rng.shuffle(&mut items);
    let mut sorted = items;
    sorted.sort();
    assert_eq!(sorted, [1, 2, 3, 4, 5]);
}

#[test]
fn the_source_digest_covers_names_and_contents_in_path_order() {
    let dir = ok(tempfile::tempdir());
    let root = dir.path();
    ok(std::fs::create_dir_all(root.join("sub")));
    ok(std::fs::write(root.join("a.json"), b"alpha"));
    ok(std::fs::write(root.join("sub/b.json"), b"beta"));
    let forward = ok(source_digest(
        root,
        ["a.json".into(), root.join("sub/b.json")],
    ));
    let backward = ok(source_digest(
        root,
        [root.join("sub/b.json"), "a.json".into()],
    ));
    assert_eq!(forward, backward);
    let mut files = FilesRead::new();
    files.record("sub/b.json");
    files.record(root.join("a.json"));
    files.record("a.json");
    assert_eq!(files.len(), 3);
    assert_eq!(ok(files.digest(root)), forward);
    // Contents and names both count.
    ok(std::fs::write(root.join("a.json"), b"alphA"));
    assert_ne!(
        ok(source_digest(root, ["a.json".into(), "sub/b.json".into()])),
        forward
    );
    ok(std::fs::write(root.join("a.json"), b"alpha"));
    ok(std::fs::rename(root.join("a.json"), root.join("c.json")));
    assert_ne!(
        ok(source_digest(root, ["c.json".into(), "sub/b.json".into()])),
        forward
    );
    // Paths outside the root, or climbing out of it, are refused.
    assert!(source_digest(root, ["/elsewhere/x".into()]).is_err());
    assert!(source_digest(root, ["../x".into()]).is_err());
}

#[test]
fn media_types_map_to_kinds() {
    for (given, kind) in [
        ("image/png", MediaKind::Image),
        ("IMAGE/JPEG", MediaKind::Image),
        ("image", MediaKind::Image),
        ("audio/wav", MediaKind::Audio),
        ("application/pdf", MediaKind::Document),
        ("application/pdf; name=a.pdf", MediaKind::Document),
        (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            MediaKind::Document,
        ),
        ("text/plain", MediaKind::Document),
        ("video/mp4", MediaKind::Other),
        ("application/zip", MediaKind::Other),
        ("", MediaKind::Other),
    ] {
        assert_eq!(media_kind(given), kind, "{given}");
    }
}
