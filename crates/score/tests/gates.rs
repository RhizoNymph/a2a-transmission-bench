//! Gates: parsing, selection by detector name and variant, evaluation, and
//! the shipped files (ported from crosstalk-eval's `tests/gates_detector.rs`
//! and the gate parts of `tests/forwarding.rs`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use a2a_bench_format::ids::{DatasetId, TransmissionRef};
use a2a_bench_format::labels::{CarrierKind, MatchClass, NegativeReason, Tier};
use a2a_bench_format::predictions::{PredictedRoute, Quality};
use a2a_bench_score::class::EvidenceClass;
use a2a_bench_score::predict::Prediction;
use a2a_bench_score::report::gates::{Check, GateError, GateStatus, Gates};
use a2a_bench_score::report::{Disclosure, GATE_FAILURE_EXIT, Report};
use a2a_bench_score::run::RunSummary;
use a2a_bench_score::score::{Score, Scorer};
use common::{Draft, complete, control, says, user, whole};

const GATES: &str = r#"
detector = "crosstalk-live"

[[gate]]
name = "live, shipped"
variant = "forwarding-off"
metric = "recall"
min = 0.5

[[gate]]
name = "live, forwarding on"
variant = "forwarding-on"
tier = "forwarding"
metric = "recall"
min = 0.9

[[gate]]
name = "live, any variant"
metric = "precision"
min = 0.5
"#;

const REFERENCE: &str = r#"
detector = "reference"

[[gate]]
name = "reference"
metric = "recall"
min = 0.5
"#;

fn names(gates: &Gates) -> Vec<&str> {
    gates.gates.iter().map(|gate| gate.name.as_str()).collect()
}

fn both() -> Gates {
    let mut gates = Gates::parse(GATES, "live.toml").unwrap();
    gates.extend(Gates::parse(REFERENCE, "reference.toml").unwrap());
    gates
}

#[test]
fn gates_select_by_detector_name_and_variant() {
    let gates = both();
    assert_eq!(
        names(&gates.for_run("crosstalk-live", "forwarding-off")),
        ["live, shipped", "live, any variant"]
    );
    assert_eq!(
        names(&gates.for_run("crosstalk-live", "forwarding-on")),
        ["live, forwarding on", "live, any variant"]
    );
    assert_eq!(names(&gates.for_run("reference", "default")), ["reference"]);
    assert!(gates.for_run("oracle", "default").gates.is_empty());
    assert_eq!(gates.gates[0].detector, "crosstalk-live");
}

#[test]
fn a_file_names_its_detector() {
    let text = "[[gate]]\nname = \"x\"\nmetric = \"recall\"\nmin = 0.5\n";
    assert!(matches!(
        Gates::parse(text, "fixture"),
        Err(GateError::Parse { .. })
    ));
}

#[test]
fn unknown_keys_are_refused() {
    for gate in [
        "forwarding = \"on\"\nmetric = \"recall\"\nmin = 0.5",
        "detector = \"live\"\nmetric = \"recall\"\nmin = 0.5",
        "metric = \"recall\"\nmin = 0.5\nmax = 0.9",
        "metric = \"recall\"",
        "metric = \"violations\"\nmax = 0.5",
        "metric = \"precision\"\nmin = 0.5\nreason = \"reread\"",
        "metric = \"f1\"\nmin = 0.5",
        "class = \"probable\"\nmetric = \"recall\"\nmin = 0.5",
    ] {
        let text = format!("detector = \"d\"\n\n[[gate]]\nname = \"x\"\n{gate}\n");
        assert!(Gates::parse(&text, "fixture").is_err(), "{gate}");
    }
}

#[test]
fn metrics_take_their_bounds() {
    let text = r#"
detector = "d"

[[gate]]
name = "recall"
metric = "recall"
min = 1

[[gate]]
name = "violations"
metric = "violations"
reason = "reread"
max = 0

[[gate]]
name = "fp"
metric = "fp_per_1k"
max = 130.0
"#;
    let gates = Gates::parse(text, "fixture").unwrap();
    let checks: Vec<&Check> = gates.gates.iter().map(|gate| &gate.check).collect();
    assert_eq!(
        checks,
        [
            &Check::Recall { min: 1.0 },
            &Check::Violations {
                max: 0,
                reason: Some(NegativeReason::Reread)
            },
            &Check::FpPer1k { max: 130.0 },
        ]
    );
}

/// A world with no label, three exchanges and two false positives: one
/// under a reread control, one unlabelled under complete coverage.
fn score() -> Score {
    let mut draft = Draft::new("w");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    draft.exchange(&bob, 1, &[&user("go")], &[&says("the plan")]);
    let read = user("the plan");
    let reads = draft.exchange(&alice, 2, &[&read], &[&says("ok")]);
    let again = user("the plan, again");
    let reread = draft.exchange(&alice, 3, &[&read, &says("ok"), &again], &[&says("ok")]);
    draft.expect(control(
        "reread",
        &bob,
        &alice,
        Some(reread),
        None,
        None,
        NegativeReason::Reread,
        Tier::Construction,
    ));
    let built = draft.finish(complete());
    let prediction = |id: &str, reader, read_at| Prediction {
        transmission: TransmissionRef::new(id).unwrap(),
        from: bob.clone(),
        to: alice.clone(),
        reader_exchange: reader,
        route: PredictedRoute::Direct,
        carrier: CarrierKind::UserTurn,
        class: EvidenceClass::Exact,
        quality: Quality::Content {
            class: MatchClass::Exact,
            carrier: CarrierKind::UserTurn,
        },
        read_at,
        origin_at: None,
    };
    let mut scorer = Scorer::new(0);
    scorer.add_world(
        &built.world(),
        &[
            prediction("t:1", reads, whole(reads, &read, 0)),
            prediction("t:2", reread, whole(reread, &again, 0)),
        ],
    );
    scorer.finish()
}

fn status(text: &str, score: &Score) -> GateStatus {
    let gate = format!("detector = \"d\"\n\n[[gate]]\nname = \"g\"\n{text}\n");
    let outcomes = Gates::parse(&gate, "fixture").unwrap().evaluate(score);
    assert_eq!(outcomes.len(), 1);
    outcomes[0].status.clone()
}

#[test]
fn gates_evaluate_their_metric() {
    let score = score();
    // No label: recall has no data and is skipped, never failed.
    assert_eq!(
        status("metric = \"recall\"\nmin = 0.9", &score),
        GateStatus::Skipped
    );
    assert_eq!(
        status("metric = \"precision\"\nmin = 0.9", &score),
        GateStatus::Fail {
            value: 0.0,
            bound: 0.9
        }
    );
    assert_eq!(
        status("metric = \"precision\"\nmin = 0.0", &score),
        GateStatus::Pass { value: 0.0 }
    );
    assert_eq!(
        status(
            "dataset = \"synthetic\"\nmetric = \"violations\"\nreason = \"reread\"\nmax = 0",
            &score
        ),
        GateStatus::Fail {
            value: 1.0,
            bound: 0.0
        }
    );
    assert_eq!(
        status(
            "metric = \"violations\"\nreason = \"boilerplate\"\nmax = 0",
            &score
        ),
        GateStatus::Pass { value: 0.0 }
    );
    // Two false positives over three exchanges.
    assert_eq!(
        status("metric = \"fp_per_1k\"\nmax = 700.0", &score),
        GateStatus::Pass {
            value: 2.0 * 1000.0 / 3.0
        }
    );
    assert!(matches!(
        status("metric = \"fp_per_1k\"\nmax = 600", &score),
        GateStatus::Fail { .. }
    ));
}

#[test]
fn a_gate_on_a_dataset_the_run_did_not_score_is_other_dataset() {
    let score = score();
    assert_eq!(
        status(
            "dataset = \"salt\"\nmetric = \"violations\"\nmax = 0",
            &score
        ),
        GateStatus::OtherDataset
    );
    assert!(score.scored(&DatasetId::new("synthetic").unwrap()));
}

#[test]
fn fp_per_1k_is_skipped_without_exchanges() {
    let score = Scorer::new(0).finish();
    assert_eq!(
        status("metric = \"fp_per_1k\"\nmax = 1.0", &score),
        GateStatus::Skipped
    );
}

#[test]
fn a_failed_gate_exits_two() {
    let score = score();
    let gates = Gates::parse(
        "detector = \"test-detector\"\n\n[[gate]]\nname = \"g\"\nmetric = \"precision\"\nmin = 0.9\n",
        "fixture",
    )
    .unwrap();
    let outcomes = gates.evaluate(&score);
    let summary = RunSummary::new(common::dataset(), common::detector(), score);
    let report = Report::new(summary, outcomes, Disclosure::Full);
    assert!(report.gates_failed());
    assert_eq!(report.exit_code(), GATE_FAILURE_EXIT);
    assert_eq!(GATE_FAILURE_EXIT, 2);
    let passing = Report::new(
        RunSummary::new(
            common::dataset(),
            common::detector(),
            Scorer::new(0).finish(),
        ),
        Vec::new(),
        Disclosure::Full,
    );
    assert_eq!(passing.exit_code(), 0);
}

fn shipped() -> Gates {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gates");
    Gates::load(&dir).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn the_shipped_gates_parse_and_gate_every_detector() {
    let gates = shipped();
    assert_eq!(gates.gates.len(), 37);
    assert_eq!(gates.for_run("reference", "default").gates.len(), 9);
    assert_eq!(
        gates
            .for_run("crosstalk-live", "forwarding-off")
            .gates
            .len(),
        19
    );
    assert_eq!(
        gates
            .for_run("crosstalk-gateway-export", "default")
            .gates
            .len(),
        8
    );
}

#[test]
fn the_shipped_gates_have_a_forwarding_on_gate() {
    let on = shipped().for_run("crosstalk-live", "forwarding-on");
    assert!(!on.gates.is_empty());
    assert!(
        on.gates
            .iter()
            .all(|gate| gate.tier == Some(Tier::Forwarding))
    );
}

#[test]
/// Shipped thresholds load as written: the SALT one is still ct-eval's
/// (7f8a2fb); the demo-swarm boilerplate FP bound is the bench's own since
/// crosstalk PR #114 tightened it (130 -> 115).
fn a_shipped_threshold_loads_as_written() {
    let gates = shipped();
    let gate = gates
        .gates
        .iter()
        .find(|gate| {
            gate.name == "demo-swarm/boilerplate: false positives per 1k exchanges stay bounded"
        })
        .unwrap();
    assert_eq!(gate.check, Check::FpPer1k { max: 115.0 });
    assert_eq!(gate.detector, "crosstalk-gateway-export");
    let gate = gates
        .gates
        .iter()
        .find(|gate| gate.name == "salt (live): delivered messages that arrive verbatim are found")
        .unwrap();
    assert_eq!(gate.check, Check::Recall { min: 0.82 });
    assert_eq!(gate.variant.as_deref(), Some("forwarding-off"));
    assert_eq!(gate.class, Some(EvidenceClass::Exact));
}
