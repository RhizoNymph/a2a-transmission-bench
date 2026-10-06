//! Labels through the builder: checked constructors refuse bad rows, the
//! builder's finish refuses rows that do not fit the world, and truth
//! round-trips through an export.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs::File;
use std::io::BufReader;

use a2a_bench_corpus::export::{LABELS_FILE, export};
use a2a_bench_corpus::source::InMemory;
use a2a_bench_corpus::split::Selection;
use a2a_bench_corpus::world::{CorpusError, WorldBuilder};
use a2a_bench_format::check::LabelError;
use a2a_bench_format::files::{Coverage, Labels};
use a2a_bench_format::ids::SourceRef;
use a2a_bench_format::jsonl::FileReader;
use a2a_bench_format::labels::{
    AgentCluster, ClusterFields, ClusterKind, ControlFields, ExpectedTransmission, InvalidLabel,
    Label, NegativeControl, NegativeReason, Tier,
};
use common::{
    calls, dataset, draft, label_id, location, ok, result, sample_world, says, system,
    transmission, user, world_key,
};

#[test]
fn transmissions_need_two_agents() {
    let mut builder = WorldBuilder::new(dataset(), world_key("w"));
    let bob = ok(builder.model_agent("bob", "m"));
    let delivered = result("c", "x: hello");
    let b1 = ok(builder.exchange(draft(&bob, 1, vec![delivered.clone()], says("ok"))));
    let fields = transmission(
        "t",
        &bob,
        &bob,
        None,
        b1,
        location(b1, &delivered, 3, "hello"),
        "hello",
    );
    assert!(matches!(
        ExpectedTransmission::new(fields),
        Err(InvalidLabel::SelfTransmission(_))
    ));
}

#[test]
fn content_text_must_fill_its_location() {
    let world = sample_world("w", common::SECRET);
    let Some(Label::Transmission(label)) = world.labels().last() else {
        panic!("no transmission")
    };
    let mut bad = label.fields().clone();
    bad.content.text = "the".into();
    assert!(matches!(
        ExpectedTransmission::new(bad),
        Err(InvalidLabel::ContentLength { .. })
    ));
}

fn two_agents() -> (
    WorldBuilder,
    a2a_bench_corpus::world::WorldAgent,
    a2a_bench_corpus::world::WorldAgent,
    a2a_bench_format::ids::ExchangeId,
    a2a_bench_format::message::Message,
) {
    let mut builder = WorldBuilder::new(dataset(), world_key("w"));
    let alice = ok(builder.model_agent("alice", "m"));
    let bob = ok(builder.model_agent("bob", "m"));
    ok(builder.exchange(draft(
        &alice,
        1,
        vec![user("go")],
        calls("c", "send", r#"{"t":"hello"}"#),
    )));
    let delivered = result("c", "x: hello");
    let b1 = ok(builder.exchange(draft(
        &bob,
        2,
        vec![system("s"), delivered.clone()],
        says("ok"),
    )));
    (builder, alice, bob, b1, delivered)
}

#[test]
fn content_must_be_the_text_at_its_location() {
    let (mut builder, alice, bob, b1, delivered) = two_agents();
    // Right length, wrong text.
    let mut fields = transmission(
        "t",
        &alice,
        &bob,
        None,
        b1,
        location(b1, &delivered, 3, "hello"),
        "hello",
    );
    fields.content.text = "jello".into();
    ok(builder.label(Label::Transmission(ok(ExpectedTransmission::new(fields)))));
    assert!(matches!(
        builder.finish(Coverage::Partial),
        Err(CorpusError::Labels {
            source: LabelError::ContentMismatch { .. },
            ..
        })
    ));
}

#[test]
fn labels_name_the_reader_exchange_of_the_reader() {
    let (mut builder, alice, bob, b1, delivered) = two_agents();
    // bob is not the sender; alice did not make b1.
    let fields = transmission(
        "t",
        &bob,
        &alice,
        None,
        b1,
        location(b1, &delivered, 3, "hello"),
        "hello",
    );
    ok(builder.label(Label::Transmission(ok(ExpectedTransmission::new(fields)))));
    assert!(matches!(
        builder.finish(Coverage::Partial),
        Err(CorpusError::Labels {
            source: LabelError::WrongAgent { .. },
            ..
        })
    ));
}

#[test]
fn labels_name_declared_agents_and_unique_ids() {
    let (mut builder, _, bob, b1, delivered) = two_agents();
    let ghost =
        a2a_bench_corpus::world::WorldAgent::new(world_key("w"), common::agent_key("ghost"));
    let fields = transmission(
        "t",
        &ghost,
        &bob,
        None,
        b1,
        location(b1, &delivered, 3, "hello"),
        "hello",
    );
    ok(builder.label(Label::Transmission(ok(ExpectedTransmission::new(fields)))));
    assert!(matches!(
        builder.finish(Coverage::Partial),
        Err(CorpusError::Labels {
            source: LabelError::UnknownAgent { .. },
            ..
        })
    ));

    let (mut builder, alice, bob, b1, delivered) = two_agents();
    for _ in 0..2 {
        let fields = transmission(
            "t",
            &alice,
            &bob,
            None,
            b1,
            location(b1, &delivered, 3, "hello"),
            "hello",
        );
        ok(builder.label(Label::Transmission(ok(ExpectedTransmission::new(fields)))));
    }
    assert!(matches!(
        builder.finish(Coverage::Partial),
        Err(CorpusError::Labels {
            source: LabelError::DuplicateId(_),
            ..
        })
    ));
}

#[test]
fn negative_controls_must_be_bounded() {
    let unbounded = ControlFields {
        id: label_id("n"),
        from: common::agent_key("alice"),
        to: common::agent_key("bob"),
        reader_exchange: None,
        at: None,
        origin: None,
        text: None,
        reason: NegativeReason::RejectedSend,
        tier: Tier::Construction,
        source: SourceRef::new("f", "/e"),
    };
    assert!(matches!(
        NegativeControl::new(unbounded.clone()),
        Err(InvalidLabel::Unbounded)
    ));
    let (mut builder, _, _, b1, _) = two_agents();
    let bounded = ControlFields {
        reader_exchange: Some(b1),
        ..unbounded
    };
    ok(builder.label(Label::NegativeControl(ok(NegativeControl::new(bounded)))));
    assert!(builder.finish(Coverage::Partial).is_ok());
}

#[test]
fn clusters_need_two_agents() {
    let one = ClusterFields {
        id: label_id("c"),
        agents: vec![common::agent_key("a")],
        cluster: ClusterKind::Identity,
        tier: Tier::Structural,
        source: SourceRef::new("f", "/"),
    };
    assert!(matches!(
        AgentCluster::new(one),
        Err(InvalidLabel::SmallCluster)
    ));
}

#[test]
fn truth_round_trips_through_an_export() {
    let worlds = vec![
        sample_world("w1", "first secret"),
        sample_world("w2", "second secret"),
    ];
    let expected: Vec<Vec<Label>> = worlds.iter().map(|w| w.labels().to_vec()).collect();
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("export");
    let info = common::info("r1");
    let mut source = InMemory::new(dataset(), worlds);
    ok(export(&mut source, &out, info, &Selection::Unsplit));
    let file = ok(File::open(out.join(LABELS_FILE)));
    let mut reader = ok(FileReader::<Labels, _>::open(BufReader::new(file)));
    let mut back = Vec::new();
    while let Some(section) = ok(reader.next_world()) {
        assert_eq!(
            section.world.coverage,
            Coverage::Complete {
                tier: Tier::Construction
            }
        );
        back.push(section.rows);
    }
    assert_eq!(back, expected);
}

#[test]
fn invalid_labels_are_refused_when_read() {
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("export");
    let info = common::info("r1");
    let mut source = InMemory::new(dataset(), vec![sample_world("w1", common::SECRET)]);
    ok(export(&mut source, &out, info, &Selection::Unsplit));
    let text = ok(std::fs::read_to_string(out.join(LABELS_FILE)));
    let tampered = text.replace(
        "\"from\":\"alice\",\"to\":\"bob\"",
        "\"from\":\"bob\",\"to\":\"bob\"",
    );
    assert_ne!(tampered, text);
    let mut reader = ok(FileReader::<Labels, _>::open(tampered.as_bytes()));
    assert!(reader.next_world().is_err());
}
