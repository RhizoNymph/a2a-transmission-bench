//! The alignment rule and the scorer's counts (ported from crosstalk-eval's
//! `tests/score.rs`; the `DetectionQuality` cross-check stays with the
//! crosstalk adapter, the transmission rows it checked are checked here).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::borrow::Cow;

use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{AgentKey, DetectorAgent, ExchangeId, TransmissionRef};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedTransmission, Label, MatchClass, NegativeReason, Route, RouteKind, Tier,
};
use a2a_bench_format::location::Location;
use a2a_bench_format::predictions::{
    Attribution, ContentEvidence, MatchKind, PredictedRoute, Prediction as Row, Quality, State,
    Transmission, TransmissionFields,
};
use a2a_bench_format::resource::Resource;
use a2a_bench_score::canon::Canonicalize;
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::{AgentMap, Prediction, exchange_agents, from_transmission};
use a2a_bench_score::score::align::aligns;
use a2a_bench_score::score::{Scorer, Selector};
use a2a_bench_score::{AsGiven, World};
use common::{Built, Draft, at, complete, content_fields, control, len, says, system, user, whole};

const DELIVERED: &str =
    "[round=1/5][from=bob][type=other]\n\nBob's findings: forty two rows match the filter.";
const CONTENT_START: u32 = 36;
const REJECTED: &str = "Rejected: a very long raw log that never reached Alice at all";

struct Scene {
    built: Built,
    alice: AgentKey,
    bob: AgentKey,
    /// Alice's exchange reading Bob's message.
    reads: ExchangeId,
    /// Alice's next exchange.
    later: ExchangeId,
    /// Where Bob's text sits in Alice's user turn.
    content: Location,
    /// Alice's system prompt, a shared-source trap.
    system: Location,
    /// Bob's rejected send, an origin a control names.
    rejected: Location,
}

fn file() -> Resource {
    Resource::File {
        host: None,
        path: "/shared/findings.md".into(),
    }
}

fn scene_with(coverage: Coverage, channel: bool, origin_control: bool) -> Scene {
    let mut draft = Draft::new("w");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    let sys = system("You are Alice. Shared policy text about verdicts.");
    let delivered = user(DELIVERED);
    let thanks = says("thanks");
    let reads = draft.exchange(&alice, 2, &[&sys, &delivered], &[&thanks]);
    let later = draft.exchange(
        &alice,
        3,
        &[&sys, &delivered, &thanks, &user("verdict now")],
        &[&says("accept")],
    );
    draft.exchange(
        &bob,
        1,
        &[&user("send your findings")],
        &[&says("Bob's findings")],
    );
    let rejected_message = says(REJECTED);
    let rejecting = draft.exchange(&bob, 4, &[&user("again")], &[&rejected_message]);
    let content = at(reads, &delivered, 0, CONTENT_START, len(DELIVERED));
    let system_at = whole(reads, &sys, 0);
    let rejected = whole(rejecting, &rejected_message, 0);
    let route = if channel {
        Route::Channel { resource: file() }
    } else {
        Route::Direct
    };
    draft.expect(Label::Transmission(
        ExpectedTransmission::new(content_fields(
            "t0",
            &bob,
            &alice,
            reads,
            route,
            CarrierKind::UserTurn,
            &DELIVERED[CONTENT_START as usize..],
            content,
            Tier::Construction,
        ))
        .unwrap(),
    ));
    draft.expect(control(
        "shared",
        &bob,
        &alice,
        None,
        Some(system_at),
        None,
        NegativeReason::SharedSource,
        Tier::Structural,
    ));
    if origin_control {
        draft.expect(control(
            "rejected",
            &bob,
            &alice,
            None,
            None,
            Some(rejected),
            NegativeReason::RejectedSend,
            Tier::Construction,
        ));
    }
    Scene {
        built: draft.finish(coverage),
        alice,
        bob,
        reads,
        later,
        content,
        system: system_at,
        rejected,
    }
}

fn scene(coverage: Coverage, channel: bool) -> Scene {
    scene_with(coverage, channel, false)
}

fn tref(id: u32) -> TransmissionRef {
    TransmissionRef::new(format!("t:{id}")).unwrap()
}

fn prediction(scene: &Scene, transmission: u32) -> Prediction {
    Prediction {
        transmission: tref(transmission),
        from: scene.bob.clone(),
        to: scene.alice.clone(),
        reader_exchange: scene.reads,
        route: PredictedRoute::Direct,
        carrier: CarrierKind::UserTurn,
        class: EvidenceClass::Exact,
        quality: Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::UserTurn,
        },
        read_at: scene.content,
        origin_at: None,
    }
}

fn sub_location(scene: &Scene, start: u32, end: u32) -> Location {
    Location {
        range: a2a_bench_format::location::ByteRange::new(start, end).unwrap(),
        ..scene.content
    }
}

fn positive(scene: &Scene) -> &ExpectedTransmission {
    scene
        .built
        .labels
        .iter()
        .find_map(|label| match label {
            Label::Transmission(expected) => Some(expected),
            _ => None,
        })
        .unwrap()
}

fn score(world: &World<'_>, predictions: &[Prediction]) -> a2a_bench_score::score::Score {
    let mut scorer = Scorer::new(10);
    scorer.add_world(world, predictions);
    scorer.finish()
}

#[test]
fn a_matching_prediction_aligns() {
    let scene = scene(complete(), false);
    assert!(aligns(&prediction(&scene, 0), positive(&scene), &AsGiven));
}

#[test]
fn partial_overlap_is_enough() {
    let scene = scene(complete(), false);
    let mut p = prediction(&scene, 0);
    p.read_at = sub_location(&scene, 50, 60);
    assert!(aligns(&p, positive(&scene), &AsGiven));
    // The header before the content is not the content.
    p.read_at = sub_location(&scene, 0, CONTENT_START);
    assert!(!aligns(&p, positive(&scene), &AsGiven));
}

#[test]
fn another_part_or_message_does_not_overlap() {
    let scene = scene(complete(), false);
    let mut p = prediction(&scene, 0);
    p.read_at.part = 1;
    assert!(!aligns(&p, positive(&scene), &AsGiven));
    let mut q = prediction(&scene, 0);
    q.read_at.message = scene.system.message;
    assert!(!aligns(&q, positive(&scene), &AsGiven));
}

#[test]
fn sender_reader_and_exchange_must_agree() {
    let scene = scene(complete(), false);
    let expected = positive(&scene);
    let mut wrong_sender = prediction(&scene, 0);
    wrong_sender.from = scene.alice.clone();
    wrong_sender.to = scene.bob.clone();
    assert!(!aligns(&wrong_sender, expected, &AsGiven));
    let mut later = prediction(&scene, 0);
    later.reader_exchange = scene.later;
    assert!(!aligns(&later, expected, &AsGiven));
}

#[test]
fn class_and_carrier_do_not_decide_alignment() {
    let scene = scene(complete(), false);
    let mut p = prediction(&scene, 0);
    p.class = EvidenceClass::Semantic;
    p.carrier = CarrierKind::ToolResult;
    p.route = PredictedRoute::Unobserved;
    assert!(aligns(&p, positive(&scene), &AsGiven));
}

#[test]
fn channel_labels_need_the_same_resource() {
    let scene = scene(complete(), true);
    let expected = positive(&scene);
    let mut p = prediction(&scene, 0);
    assert!(
        !aligns(&p, expected, &AsGiven),
        "a direct route is not the channel"
    );
    p.route = PredictedRoute::Channel {
        resources: vec![Resource::File {
            host: None,
            path: "/other.md".into(),
        }],
    };
    assert!(!aligns(&p, expected, &AsGiven));
    p.route = PredictedRoute::Channel {
        resources: vec![file()],
    };
    assert!(aligns(&p, expected, &AsGiven));
}

#[test]
fn a_channel_label_aligns_with_any_of_the_predicted_resources() {
    let scene = scene(complete(), true);
    let expected = positive(&scene);
    let other = Resource::File {
        host: None,
        path: "/other.md".into(),
    };
    let mut p = prediction(&scene, 0);
    p.route = PredictedRoute::Channel {
        resources: vec![other.clone(), file()],
    };
    assert!(
        aligns(&p, expected, &AsGiven),
        "the second resource is the label's"
    );
    p.route = PredictedRoute::Channel {
        resources: vec![Resource::Url("https://example.com/other".into()), other],
    };
    assert!(
        !aligns(&p, expected, &AsGiven),
        "no resource is the label's"
    );
    p.route = PredictedRoute::Channel {
        resources: Vec::new(),
    };
    assert!(
        !aligns(&p, expected, &AsGiven),
        "an empty channel names nothing"
    );
    p.route = PredictedRoute::Channel {
        resources: vec![
            Resource::File {
                host: None,
                path: "/SHARED/Findings.md".into(),
            },
            Resource::Url("https://example.com/other".into()),
        ],
    };
    assert!(!aligns(&p, expected, &AsGiven));
    assert!(
        aligns(&p, expected, &FoldCase),
        "each resource is canonicalised"
    );
}

/// Folds a file path's case: a stand-in for the resource canonicaliser.
struct FoldCase;

impl Canonicalize for FoldCase {
    fn canonical<'r>(&self, resource: &'r Resource) -> Cow<'r, Resource> {
        match resource {
            Resource::File { host, path } => Cow::Owned(Resource::File {
                host: host.clone(),
                path: path.to_lowercase(),
            }),
            other => Cow::Borrowed(other),
        }
    }
}

#[test]
fn resources_are_compared_after_the_canonicaliser() {
    let scene = scene(complete(), true);
    let expected = positive(&scene);
    let mut p = prediction(&scene, 0);
    p.route = PredictedRoute::Channel {
        resources: vec![Resource::File {
            host: None,
            path: "/SHARED/Findings.md".into(),
        }],
    };
    assert!(!aligns(&p, expected, &AsGiven), "as given, they differ");
    assert!(aligns(&p, expected, &FoldCase));
    let mut scorer = Scorer::with_canonicalizer(10, Box::new(FoldCase));
    scorer.add_world(&scene.built.world(), &[p]);
    let all = scorer.finish().total(&Selector::default());
    assert_eq!((all.found, all.correct), (1, 1));
}

#[test]
fn scorer_counts_found_missed_correct_and_false() {
    let scene = scene(complete(), false);
    let mut later = prediction(&scene, 2);
    later.reader_exchange = scene.later;
    let predictions = vec![prediction(&scene, 1), prediction(&scene, 1), later];
    let score = score(&scene.built.world(), &predictions);
    let all = score.total(&Selector::default());
    assert_eq!(all.expected, 1);
    assert_eq!(all.found, 1);
    assert_eq!(all.missed, 0);
    assert_eq!(all.predicted, 3);
    assert_eq!(
        all.correct, 2,
        "duplicates of one correct match are each correct"
    );
    assert_eq!(all.false_positive, 1);
    assert_eq!(all.precision(), Some(2.0 / 3.0));
    assert_eq!(all.recall(), Some(1.0));
    assert_eq!(score.false_positives.len(), 1);
    assert_eq!(score.totals.negative_controls, 1);
}

#[test]
fn a_label_with_no_prediction_is_missed() {
    let scene = scene(complete(), false);
    let score = score(&scene.built.world(), &[]);
    let all = score.total(&Selector::default());
    assert_eq!((all.expected, all.found, all.missed), (1, 0, 1));
    assert_eq!(all.recall(), Some(0.0));
    assert_eq!(all.precision(), None);
    assert_eq!(score.misses.len(), 1);
}

#[test]
fn rows_break_down_by_route_carrier_class_and_tier() {
    let scene = scene(complete(), false);
    let mut normalized = prediction(&scene, 1);
    normalized.class = EvidenceClass::Normalized;
    let score = score(&scene.built.world(), &[normalized]);
    let label_row = Selector {
        dataset: Some(common::dataset()),
        route: Some(RouteKind::Direct),
        carrier: Some(CarrierKind::UserTurn),
        class: Some(EvidenceClass::Exact),
        tier: Some(Tier::Construction),
    };
    let found = score.total(&label_row);
    assert_eq!((found.expected, found.found, found.predicted), (1, 1, 0));
    let prediction_row = Selector {
        class: Some(EvidenceClass::Normalized),
        ..label_row
    };
    let predicted = score.total(&prediction_row);
    assert_eq!((predicted.expected, predicted.correct), (0, 1));
}

#[test]
fn negative_controls_are_charged_as_violations() {
    let scene = scene(complete(), false);
    let mut trap = prediction(&scene, 9);
    trap.read_at = scene.system;
    trap.carrier = CarrierKind::SystemPrompt;
    let score = score(&scene.built.world(), &[trap]);
    assert_eq!(
        score.violation_count(None, Some(NegativeReason::SharedSource)),
        1
    );
    assert_eq!(score.total(&Selector::default()).false_positive, 1);
    let shared = score.total(&Selector {
        tier: Some(Tier::Structural),
        ..Selector::default()
    });
    assert_eq!(shared.false_positive, 1, "charged at the control's tier");
    assert_eq!(score.sources.len(), 1);
    assert_eq!(score.sources[0].count, 1);
    assert_eq!(
        score.sources[0].text,
        "You are Alice. Shared policy text about verdicts."
    );
}

#[test]
fn unlabelled_predictions_under_partial_coverage_are_unjudged() {
    let scene = scene(Coverage::Partial, false);
    let mut stray = prediction(&scene, 3);
    stray.reader_exchange = scene.later;
    let mut trap = prediction(&scene, 4);
    trap.read_at = scene.system;
    let score = score(&scene.built.world(), &[stray, trap]);
    let all = score.total(&Selector::default());
    assert_eq!(all.unjudged, 1);
    assert_eq!(
        all.false_positive, 1,
        "a violated control is false under any coverage"
    );
    let unjudged = score.rows.iter().find(|row| row.counts.unjudged > 0);
    assert_eq!(unjudged.map(|row| row.key.tier), Some(None));
}

fn d(name: &str) -> DetectorAgent {
    DetectorAgent::new(name).unwrap()
}

/// A confirmed transmission from detector agent `d:bob` to `d:alice` with
/// one match per kind, all at `read_at`.
fn transmission(scene: &Scene, id: u32, read_at: Location, kinds: &[MatchKind]) -> Transmission {
    let matches: Vec<ContentEvidence> = kinds
        .iter()
        .map(|kind| ContentEvidence {
            from: d("d:bob"),
            to: d("d:alice"),
            reader_exchange: scene.reads,
            read_at,
            origin_at: None,
            kind: kind.clone(),
            carrier: CarrierKind::UserTurn,
            route: PredictedRoute::Direct,
        })
        .collect();
    let strongest = kinds.iter().map(MatchKind::class).min().unwrap();
    Transmission::new(TransmissionFields {
        id: tref(id),
        state: State::Confirmed,
        quality: Some(Quality::Content {
            class: strongest,
            carrier: CarrierKind::UserTurn,
        }),
        matches,
        co_access: Vec::new(),
    })
    .unwrap()
}

/// The detector's attribution: its agents hold exactly the scene's agents'
/// exchanges.
fn agents(scene: &Scene) -> AgentMap {
    let owners = exchange_agents(&scene.built.labels);
    let held = |agent: &AgentKey| {
        owners
            .iter()
            .filter(|(_, owner)| *owner == agent)
            .map(|(exchange, _)| *exchange)
            .collect::<Vec<_>>()
    };
    let rows = vec![
        Row::Attribution(Attribution {
            agent: d("d:alice"),
            exchanges: held(&scene.alice),
        }),
        Row::Attribution(Attribution {
            agent: d("d:bob"),
            exchanges: held(&scene.bob),
        }),
    ];
    AgentMap::from_rows(&owners, &rows).unwrap()
}

#[test]
fn transmissions_become_one_prediction_per_match() {
    let scene = scene(complete(), false);
    let transmission = transmission(
        &scene,
        7,
        scene.content,
        &[MatchKind::Exact, MatchKind::Normalized],
    );
    let predictions = from_transmission(&transmission, &agents(&scene)).unwrap();
    assert_eq!(predictions.len(), 2);
    assert!(
        predictions
            .iter()
            .all(|p| p.from == scene.bob && p.to == scene.alice)
    );
    assert_eq!(predictions[1].class, EvidenceClass::Normalized);
    assert!(
        predictions
            .iter()
            .all(|p| aligns(p, positive(&scene), &AsGiven))
    );
    assert!(predictions.iter().all(|p| p.quality
        == Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::UserTurn
        }));
}

#[test]
fn transmission_rows_are_keyed_by_quality_with_the_truths_verdicts() {
    let scene = scene(complete(), false);
    let transmissions = [
        transmission(
            &scene,
            1,
            scene.content,
            &[MatchKind::Normalized, MatchKind::Exact],
        ),
        transmission(&scene, 2, scene.system, &[MatchKind::Exact]),
        transmission(
            &scene,
            3,
            sub_location(&scene, 0, 5),
            &[MatchKind::Normalized],
        ),
    ];
    let map = agents(&scene);
    let predictions: Vec<Prediction> = transmissions
        .iter()
        .flat_map(|t| from_transmission(t, &map).unwrap())
        .collect();
    let score = score(&scene.built.world(), &predictions);
    let rows: Vec<(RouteKind, Quality, u64, u64, u64)> = score
        .transmissions
        .iter()
        .map(|row| {
            (
                row.key.route,
                row.key.quality,
                row.counts.genuine,
                row.counts.false_detection,
                row.counts.unlabeled,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                RouteKind::Direct,
                Quality::Content {
                    class: MatchClass::Exact,
                    carrier: CarrierKind::UserTurn
                },
                1,
                1,
                0
            ),
            (
                RouteKind::Direct,
                Quality::Content {
                    class: MatchClass::Normalized,
                    carrier: CarrierKind::UserTurn
                },
                0,
                1,
                0
            ),
        ]
    );
}

#[test]
fn origin_bounded_controls_need_the_matched_span() {
    let scene = scene_with(complete(), false, true);
    let mut stray = prediction(&scene, 5);
    stray.reader_exchange = scene.later;
    let mut unknown_origin = stray.clone();
    unknown_origin.transmission = tref(6);
    stray.origin_at = Some(scene.rejected);
    let score = score(&scene.built.world(), &[stray, unknown_origin]);
    assert_eq!(
        score.violation_count(None, Some(NegativeReason::RejectedSend)),
        1
    );
    assert_eq!(score.total(&Selector::default()).false_positive, 2);
}

#[test]
fn the_most_specific_control_is_charged() {
    let mut scene = scene(complete(), false);
    // An exchange-bounded control over the same read as the location-bounded
    // shared-source control: the location wins.
    scene.built.labels.push(control(
        "boiler",
        &scene.bob,
        &scene.alice,
        Some(scene.reads),
        None,
        None,
        NegativeReason::Boilerplate,
        Tier::Heuristic,
    ));
    let mut trap = prediction(&scene, 9);
    trap.read_at = scene.system;
    let score = score(&scene.built.world(), &[trap]);
    assert_eq!(
        score.violation_count(None, Some(NegativeReason::SharedSource)),
        1
    );
    assert_eq!(
        score.violation_count(None, Some(NegativeReason::Boilerplate)),
        0
    );
}

#[test]
fn totals_count_the_world() {
    let scene = scene(complete(), false);
    let score = score(&scene.built.world(), &[prediction(&scene, 1)]);
    let t = score.totals;
    assert_eq!(
        (
            t.worlds,
            t.agents,
            t.exchanges,
            t.expectations,
            t.negative_controls,
            t.predictions
        ),
        (1, 2, 4, 1, 1, 1)
    );
}
