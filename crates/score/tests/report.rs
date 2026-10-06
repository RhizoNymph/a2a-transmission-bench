//! The report: ct-eval's fields, the background rate, and holdout mode,
//! which leaves out every per-label, per-world and example output.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use a2a_bench_format::ids::{DetectorAgent, TransmissionRef, WorldKey};
use a2a_bench_format::labels::{
    CarrierKind, ExpectedTransmission, Label, MatchClass, NegativeReason, Route, Tier,
};
use a2a_bench_format::predictions::Quality;
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::Prediction;
use a2a_bench_score::report::gates::Gates;
use a2a_bench_score::report::table::render;
use a2a_bench_score::report::{Disclosure, Report};
use a2a_bench_score::run::{FailedWorld, RunSummary, UnknownDetected, WorldFailure};
use a2a_bench_score::score::Scorer;
use common::{Draft, complete, content_fields, control, says, system, user, whole};

const NOTE: &str = "the deploy window moved to thursday";
const BANNER: &str = "=== test session starts ===\nplatform linux";

/// One missed label, one false positive under a boilerplate control, one
/// failed world and one unknown detected agent.
fn summary() -> RunSummary {
    let mut draft = Draft::new("w1");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    draft.exchange(&alice, 1, &[&user("tell bob")], &[&says(NOTE)]);
    let delivered = user(NOTE);
    let banner = system(BANNER);
    let b1 = draft.exchange(&bob, 2, &[&banner, &delivered], &[&says("ok")]);
    draft.expect(Label::Transmission(
        ExpectedTransmission::new(content_fields(
            "note",
            &alice,
            &bob,
            b1,
            Route::Direct,
            CarrierKind::UserTurn,
            NOTE,
            whole(b1, &delivered, 0),
            Tier::Construction,
        ))
        .unwrap(),
    ));
    let banner_at = whole(b1, &banner, 0);
    draft.expect(control(
        "banner",
        &alice,
        &bob,
        None,
        Some(banner_at),
        None,
        NegativeReason::Boilerplate,
        Tier::Heuristic,
    ));
    let built = draft.finish(complete());
    let trap = Prediction {
        transmission: TransmissionRef::new("t:1").unwrap(),
        from: alice,
        to: bob,
        reader_exchange: b1,
        route: Route::Direct,
        carrier: CarrierKind::SystemPrompt,
        class: EvidenceClass::Exact,
        quality: Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::SystemPrompt,
        },
        read_at: banner_at,
        origin_at: None,
    };
    let mut scorer = Scorer::new(10);
    scorer.add_world(&built.world(), &[trap]);
    let mut summary = RunSummary::new(common::dataset(), common::detector(), scorer.finish());
    summary.failures.push(FailedWorld {
        world: WorldKey::new("w2").unwrap(),
        failure: WorldFailure::Detector {
            reason: "timeout".into(),
        },
    });
    summary.unknown_detected_agents.push(UnknownDetected {
        world: WorldKey::new("w1").unwrap(),
        transmission: TransmissionRef::new("t:9").unwrap(),
        agent: DetectorAgent::new("d:ghost").unwrap(),
    });
    summary
}

fn gates() -> Gates {
    Gates::parse(
        "detector = \"test-detector\"\n\n[[gate]]\nname = \"recall\"\nmetric = \"recall\"\nmin = 0.5\n",
        "fixture",
    )
    .unwrap()
}

fn report(disclosure: Disclosure) -> Report {
    let summary = summary();
    let outcomes = gates().evaluate(&summary.score);
    Report::new(summary, outcomes, disclosure)
}

fn json(report: &Report) -> serde_json::Value {
    serde_json::from_str(&report.to_json().unwrap()).unwrap()
}

#[test]
fn the_full_report_keeps_ct_evals_fields() {
    let report = report(Disclosure::Full);
    let value = json(&report);
    for field in [
        "dataset",
        "detector",
        "totals",
        "overall",
        "out_of_reach",
        "forwarding",
        "access_only",
        "rows",
        "transmissions",
        "violations",
        "access_only_under_controls",
        "gates",
        "failures",
        "unscored",
        "misses",
        "false_positives",
        "background",
    ] {
        assert!(value.get(field).is_some(), "{field}: {value:#}");
    }
    let row = &value["rows"][0];
    for field in [
        "dataset",
        "route",
        "carrier",
        "class",
        "tier",
        "expected",
        "found",
        "missed",
        "suspected",
        "predicted",
        "correct",
        "false_positive",
        "unjudged",
        "dismissed",
        "precision",
        "recall",
    ] {
        assert!(row.get(field).is_some(), "{field}: {row:#}");
    }
    assert_eq!(value["failed_worlds"], 1);
    assert_eq!(value["unknown_detected_agent"], 1);
    assert_eq!(value["misses"].as_array().map(Vec::len), Some(1));
    assert_eq!(value["false_positives"].as_array().map(Vec::len), Some(1));
    assert_eq!(value["background"]["false_positives"], 1);
    assert_eq!(value["background"]["exchanges"], 2);
    assert_eq!(value["background"]["per_1k_exchanges"], 500.0);
    assert_eq!(
        value["background"]["sources"][0]["text"],
        "=== test session starts ==="
    );
    assert_eq!(value["gates"][0]["status"], "fail");
    let text = render(&report);
    assert!(text.contains("1 worlds failed:"), "{text}");
    assert!(text.contains("w2"), "{text}");
    assert!(text.contains("top boilerplate sources:"), "{text}");
    assert!(text.contains("unknown detected agent"), "{text}");
}

#[test]
fn a_holdout_report_is_aggregate_only() {
    let report = report(Disclosure::Holdout);
    let value = json(&report);
    for field in [
        "misses",
        "false_positives",
        "failures",
        "unknown_detected_agents",
    ] {
        assert!(value.get(field).is_none(), "{field}: {value:#}");
    }
    assert!(value["background"].get("sources").is_none());
    for field in ["overall", "rows", "violations", "gates", "transmissions"] {
        assert!(value.get(field).is_some(), "{field}");
    }
    assert_eq!(value["failed_worlds"], 1);
    assert_eq!(value["unknown_detected_agent"], 1);
    assert_eq!(value["background"]["per_1k_exchanges"], 500.0);
    let text = render(&report);
    assert!(!text.contains("w2"), "{text}");
    assert!(!text.contains("test session"), "{text}");
    assert!(text.contains("1 worlds failed"), "{text}");
    assert!(!report.to_json().unwrap().contains(NOTE));
}

#[test]
fn no_background_without_negative_controls() {
    let summary = RunSummary::new(
        common::dataset(),
        common::detector(),
        Scorer::new(0).finish(),
    );
    let report = Report::new(summary, Vec::new(), Disclosure::Full);
    assert!(report.background.is_none());
}
