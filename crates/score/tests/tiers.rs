//! Rows kept out of `overall`: forwarding and out-of-reach labels (ported
//! from crosstalk-eval's `tests/forwarding.rs` scoring and report parts and
//! `tests/unobserved.rs`), access-only recall of content labels, and
//! exemptions.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, ExchangeId, SourceRef, TransmissionRef};
use a2a_bench_format::labels::{
    CarrierKind, Exemption, ExemptionFields, ExemptionReason, ExpectedTransmission, Label,
    MatchClass, MatchNeed, Route, Tier,
};
use a2a_bench_format::location::Location;
use a2a_bench_format::predictions::Quality;
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::Prediction;
use a2a_bench_score::report::table::render;
use a2a_bench_score::report::{Disclosure, Report};
use a2a_bench_score::run::RunSummary;
use a2a_bench_score::score::{Scorer, Selector};
use common::{Built, Draft, calls, complete, content_fields, result, says, system, user, whole};

const LOG: &str = "seq 1 read_code src/ledger.py ok; seq 2 query_database SELECT total FROM orders WHERE month = 'march' ok; seq 3 resolve_records vendor 4471 ok";
const NOTE: &str =
    "Bob, the March ledger total is off by exactly forty-two dollars and nine cents.";

struct Forwarding {
    built: Built,
    alice: AgentKey,
    bob: AgentKey,
    /// Bob's read of the forwarded log.
    b3: ExchangeId,
    log_at: Location,
    /// Bob's read of Alice's own note.
    b5: ExchangeId,
    note_at: Location,
}

/// Alice reads a log through `get_log` and sends it to Bob verbatim; Bob
/// reads it in his next user turn. One `Forwarding` label, and one
/// `Construction` label for a note Alice wrote herself.
fn forwarding_world() -> Forwarding {
    let mut draft = Draft::new("forwarding");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    let (sys_a, sys_b) = (system("You are Alice."), system("You are Bob."));
    let start = user("start");
    let get_log = calls("call_log", "get_log", "{}");
    let log = result("call_log", LOG);
    let send_log = calls(
        "call_s1",
        "send_message",
        &serde_json::json!({ "content": LOG }).to_string(),
    );
    let sent = result("call_s1", "ok");
    let send_note = calls(
        "call_s2",
        "send_message",
        &serde_json::json!({ "content": NOTE }).to_string(),
    );
    let (to_bob_log, to_bob_note) = (user(LOG), user(NOTE));
    let thanks = says("thanks");
    draft.exchange(&alice, 1, &[&sys_a, &start], &[&get_log]);
    let a2 = draft.exchange(&alice, 2, &[&sys_a, &start, &get_log, &log], &[&send_log]);
    let b3 = draft.exchange(&bob, 3, &[&sys_b, &to_bob_log], &[&thanks]);
    let a4 = draft.exchange(
        &alice,
        4,
        &[&sys_a, &start, &get_log, &log, &send_log, &sent],
        &[&send_note],
    );
    let b5 = draft.exchange(
        &bob,
        5,
        &[&sys_b, &to_bob_log, &thanks, &to_bob_note],
        &[&says("noted")],
    );
    let log_at = whole(b3, &to_bob_log, 0);
    let note_at = whole(b5, &to_bob_note, 0);
    for (id, text, sender, reader, location, tier) in [
        ("log", LOG, a2, b3, log_at, Tier::Forwarding),
        ("note", NOTE, a4, b5, note_at, Tier::Construction),
    ] {
        let mut fields = content_fields(
            id,
            &alice,
            &bob,
            reader,
            Route::Direct,
            CarrierKind::UserTurn,
            text,
            location,
            tier,
        );
        fields.sender_exchange = Some(sender);
        draft.expect(Label::Transmission(
            ExpectedTransmission::new(fields).unwrap(),
        ));
    }
    Forwarding {
        built: draft.finish(complete()),
        alice,
        bob,
        b3,
        log_at,
        b5,
        note_at,
    }
}

fn exact(
    id: &str,
    from: &AgentKey,
    to: &AgentKey,
    reader: ExchangeId,
    read_at: Location,
) -> Prediction {
    Prediction {
        transmission: TransmissionRef::new(id).unwrap(),
        from: from.clone(),
        to: to.clone(),
        reader_exchange: reader,
        route: Route::Direct,
        carrier: CarrierKind::UserTurn,
        class: EvidenceClass::Exact,
        quality: Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::UserTurn,
        },
        read_at,
        origin_at: None,
    }
}

fn report(built: &Built, predictions: &[Prediction]) -> Report {
    let mut scorer = Scorer::new(10);
    scorer.add_world(&built.world(), predictions);
    let summary = RunSummary::new(common::dataset(), common::detector(), scorer.finish());
    Report::new(summary, Vec::new(), Disclosure::Full)
}

#[test]
fn forwarding_labels_are_kept_apart_from_overall() {
    let w = forwarding_world();
    let note = exact("t:note", &w.alice, &w.bob, w.b5, w.note_at);
    // A detector with forwarding off finds only the note.
    let off = report(&w.built, std::slice::from_ref(&note));
    assert_eq!(off.forwarding.counts.expected, 1);
    assert_eq!(off.forwarding.counts.found, 0, "{off:#?}");
    // Overall holds the construction label only: the known miss is not
    // charged to it.
    assert_eq!(off.overall.counts.expected, 1);
    assert_eq!(off.overall.counts.found, 1, "{off:#?}");

    // With forwarding on, it finds both.
    let log = exact("t:log", &w.alice, &w.bob, w.b3, w.log_at);
    let on = report(&w.built, &[note, log]);
    assert_eq!(on.forwarding.counts.expected, 1);
    assert_eq!(on.forwarding.counts.found, 1, "{on:#?}");
    assert_eq!(on.overall.counts.expected, 1);
    assert_eq!(on.overall.counts.found, 1);
    assert!(
        render(&on).contains("forwarding (sender relayed"),
        "the table shows the forwarding summary"
    );
    assert!(!render(&on).contains("out of reach"));
}

/// Alice's text reaches Bob from a medium she never wrote: out of reach,
/// counted in the row of its arrival class.
fn unobserved_world() -> (Built, AgentKey, AgentKey, ExchangeId, Location) {
    let mut draft = Draft::new("unobserved");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    draft.exchange(&alice, 1, &[&user("write")], &[&says(NOTE)]);
    let page = user(NOTE);
    let reads = draft.exchange(&bob, 2, &[&page], &[&says("ok")]);
    let location = whole(reads, &page, 0);
    let mut fields = content_fields(
        "copy",
        &alice,
        &bob,
        reads,
        Route::Unobserved,
        CarrierKind::UserTurn,
        NOTE,
        location,
        Tier::OutOfReach,
    );
    fields.needs = MatchNeed::Unobserved {
        reason: "sender medium unobserved".into(),
        arrival: MatchClass::Normalized,
    };
    draft.expect(Label::Transmission(
        ExpectedTransmission::new(fields).unwrap(),
    ));
    (draft.finish(complete()), alice, bob, reads, location)
}

#[test]
fn an_unobserved_label_is_out_of_reach_in_its_arrival_class_row() {
    let (built, ..) = unobserved_world();
    let report = report(&built, &[]);
    assert_eq!(report.overall.counts.expected, 0);
    assert_eq!(report.out_of_reach.counts.expected, 1);
    assert_eq!(report.out_of_reach.counts.missed, 1);
    let row = report
        .rows
        .iter()
        .find(|row| row.counts.expected == 1)
        .unwrap();
    assert_eq!(row.key.class, EvidenceClass::Normalized);
    assert_eq!(row.key.tier, Some(Tier::OutOfReach));
    assert!(render(&report).contains("out of reach (missed by design"));
}

#[test]
fn an_undecodable_label_is_counted_as_decoded() {
    let (mut built, ..) = unobserved_world();
    for label in &mut built.labels {
        if let Label::Transmission(expected) = label {
            let mut fields = expected.fields().clone();
            fields.needs = MatchNeed::Undecodable {
                codec: "json_string+json_string".into(),
            };
            *expected = ExpectedTransmission::new(fields).unwrap();
        }
    }
    let mut scorer = Scorer::new(0);
    scorer.add_world(&built.world(), &[]);
    let score = scorer.finish();
    let decoded = score.total(&Selector {
        class: Some(EvidenceClass::Decoded),
        tier: Some(Tier::OutOfReach),
        ..Selector::default()
    });
    assert_eq!(decoded.expected, 1);
}

#[test]
fn a_content_label_only_a_suspected_prediction_found_is_access_only_recall() {
    let w = forwarding_world();
    let mut suspected = exact("t:s", &w.alice, &w.bob, w.b5, w.note_at);
    suspected.class = EvidenceClass::Suspected;
    suspected.quality = Quality::Suspected;
    let report = report(&w.built, &[suspected]);
    assert_eq!(report.overall.counts.expected, 1);
    assert_eq!(report.overall.counts.found, 0);
    assert_eq!(report.overall.counts.missed, 1);
    assert_eq!(report.overall.counts.suspected, 1);
    assert_eq!(report.access_only.labels, 1);
    assert_eq!(report.access_only.expected, 1);
    assert_eq!(report.access_only.recall, Some(1.0));
    assert!(render(&report).contains("access-only recall (suspected or discarded only"));
}

#[test]
fn an_exempt_prediction_is_unjudged_whoever_its_sender() {
    let mut w = forwarding_world();
    w.built.labels.push(Label::Exemption(
        Exemption::new(ExemptionFields {
            id: common::label_id("exempt"),
            to: w.bob.clone(),
            reader_exchange: w.b3,
            at: Location {
                range: a2a_bench_format::location::ByteRange::new(0, 5).unwrap(),
                ..w.log_at
            },
            text: None,
            reason: ExemptionReason::UnknownSender,
            tier: Tier::Heuristic,
            source: SourceRef::new("f", "/exempt"),
        })
        .unwrap(),
    ));
    // Bob is no sender, and the prediction reads the exempt place only
    // through overlap: unjudged, not false, under complete coverage.
    let mut stray = exact("t:x", &w.bob, &w.alice, w.b3, w.log_at);
    stray.from = common::agent_key("carol");
    stray.to = w.bob.clone();
    let mut scorer = Scorer::new(0);
    scorer.add_world(&w.built.world(), &[stray]);
    let all = scorer.finish().total(&Selector::default());
    assert_eq!((all.unjudged, all.false_positive), (1, 0));
}

#[test]
fn coverage_decides_an_unlabelled_prediction() {
    let w = forwarding_world();
    let stray = exact("t:x", &w.bob, &w.alice, w.b3, w.log_at);
    for (coverage, unjudged, false_positive, tier) in [
        (Coverage::Partial, 1, 0, None),
        (
            Coverage::Complete {
                tier: Tier::Heuristic,
            },
            0,
            1,
            Some(Tier::Heuristic),
        ),
    ] {
        let mut world = w.built.world();
        world.coverage = coverage;
        let mut scorer = Scorer::new(0);
        scorer.add_world(&world, std::slice::from_ref(&stray));
        let score = scorer.finish();
        let all = score.total(&Selector::default());
        assert_eq!(
            (all.unjudged, all.false_positive),
            (unjudged, false_positive)
        );
        let row = score.rows.iter().find(|row| row.counts.predicted == 1);
        assert_eq!(row.map(|row| row.key.tier), Some(tier));
    }
}
