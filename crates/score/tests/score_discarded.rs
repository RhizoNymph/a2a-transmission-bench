//! A discarded transmission is the detector's "no": scoring it as a
//! reported transmission charged the node0 bench's reread controls
//! (demo-swarm headline 20261005T184212Z: one reread violation, every one
//! of them a `discarded` co-access opened on the reread and discarded).
//!
//! Minimal shape: Alice reads Bob's page (a label), then rereads the same
//! version in a later exchange (a `reread` control at that read). The
//! detector confirms the first read and discards the co-access the reread
//! opened. A second discarded co-access lands where no label is, under
//! complete coverage (an older writer of a page whose read carried a later
//! writer's version).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use a2a_bench_format::ids::{AgentKey, ExchangeId, TransmissionRef};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedTransmission, Label, MatchClass, NegativeReason, Route, RouteKind, Tier,
};
use a2a_bench_format::location::Location;
use a2a_bench_format::predictions::PredictedRoute;
use a2a_bench_format::predictions::Quality;
use a2a_bench_format::resource::Resource;
use a2a_bench_score::AsGiven;
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::Prediction;
use a2a_bench_score::report::gates::{GateStatus, Gates};
use a2a_bench_score::report::table::render;
use a2a_bench_score::report::{Disclosure, Report};
use a2a_bench_score::run::RunSummary;
use a2a_bench_score::score::judge::{Judge, Outcome};
use a2a_bench_score::score::{Score, Scorer, Selector};
use common::{Built, Draft, complete, content_fields, control, says, user, whole};

const PAGE: &str = "queue backpressure: redelivery after 242 ms, then drop";

struct Scene {
    built: Built,
    alice: AgentKey,
    bob: AgentKey,
    carol: AgentKey,
    /// Alice's first read of Bob's page.
    first: ExchangeId,
    /// Alice's reread of the same version.
    reread: ExchangeId,
    first_at: Location,
    reread_at: Location,
}

fn page() -> Resource {
    Resource::File {
        host: None,
        path: "/pages/queue-backpressure".into(),
    }
}

fn scene() -> Scene {
    let mut draft = Draft::new("w");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    let carol = draft.agent("carol");
    draft.exchange(&bob, 1, &[&user("write the page")], &[&says(PAGE)]);
    let read = user(PAGE);
    let noted = says("noted");
    let first = draft.exchange(&alice, 2, &[&read], &[&noted]);
    let again = user(&format!("again: {PAGE}"));
    let reread = draft.exchange(
        &alice,
        3,
        &[&read, &noted, &again],
        &[&says("same as before")],
    );
    let first_at = whole(first, &read, 0);
    let reread_at = whole(reread, &again, 0);
    draft.expect(Label::Transmission(
        ExpectedTransmission::new(content_fields(
            "page",
            &bob,
            &alice,
            first,
            Route::Channel { resource: page() },
            CarrierKind::ToolResult,
            PAGE,
            first_at,
            Tier::Construction,
        ))
        .unwrap(),
    ));
    draft.expect(control(
        "reread",
        &bob,
        &alice,
        Some(reread),
        Some(reread_at),
        None,
        NegativeReason::Reread,
        Tier::Construction,
    ));
    Scene {
        built: draft.finish(complete()),
        alice,
        bob,
        carol,
        first,
        reread,
        first_at,
        reread_at,
    }
}

fn tref(id: u32) -> TransmissionRef {
    TransmissionRef::new(format!("t:{id}")).unwrap()
}

/// The confirmed first read: an exact match in Alice's first exchange.
fn confirmed(scene: &Scene) -> Prediction {
    Prediction {
        transmission: tref(1),
        from: scene.bob.clone(),
        to: scene.alice.clone(),
        reader_exchange: scene.first,
        route: PredictedRoute::Channel {
            resources: vec![page()],
        },
        carrier: CarrierKind::ToolResult,
        class: EvidenceClass::Exact,
        quality: Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::ToolResult,
        },
        read_at: scene.first_at,
        origin_at: None,
    }
}

/// The co-access the reread opened, discarded: located at the whole tool
/// result of the reread, as a co-access prediction is.
fn discarded_reread(scene: &Scene) -> Prediction {
    Prediction {
        transmission: tref(2),
        reader_exchange: scene.reread,
        class: EvidenceClass::Discarded,
        quality: Quality::Discarded,
        read_at: scene.reread_at,
        ..confirmed(scene)
    }
}

/// A discarded co-access from a writer no label names (Carol wrote an
/// older version of the page).
fn discarded_stale_writer(scene: &Scene) -> Prediction {
    Prediction {
        transmission: tref(3),
        from: scene.carol.clone(),
        class: EvidenceClass::Discarded,
        quality: Quality::Discarded,
        ..confirmed(scene)
    }
}

fn score(scene: &Scene, predictions: &[Prediction]) -> Score {
    let mut scorer = Scorer::new(10);
    scorer.add_world(&scene.built.world(), predictions);
    scorer.finish()
}

fn report(scene: &Scene, predictions: &[Prediction]) -> Report {
    let summary = RunSummary::new(
        common::dataset(),
        common::detector(),
        score(scene, predictions),
    );
    Report::new(summary, Vec::new(), Disclosure::Full)
}

#[test]
fn a_discarded_prediction_aligned_with_nothing_is_dismissed() {
    let scene = scene();
    let world = scene.built.world();
    let judge = Judge::new(&world, &AsGiven);
    assert_eq!(judge.judge(&discarded_reread(&scene)).0, Outcome::Dismissed);
    assert_eq!(
        judge.judge(&discarded_stale_writer(&scene)).0,
        Outcome::Dismissed
    );
}

#[test]
fn a_discarded_reread_is_no_violation_and_no_false_positive() {
    let scene = scene();
    let predictions = [
        confirmed(&scene),
        discarded_reread(&scene),
        discarded_stale_writer(&scene),
    ];
    let score = score(&scene, &predictions);
    assert_eq!(
        score.violation_count(None, None),
        0,
        "{:?}",
        score.violations
    );
    assert!(
        score.false_positives.is_empty(),
        "{:?}",
        score.false_positives
    );
    let discarded = score.total(&Selector {
        class: Some(EvidenceClass::Discarded),
        ..Selector::default()
    });
    assert_eq!(
        (
            discarded.predicted,
            discarded.correct,
            discarded.false_positive,
            discarded.unjudged,
            discarded.dismissed
        ),
        (2, 0, 0, 0, 2)
    );
    let content = score.total(&Selector::default());
    assert_eq!(
        (content.found, content.correct, content.false_positive),
        (1, 1, 0)
    );
    // The transmission rows: a dismissed transmission has no verdict.
    let discarded_row = score
        .transmissions
        .iter()
        .find(|row| row.key.quality == Quality::Discarded)
        .unwrap_or_else(|| panic!("a discarded transmission row"));
    assert_eq!(discarded_row.key.route, RouteKind::Channel);
    assert_eq!(
        (
            discarded_row.counts.genuine,
            discarded_row.counts.false_detection,
            discarded_row.counts.unlabeled
        ),
        (0, 0, 2)
    );
    // The dismissed reread fell under the reread control: recorded apart,
    // never charged.
    assert_eq!(score.access_only_under_controls.len(), 1);
    let row = &score.access_only_under_controls[0];
    assert_eq!(
        (row.class, row.reason, row.count),
        (EvidenceClass::Discarded, NegativeReason::Reread, 1)
    );
}

#[test]
fn the_reread_gate_passes_on_a_discarded_reread() {
    let scene = scene();
    let gates = Gates::parse(
        r#"
        detector = "test-detector"

        [[gate]]
        name = "no reread is reported as a transmission"
        dataset = "synthetic"
        metric = "violations"
        reason = "reread"
        max = 0
        "#,
        "gates.toml",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let score = score(&scene, &[confirmed(&scene), discarded_reread(&scene)]);
    let outcomes = gates.evaluate(&score);
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(
        matches!(outcomes[0].status, GateStatus::Pass { .. }),
        "{outcomes:?}"
    );
}

#[test]
fn a_confirmed_reread_is_still_a_violation() {
    let scene = scene();
    let reported = Prediction {
        transmission: tref(4),
        reader_exchange: scene.reread,
        read_at: scene.reread_at,
        ..confirmed(&scene)
    };
    let score = score(&scene, &[confirmed(&scene), reported]);
    assert_eq!(score.violation_count(None, Some(NegativeReason::Reread)), 1);
    assert_eq!(score.total(&Selector::default()).false_positive, 1);
}

#[test]
fn a_suspected_prediction_aligned_with_nothing_is_still_false() {
    let scene = scene();
    let suspected = Prediction {
        class: EvidenceClass::Suspected,
        quality: Quality::Suspected,
        ..discarded_stale_writer(&scene)
    };
    let score = score(&scene, &[suspected]);
    let row = score.total(&Selector {
        class: Some(EvidenceClass::Suspected),
        ..Selector::default()
    });
    assert_eq!((row.false_positive, row.dismissed), (1, 0));
}

#[test]
fn a_suspected_prediction_under_a_control_is_not_a_violation() {
    let scene = scene();
    let suspected = Prediction {
        class: EvidenceClass::Suspected,
        quality: Quality::Suspected,
        ..discarded_reread(&scene)
    };
    let score = score(&scene, &[suspected]);
    assert_eq!(score.violation_count(None, None), 0);
    assert!(score.sources.is_empty());
    let row = score.total(&Selector {
        class: Some(EvidenceClass::Suspected),
        ..Selector::default()
    });
    assert_eq!(row.false_positive, 1, "still false in its own row");
    assert_eq!(score.access_only_under_controls.len(), 1);
    assert_eq!(
        score.access_only_under_controls[0].class,
        EvidenceClass::Suspected
    );
}

#[test]
fn the_table_shows_dismissed_discarded_rows() {
    let scene = scene();
    let text = render(&report(
        &scene,
        &[confirmed(&scene), discarded_reread(&scene)],
    ));
    assert!(text.contains("dismissed"), "{text}");
    assert!(!text.contains("negative-control violations"), "{text}");
    assert!(text.contains("dismissed on reread controls: 1"), "{text}");
    let row = text
        .lines()
        .find(|line| line.contains(" discarded "))
        .unwrap_or_else(|| panic!("a discarded row: {text}"));
    let cells: Vec<&str> = row.split_whitespace().collect();
    // route carrier class tier expected found missed recall predicted
    // correct false unjudged dismissed precision
    assert_eq!(&cells[8..13], ["1", "0", "0", "0", "1"], "{row}");
}
