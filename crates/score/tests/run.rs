//! The library entry point: an export's files and a predictions file
//! streamed world by world in lockstep, checked, scored and reported.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::io::Cursor;

use a2a_bench_format::files::Coverage;
use a2a_bench_format::ids::{DetectorAgent, ExchangeId, TransmissionRef};
use a2a_bench_format::labels::{CarrierKind, ExpectedTransmission, Label, MatchClass, Route, Tier};
use a2a_bench_format::location::Location;
use a2a_bench_format::manifest::{Converter, FileDigests, Manifest, Source, Split, WorldEntry};
use a2a_bench_format::predictions::{
    Attribution, ContentEvidence, MatchKind, Prediction as Row, Quality, State, Transmission,
    TransmissionFields, Unattributed, WorldStatus,
};
use a2a_bench_format::version::FORMAT;
use a2a_bench_score::report::{Disclosure, Report};
use a2a_bench_score::run::{
    FileName, RunError, ScoreOptions, Streams, WorldFailure, score_export, score_streams,
};
use a2a_bench_score::score::Selector;
use common::{Built, Draft, Files, complete, content_fields, files, says, user, whole};

const NOTE: &str = "the deploy window moved to thursday";

struct Scene {
    built: Built,
    a1: ExchangeId,
    b1: ExchangeId,
    read_at: Location,
}

fn scene(world: &str) -> Scene {
    let mut draft = Draft::new(world);
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    let a1 = draft.exchange(&alice, 1, &[&user("tell bob")], &[&says(NOTE)]);
    let delivered = user(NOTE);
    let b1 = draft.exchange(&bob, 2, &[&delivered], &[&says("ok")]);
    let read_at = whole(b1, &delivered, 0);
    draft.expect(Label::Transmission(
        ExpectedTransmission::new(content_fields(
            "note",
            &alice,
            &bob,
            b1,
            Route::Direct,
            CarrierKind::UserTurn,
            NOTE,
            read_at,
            Tier::Construction,
        ))
        .unwrap(),
    ));
    Scene {
        built: draft.finish(complete()),
        a1,
        b1,
        read_at,
    }
}

fn d(name: &str) -> DetectorAgent {
    DetectorAgent::new(name).unwrap()
}

fn found(scene: &Scene, from: &str) -> Row {
    Row::Transmission(
        Transmission::new(TransmissionFields {
            id: TransmissionRef::new(format!("t:{from}")).unwrap(),
            state: State::Confirmed,
            quality: Some(Quality::Content {
                class: MatchClass::Exact,
                carrier: CarrierKind::UserTurn,
            }),
            matches: vec![ContentEvidence {
                from: d(from),
                to: d("d:bob"),
                reader_exchange: scene.b1,
                read_at: scene.read_at,
                origin_at: None,
                kind: MatchKind::Exact,
                carrier: CarrierKind::UserTurn,
                route: Route::Direct,
            }],
            co_access: Vec::new(),
        })
        .unwrap(),
    )
}

fn attributed(scene: &Scene) -> Vec<Row> {
    vec![
        Row::Attribution(Attribution {
            agent: d("d:alice"),
            exchanges: vec![scene.a1],
        }),
        Row::Attribution(Attribution {
            agent: d("d:bob"),
            exchanges: vec![scene.b1],
        }),
        found(scene, "d:alice"),
    ]
}

type Bytes<'a> = Cursor<&'a [u8]>;

fn streams(files: &Files) -> Streams<Bytes<'_>, Bytes<'_>, Bytes<'_>, Bytes<'_>> {
    Streams {
        messages: Cursor::new(files.messages.as_slice()),
        exchanges: Cursor::new(files.exchanges.as_slice()),
        labels: Cursor::new(files.labels.as_slice()),
        predictions: Cursor::new(files.predictions.as_slice()),
    }
}

fn run(files: &Files) -> Result<a2a_bench_score::run::RunSummary, RunError> {
    score_streams(streams(files), ScoreOptions::default())
}

#[test]
fn scored_unscored_and_failed_worlds_are_counted_apart() {
    let (one, two, three) = (scene("w1"), scene("w2"), scene("w3"));
    let files = files(&[
        (&one.built, WorldStatus::Scored, attributed(&one)),
        (
            &two.built,
            WorldStatus::NoConsumers { ingested: 2 },
            Vec::new(),
        ),
        (
            &three.built,
            WorldStatus::Failed {
                reason: "backend crashed".into(),
            },
            Vec::new(),
        ),
    ]);
    let summary = run(&files).unwrap();
    assert_eq!(summary.dataset, common::dataset());
    assert_eq!(summary.detector, common::detector());
    assert_eq!(summary.score.totals.worlds, 1);
    let all = summary.score.total(&Selector::default());
    assert_eq!((all.expected, all.found, all.correct), (1, 1, 1));
    assert_eq!((summary.unscored.worlds, summary.unscored.ingested), (1, 2));
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(summary.failures[0].world.as_str(), "w3");
    assert_eq!(
        summary.failures[0].failure,
        WorldFailure::Detector {
            reason: "backend crashed".into()
        }
    );
}

#[test]
fn a_merge_fails_its_world_and_the_run_goes_on() {
    let (one, two) = (scene("w1"), scene("w2"));
    let merged = vec![
        Row::Attribution(Attribution {
            agent: d("d:bob"),
            exchanges: vec![one.a1, one.b1],
        }),
        found(&one, "d:bob"),
    ];
    let files = files(&[
        (&one.built, WorldStatus::Scored, merged),
        (&two.built, WorldStatus::Scored, attributed(&two)),
    ]);
    let summary = run(&files).unwrap();
    assert_eq!(summary.score.totals.worlds, 1);
    assert_eq!(
        summary.failures[0].failure,
        WorldFailure::MergedAgents {
            agent: d("d:bob"),
            first: common::agent_key("alice"),
            second: common::agent_key("bob"),
        }
    );
}

#[test]
fn predictions_that_fail_their_checks_fail_the_world() {
    let one = scene("w1");
    // d:carol has neither an attribution nor an unattributed row.
    let mut rows = attributed(&one);
    rows.push(found(&one, "d:carol"));
    let files = files(&[(&one.built, WorldStatus::Scored, rows)]);
    let summary = run(&files).unwrap();
    assert_eq!(summary.score.totals.worlds, 0);
    assert!(matches!(
        summary.failures[0].failure,
        WorldFailure::InvalidPredictions { .. }
    ));
}

#[test]
fn unattributed_senders_are_reported_not_scored() {
    let one = scene("w1");
    let rows = vec![
        Row::Attribution(Attribution {
            agent: d("d:bob"),
            exchanges: vec![one.b1],
        }),
        Row::Unattributed(Unattributed {
            agent: d("d:ghost"),
        }),
        found(&one, "d:ghost"),
    ];
    let files = files(&[(&one.built, WorldStatus::Scored, rows)]);
    let summary = run(&files).unwrap();
    assert_eq!(summary.score.totals.predictions, 0);
    assert_eq!(summary.score.total(&Selector::default()).missed, 1);
    assert_eq!(summary.unknown_detected_agents.len(), 1);
    let row = &summary.unknown_detected_agents[0];
    assert_eq!(
        (
            row.world.as_str(),
            row.transmission.as_str(),
            row.agent.as_str()
        ),
        ("w1", "t:d:ghost", "d:ghost")
    );
    let report = Report::new(summary, Vec::new(), Disclosure::Full);
    assert_eq!(report.unknown_detected_agent, 1);
}

#[test]
fn worlds_must_come_in_the_same_order() {
    let (one, two) = (scene("w1"), scene("w2"));
    let export = files(&[
        (&one.built, WorldStatus::Scored, attributed(&one)),
        (&two.built, WorldStatus::Scored, attributed(&two)),
    ]);
    let swapped = files(&[
        (&two.built, WorldStatus::Scored, attributed(&two)),
        (&one.built, WorldStatus::Scored, attributed(&one)),
    ]);
    let mixed = Files {
        predictions: swapped.predictions,
        ..export
    };
    let error = run(&mixed).err();
    assert!(
        matches!(
            &error,
            Some(RunError::WorldOrder { file: FileName::Predictions, expected, got })
                if expected.as_str() == "w1" && got.as_str() == "w2"
        ),
        "{error:?}"
    );
}

#[test]
fn a_missing_world_is_an_error() {
    let (one, two) = (scene("w1"), scene("w2"));
    let export = files(&[
        (&one.built, WorldStatus::Scored, attributed(&one)),
        (&two.built, WorldStatus::Scored, attributed(&two)),
    ]);
    let short = files(&[(&one.built, WorldStatus::Scored, attributed(&one))]);
    let mixed = Files {
        labels: short.labels,
        ..export
    };
    let error = run(&mixed).err();
    assert!(
        matches!(
            &error,
            Some(RunError::MissingWorld { file: FileName::Labels, expected }) if expected.as_str() == "w2"
        ),
        "{error:?}"
    );
}

#[test]
fn a_truncated_file_is_an_error_not_a_smaller_score() {
    let one = scene("w1");
    let mut files = files(&[(&one.built, WorldStatus::Scored, attributed(&one))]);
    let cut = files.predictions[..files.predictions.len() - 1]
        .iter()
        .rposition(|b| *b == b'\n')
        .unwrap();
    files.predictions.truncate(cut + 1);
    assert!(matches!(
        run(&files),
        Err(RunError::Read {
            file: FileName::Predictions,
            ..
        })
    ));
}

#[test]
fn reports_are_byte_identical_across_runs() {
    let (one, two) = (scene("w1"), scene("w2"));
    let files = files(&[
        (&one.built, WorldStatus::Scored, attributed(&one)),
        (&two.built, WorldStatus::Scored, Vec::new()),
    ]);
    let json = || {
        Report::new(run(&files).unwrap(), Vec::new(), Disclosure::Full)
            .to_json()
            .unwrap()
    };
    let first = json();
    assert_eq!(first, json());
    assert!(first.ends_with('\n'));
}

fn manifest(worlds: &[&Built]) -> Manifest {
    let zero = a2a_bench_format::ids::Digest::from_bytes([0; 32]);
    Manifest {
        format: FORMAT,
        dataset: common::dataset(),
        dataset_version: 1,
        split: Split::Dev,
        source: Source {
            path: "synthetic".into(),
            revision: "r0".into(),
            digest: zero,
        },
        converter: Converter {
            version: "0".into(),
            git: "0".into(),
        },
        selection: BTreeMap::new(),
        pace: BTreeMap::new(),
        worlds: worlds
            .iter()
            .map(|built| WorldEntry {
                key: built.key.clone(),
                exchanges: u64::try_from(built.exchanges.len()).unwrap(),
            })
            .collect(),
        files: FileDigests {
            messages: zero,
            exchanges: zero,
            labels: Some(zero),
        },
    }
}

fn write_export(dir: &std::path::Path, manifest: &Manifest, files: &Files) {
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string(manifest).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("messages.jsonl"), &files.messages).unwrap();
    std::fs::write(dir.join("exchanges.jsonl"), &files.exchanges).unwrap();
    std::fs::write(dir.join("labels.jsonl"), &files.labels).unwrap();
    std::fs::write(dir.join("predictions.jsonl"), &files.predictions).unwrap();
}

#[test]
fn an_export_directory_is_scored_against_its_manifest() {
    let one = scene("w1");
    let manifest = manifest(&[&one.built]);
    let files = common::files_for(
        &[(&one.built, WorldStatus::Scored, attributed(&one))],
        manifest.digest().unwrap(),
    );
    let dir = tempfile::tempdir().unwrap();
    write_export(dir.path(), &manifest, &files);
    let summary = score_export(
        dir.path(),
        &dir.path().join("predictions.jsonl"),
        ScoreOptions::default(),
    )
    .unwrap();
    assert_eq!(summary.score.total(&Selector::default()).found, 1);

    // Predictions made on another export are refused.
    let stale = common::files(&[(&one.built, WorldStatus::Scored, attributed(&one))]);
    write_export(dir.path(), &manifest, &stale);
    let error = score_export(
        dir.path(),
        &dir.path().join("predictions.jsonl"),
        ScoreOptions::default(),
    )
    .err();
    assert!(
        matches!(error, Some(RunError::ManifestDigest { .. })),
        "{error:?}"
    );
}

#[test]
fn a_partial_world_is_scored_under_its_coverage() {
    let mut draft = Draft::new("w1");
    let alice = draft.agent("alice");
    let bob = draft.agent("bob");
    let a1 = draft.exchange(&alice, 1, &[&user("go")], &[&says(NOTE)]);
    let delivered = user(NOTE);
    let b1 = draft.exchange(&bob, 2, &[&delivered], &[&says("ok")]);
    let built = draft.finish(Coverage::Partial);
    let one = Scene {
        read_at: whole(b1, &delivered, 0),
        built,
        a1,
        b1,
    };
    let files = files(&[(&one.built, WorldStatus::Scored, attributed(&one))]);
    let all = run(&files).unwrap().score.total(&Selector::default());
    assert_eq!((all.predicted, all.unjudged, all.false_positive), (1, 1, 0));
}
