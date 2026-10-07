//! demo-swarm holdout exports through the binary (design §9.2): a whole
//! run made with a holdout seed, over the demo-swarm crate's synthetic
//! capture (`fixture`, its truth header given a seed of 1,000,001 and a
//! `bench.env` beside it). The seed rule, the commitment list, the
//! manifest marks, and `score`/`run`/`validate` on the export, with the
//! capture's predictions and with the reference detector's.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
#[path = "../../datasets/demo-swarm/tests/swarm_truth/fixture.rs"]
mod fixture;

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Output;

use a2a_bench_cli::holdout::demo_swarm::bench_env_seed;
use a2a_bench_corpus::split::Release;
use a2a_bench_format::files::Predictions;
use a2a_bench_format::ids::Digest;
use a2a_bench_format::jsonl::{FileReader, FileWriter};
use common::{assert_code, manifest, outside_repository, reference, run_str, scratch, stderr};

const SEED: u64 = 1_000_001;
const GATEWAY: &str = "crosstalk-gateway-export";
/// An image digest, as crosstalk's gateway export names its version.
const IMAGE: &str = "sha256:066b562ccb1eda65f031b08958daf62f1ced3b9f1151b8b8663034bc2a72347d";
const OTHER_IMAGE: &str = "sha256:3d64b30b83619949d0c0b2b22bc8332500a3c33184b2c01bacfe0e2339e15602";
const REFERENCE_RELEASE: &str = "reference@0.1.0";

fn gateway_release(image: &str) -> String {
    format!("{GATEWAY}@{image}")
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

struct Setup {
    /// Inside the repository: the capture, the splits dir, dev outputs.
    inside: tempfile::TempDir,
    /// Outside every repository: holdout outputs.
    outside: tempfile::TempDir,
}

impl Setup {
    fn run(&self) -> PathBuf {
        self.inside.path().join("run")
    }

    fn capture(&self) -> PathBuf {
        self.run().join("capture")
    }

    fn splits(&self) -> PathBuf {
        self.inside.path().join("splits")
    }

    fn commit(&self) -> PathBuf {
        self.splits().join("demo-swarm@1.holdout.commit")
    }

    fn out(&self, name: &str) -> PathBuf {
        self.outside.path().join(name)
    }
}

/// The fixture's capture in `<run>/capture`, its truth header's seed
/// `truth_seed`, and `bench_env` (if any) written to `<run>/bench.env`, as
/// node0 lays a run out.
fn setup_with(truth_seed: u64, bench_env: Option<&str>) -> Option<Setup> {
    let Some(outside) = outside_repository() else {
        eprintln!("no directory outside a git repository: demo-swarm holdout tests skipped");
        return None;
    };
    let setup = Setup {
        inside: scratch(),
        outside,
    };
    std::fs::create_dir_all(setup.capture()).unwrap();
    let mut truth = fixture::truth_rows();
    truth[0]["seed"] = serde_json::json!(truth_seed);
    fixture::write(&setup.capture(), &truth);
    if let Some(text) = bench_env {
        std::fs::write(setup.run().join("bench.env"), text).unwrap();
    }
    Some(setup)
}

fn node0_env(seed: u64) -> String {
    format!(
        "run=20261007T010203Z\nscenario=headline\nswarm=--agents 20 --duration 2m --seed {seed} --scenario headline \ncrosstalk_image={IMAGE}\n"
    )
}

fn setup() -> Option<Setup> {
    setup_with(SEED, Some(&node0_env(SEED)))
}

fn export(setup: &Setup, out: &Path, extra: &[&str]) -> Output {
    let (capture, splits) = (setup.capture(), setup.splits());
    let truth = capture.join("truth.jsonl");
    let mut args = vec![
        "export",
        "--dataset",
        "demo-swarm",
        "--inputs",
        s(&capture),
        "--truth",
        s(&truth),
        "--splits",
        s(&splits),
        "--out",
        s(out),
    ];
    args.extend(extra);
    run_str(&args)
}

fn holdout(setup: &Setup, out: &Path, release: &str) -> Output {
    export(setup, out, &["--split", "holdout", "--release", release])
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A manifest as JSON without what the input view drops.
fn input_view(mut m: serde_json::Value) -> serde_json::Value {
    m["files"].as_object_mut().unwrap().remove("labels");
    for world in m["worlds"].as_array_mut().unwrap() {
        let world = world.as_object_mut().unwrap();
        world.remove("labels");
        world.remove("notes");
    }
    m
}

/// A copy of the predictions at `from` under another detector name,
/// version and (optionally) manifest digest.
fn repoint(from: &Path, to: &Path, name: &str, version: &str, digest: Option<Digest>) {
    let mut reader =
        FileReader::<Predictions, _>::open(BufReader::new(File::open(from).unwrap())).unwrap();
    let mut header = reader.header().clone();
    header.detector.name = name.to_owned();
    header.detector.version = version.to_owned();
    if let Some(digest) = digest {
        header.manifest_digest = digest;
    }
    let mut writer =
        FileWriter::<Predictions, _>::new(BufWriter::new(File::create(to).unwrap()), &header)
            .unwrap();
    while let Some(section) = reader.next_world().unwrap() {
        writer.world(&section.world).unwrap();
        for row in &section.rows {
            writer.row(row).unwrap();
        }
    }
    let (mut file, _) = writer.finish().unwrap();
    file.flush().unwrap();
}

/// A gates file for `detector` with one recall gate of 0 on the fixture's
/// dataset.
fn gates(dir: &Path, detector: &str) -> PathBuf {
    let path = dir.join(format!("gates-{detector}.toml"));
    std::fs::write(
        &path,
        format!(
            "detector = \"{detector}\"\n\n[[gate]]\nname = \"demo-swarm: every label\"\ndataset = \"{}\"\nmetric = \"recall\"\nmin = 0.0\n",
            fixture::HEADLINE
        ),
    )
    .unwrap();
    path
}

#[test]
fn a_release_version_may_be_an_image_digest() {
    let release: Release = gateway_release(IMAGE).parse().unwrap();
    assert_eq!(release.detector(), GATEWAY);
    assert_eq!(release.tag(), IMAGE);
    assert_eq!(release.to_string(), gateway_release(IMAGE));
}

#[test]
fn bench_env_seeds_are_read_from_a_seed_line_or_the_swarm_arguments() {
    let path = Path::new("bench.env");
    assert_eq!(bench_env_seed(&node0_env(SEED), path).unwrap(), Some(SEED));
    assert_eq!(
        bench_env_seed("seed=1000002\n", path).unwrap(),
        Some(1_000_002)
    );
    assert_eq!(
        bench_env_seed("swarm=--seed=1000003 --agents 2\n", path).unwrap(),
        Some(1_000_003)
    );
    assert_eq!(
        bench_env_seed("run=x\nscenario=headline\n", path).unwrap(),
        None
    );
    assert!(bench_env_seed("seed=many\n", path).is_err());
    assert!(bench_env_seed("swarm=--seed\n", path).is_err());
    assert!(bench_env_seed("seed=1000001\nswarm=--seed 1000002\n", path).is_err());
}

#[test]
fn a_holdout_export_is_the_capture_marked_for_the_release() {
    let Some(setup) = setup() else { return };
    let out = setup.out("holdout");
    let release = gateway_release(IMAGE);
    let exported = holdout(&setup, &out, &release);
    assert_code(&exported, 0);
    assert!(common::stdout(&exported).contains("recorded"));
    assert!(common::stdout(&exported).contains("seed 1000001"));

    let m = manifest(&out);
    assert_eq!(m["split"], "holdout");
    assert_eq!(m["selection"]["release"], release.as_str());
    let capture_digest = m["selection"]["capture_digest"].as_str().unwrap();
    assert_eq!(capture_digest.len(), 64);
    assert!(m["files"]["labels"].is_string());

    // The input view is the capture's but for split, release and the
    // capture digest.
    let mut view = input_view(m);
    view["split"] = "dev".into();
    let selection = view["selection"].as_object_mut().unwrap();
    selection.remove("release");
    selection.remove("capture_digest");
    assert_eq!(view, json(&setup.capture().join("manifest.json")));
}

#[test]
fn the_commitment_lists_the_run_and_refuses_a_changed_one() {
    let Some(setup) = setup() else { return };
    assert_code(
        &holdout(&setup, &setup.out("h1"), &gateway_release(IMAGE)),
        0,
    );
    let listed = std::fs::read_to_string(setup.commit()).unwrap();
    let fields: Vec<&str> = listed.split_whitespace().collect();
    assert_eq!(listed.lines().count(), 1);
    assert_eq!(fields[0], fixture::RUN);
    assert_eq!(fields[1], SEED.to_string());
    assert_eq!(fields[2].len(), 64);

    // The same run again, for another release: the same entry.
    let again = holdout(&setup, &setup.out("h2"), REFERENCE_RELEASE);
    assert_code(&again, 0);
    assert!(common::stdout(&again).contains("matches"));
    assert_eq!(std::fs::read_to_string(setup.commit()).unwrap(), listed);

    // Another run's entry is kept beside it.
    let other = format!("01J99999999999999999999999 1000005 {}\n", "1".repeat(64));
    std::fs::write(setup.commit(), format!("{listed}{other}")).unwrap();
    assert_code(&holdout(&setup, &setup.out("h3"), REFERENCE_RELEASE), 0);

    // A different labels digest for the same run is refused.
    std::fs::write(
        setup.commit(),
        format!("{} {SEED} {}\n", fixture::RUN, "0".repeat(64)),
    )
    .unwrap();
    let changed = holdout(&setup, &setup.out("h4"), REFERENCE_RELEASE);
    assert_code(&changed, 1);
    assert!(
        stderr(&changed).contains("the holdout changed"),
        "{}",
        stderr(&changed)
    );

    // So is another seed for the same run.
    std::fs::write(setup.commit(), listed.replace("1000001", "1000009")).unwrap();
    let reseeded = holdout(&setup, &setup.out("h5"), REFERENCE_RELEASE);
    assert_code(&reseeded, 1);
    assert!(
        stderr(&reseeded).contains("with seed"),
        "{}",
        stderr(&reseeded)
    );

    std::fs::write(setup.commit(), "not a list\n").unwrap();
    let malformed = holdout(&setup, &setup.out("h6"), REFERENCE_RELEASE);
    assert_code(&malformed, 1);
}

#[test]
fn a_seed_below_a_million_is_not_a_holdout_run() {
    let Some(setup) = setup_with(999_999, Some(&node0_env(999_999))) else {
        return;
    };
    let out = setup.out("low");
    let refused = holdout(&setup, &out, REFERENCE_RELEASE);
    assert_code(&refused, 1);
    assert!(
        stderr(&refused).contains("not a holdout run"),
        "{}",
        stderr(&refused)
    );
    assert!(!out.exists());
    assert!(!setup.commit().exists());

    // A dev run's seed given by hand is refused the same way.
    let Some(dev) = setup_with(42, None) else {
        return;
    };
    let by_flag = export(
        &dev,
        &dev.out("low"),
        &[
            "--split",
            "holdout",
            "--release",
            REFERENCE_RELEASE,
            "--seed",
            "42",
        ],
    );
    assert_code(&by_flag, 1);
    assert!(stderr(&by_flag).contains("not a holdout run"));
}

#[test]
fn the_seed_comes_from_bench_env_or_the_flag_and_must_be_the_truths() {
    // No bench.env: --seed is required.
    let Some(bare) = setup_with(SEED, None) else {
        return;
    };
    let required = holdout(&bare, &bare.out("a"), REFERENCE_RELEASE);
    assert_code(&required, 1);
    assert!(
        stderr(&required).contains("needs --seed"),
        "{}",
        stderr(&required)
    );
    let flagged = export(
        &bare,
        &bare.out("b"),
        &[
            "--split",
            "holdout",
            "--release",
            REFERENCE_RELEASE,
            "--seed",
            "1000001",
        ],
    );
    assert_code(&flagged, 0);
    assert!(common::stdout(&flagged).contains("from --seed"));

    // bench.env in the inputs dir itself is read too; a --seed that
    // contradicts it is refused.
    let Some(setup) = setup_with(SEED, None) else {
        return;
    };
    std::fs::write(setup.capture().join("bench.env"), "seed=1000001\n").unwrap();
    let contradicted = export(
        &setup,
        &setup.out("c"),
        &[
            "--split",
            "holdout",
            "--release",
            REFERENCE_RELEASE,
            "--seed",
            "1000002",
        ],
    );
    assert_code(&contradicted, 1);
    assert!(stderr(&contradicted).contains("differs from the bench.env seed"));

    // A bench.env seed that is not the truth header's is refused.
    let Some(other) = setup_with(SEED, Some(&node0_env(1_000_002))) else {
        return;
    };
    let mismatched = holdout(&other, &other.out("d"), REFERENCE_RELEASE);
    assert_code(&mismatched, 1);
    assert!(
        stderr(&mismatched).contains("truth header"),
        "{}",
        stderr(&mismatched)
    );
}

#[test]
fn a_holdout_export_inside_a_repository_is_refused() {
    let Some(setup) = setup() else { return };
    let out = setup.inside.path().join("holdout");
    let refused = holdout(&setup, &out, REFERENCE_RELEASE);
    assert_code(&refused, 1);
    assert!(
        stderr(&refused).contains("inside a git repository"),
        "{}",
        stderr(&refused)
    );
    assert!(!out.exists());
    assert!(!setup.commit().exists());
}

#[test]
fn dev_exports_are_unchanged_and_refuse_a_seed() {
    let Some(setup) = setup() else { return };
    let dev = setup.inside.path().join("dev");
    assert_code(&export(&setup, &dev, &[]), 0);
    let m = manifest(&dev);
    assert_eq!(m["split"], "dev");
    assert!(m["selection"].get("release").is_none());
    assert!(m["selection"].get("capture_digest").is_none());
    assert_eq!(input_view(m), json(&setup.capture().join("manifest.json")));
    assert!(!setup.commit().exists());

    let seeded = export(
        &setup,
        &setup.inside.path().join("dev2"),
        &["--seed", "1000001"],
    );
    assert_code(&seeded, 1);
    assert!(stderr(&seeded).contains("--seed applies only"));
}

#[test]
fn the_captures_predictions_score_against_the_holdout_with_aggregates_only() {
    let Some(setup) = setup() else { return };
    // The capture's predictions: the reference detector on the dev export,
    // whose input view is the capture's manifest, renamed as the gateway
    // export at an image digest.
    let dev = setup.inside.path().join("dev");
    assert_code(&export(&setup, &dev, &[]), 0);
    let dev_run = setup.inside.path().join("dev-run");
    let cmd = format!("'{}'", reference().display());
    let reference_gates = gates(setup.inside.path(), "reference");
    let ran = run_str(&[
        "run",
        "--export",
        s(&dev),
        "--detector-cmd",
        &cmd,
        "--out",
        s(&dev_run),
        "--gates",
        s(&reference_gates),
    ]);
    assert_code(&ran, 0);
    let predictions = setup.inside.path().join("gateway.jsonl");
    repoint(
        &dev_run.join("predictions.jsonl"),
        &predictions,
        GATEWAY,
        IMAGE,
        None,
    );

    let release = gateway_release(IMAGE);
    let export_dir = setup.out("holdout");
    assert_code(&holdout(&setup, &export_dir, &release), 0);
    let gateway_gates = gates(setup.inside.path(), GATEWAY);
    let score = |predictions: &Path, out: &Path, release: &str| {
        run_str(&[
            "score",
            "--export",
            s(&export_dir),
            "--predictions",
            s(predictions),
            "--out",
            s(out),
            "--gates",
            s(&gateway_gates),
            "--holdout-release",
            release,
        ])
    };

    let scored = score(&predictions, &setup.out("s1"), &release);
    assert_code(&scored, 0);
    let report = json(&setup.out("s1").join("report.json"));
    assert_eq!(report["disclosure"], "holdout");
    for field in ["misses", "false_positives", "failures"] {
        assert!(report.get(field).is_none(), "{field}");
    }

    let validated = run_str(&["validate", s(&export_dir), "--predictions", s(&predictions)]);
    assert_code(&validated, 0);

    // Inside a repository: refused.
    let inside = score(&predictions, &setup.inside.path().join("s2"), &release);
    assert_code(&inside, 1);
    assert!(stderr(&inside).contains("inside a git repository"));

    // Another release than the export's: refused.
    let other = score(
        &predictions,
        &setup.out("s3"),
        &gateway_release(OTHER_IMAGE),
    );
    assert_code(&other, 1);
    assert!(
        stderr(&other).contains("is for release"),
        "{}",
        stderr(&other)
    );

    // A detector at another image than the release's: refused.
    let other_export = setup.out("holdout-other");
    assert_code(
        &holdout(&setup, &other_export, &gateway_release(OTHER_IMAGE)),
        0,
    );
    let untagged = run_str(&[
        "score",
        "--export",
        s(&other_export),
        "--predictions",
        s(&predictions),
        "--out",
        s(&setup.out("s4")),
        "--gates",
        s(&gateway_gates),
        "--holdout-release",
        &gateway_release(OTHER_IMAGE),
    ]);
    assert_code(&untagged, 1);
    assert!(
        stderr(&untagged).contains("needs a detector at tag"),
        "{}",
        stderr(&untagged)
    );

    // Predictions naming any other manifest: refused.
    let stray = setup.inside.path().join("stray.jsonl");
    repoint(
        &predictions,
        &stray,
        GATEWAY,
        IMAGE,
        Some(Digest::from_bytes([9; 32])),
    );
    let strayed = score(&stray, &setup.out("s5"), &release);
    assert_code(&strayed, 1);
    assert!(
        stderr(&strayed).contains("made on manifest"),
        "{}",
        stderr(&strayed)
    );

    // A recorded capture digest the export does not imply: refused.
    let mut m = manifest(&export_dir);
    m["selection"]["capture_digest"] = "0".repeat(64).into();
    std::fs::write(
        export_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&m).unwrap(),
    )
    .unwrap();
    let tampered = score(&predictions, &setup.out("s6"), &release);
    assert_code(&tampered, 1);
    assert!(
        stderr(&tampered).contains("capture view"),
        "{}",
        stderr(&tampered)
    );
}

#[test]
fn the_reference_detector_runs_on_a_holdout_export() {
    let Some(setup) = setup() else { return };
    let export_dir = setup.out("holdout");
    assert_code(&holdout(&setup, &export_dir, REFERENCE_RELEASE), 0);
    let cmd = format!("'{}'", reference().display());
    let reference_gates = gates(setup.inside.path(), "reference");
    let run = |out: &Path| {
        run_str(&[
            "run",
            "--export",
            s(&export_dir),
            "--detector-cmd",
            &cmd,
            "--out",
            s(out),
            "--gates",
            s(&reference_gates),
            "--holdout-release",
            REFERENCE_RELEASE,
        ])
    };
    let inside = run(&setup.inside.path().join("r"));
    assert_code(&inside, 1);
    assert!(stderr(&inside).contains("inside a git repository"));

    let out = setup.out("r");
    let ran = run(&out);
    assert_code(&ran, 0);
    let report = json(&out.join("report.json"));
    assert_eq!(report["disclosure"], "holdout");
    assert_eq!(report["detector"]["version"], "0.1.0");
    for field in ["misses", "false_positives", "failures"] {
        assert!(report.get(field).is_none(), "{field}");
    }
}

#[test]
fn a_capture_that_is_not_a_dev_view_is_refused() {
    let Some(setup) = setup() else { return };
    let path = setup.capture().join("manifest.json");
    let mut m = json(&path);
    m["split"] = "holdout".into();
    std::fs::write(&path, serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    let out = setup.out("h");
    let refused = holdout(&setup, &out, REFERENCE_RELEASE);
    assert_code(&refused, 1);
    assert!(
        stderr(&refused).contains("made from a dev capture"),
        "{}",
        stderr(&refused)
    );
    assert!(!out.join("manifest.json").exists());
    assert!(!setup.commit().exists());
}
