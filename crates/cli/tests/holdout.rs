//! The holdout (design §9.2) through the binary, with a synthetic dev list:
//! refusals, the commitment file, and release runs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use common::{
    assert_code, fixture, manifest, outside_repository, reference, run_str, scratch, stderr,
};

const RELEASE: &str = "reference@0.1.0";

struct Setup {
    /// Outside every repository: the dataset copy and holdout outputs.
    outside: tempfile::TempDir,
    /// Inside the repository: the splits dir and dev outputs.
    inside: tempfile::TempDir,
}

impl Setup {
    fn dataset(&self) -> PathBuf {
        self.outside.path().join("salt")
    }

    fn splits(&self) -> PathBuf {
        self.inside.path().join("splits")
    }
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The SALT fixture copied outside any repository (revision
/// `unversioned`), and a dev list naming its first world.
fn setup() -> Option<Setup> {
    let Some(outside) = outside_repository() else {
        eprintln!("no directory outside a git repository: holdout tests skipped");
        return None;
    };
    let setup = Setup {
        outside,
        inside: scratch(),
    };
    common::copy_tree(&fixture("salt/tests/fixtures/salt"), &setup.dataset());
    let all = setup.inside.path().join("all");
    let exported = export(&setup, &all, &[]);
    assert_code(&exported, 0);
    let m = manifest(&all);
    assert_eq!(m["source"]["revision"], "unversioned");
    let first = m["worlds"][0]["key"].as_str().unwrap().to_owned();
    std::fs::create_dir_all(setup.splits()).unwrap();
    std::fs::write(
        setup.splits().join("salt@1.dev.toml"),
        format!(
            "dataset = \"salt\"\nversion = 1\nrevision = \"unversioned\"\nworlds = [\"{first}\"]\n"
        ),
    )
    .unwrap();
    Some(setup)
}

fn export(setup: &Setup, out: &Path, extra: &[&str]) -> std::process::Output {
    let (dataset, splits) = (setup.dataset(), setup.splits());
    let mut args = vec![
        "export",
        "--dataset",
        "salt",
        "--dataset-dir",
        s(&dataset),
        "--splits",
        s(&splits),
        "--out",
        s(out),
    ];
    args.extend(extra);
    run_str(&args)
}

fn holdout(setup: &Setup, out: &Path) -> std::process::Output {
    export(setup, out, &["--split", "holdout", "--release", RELEASE])
}

#[test]
fn holdout_exports_are_refused_without_a_release_inside_a_repository_or_without_a_dev_list() {
    let Some(setup) = setup() else { return };
    let out = setup.outside.path().join("h");
    let no_release = export(&setup, &out, &["--split", "holdout"]);
    assert_code(&no_release, 1);
    assert!(stderr(&no_release).contains("needs --release"));

    let release_on_dev = export(&setup, &out, &["--release", RELEASE]);
    assert_code(&release_on_dev, 1);

    let inside = holdout(&setup, &setup.inside.path().join("h"));
    assert_code(&inside, 1);
    assert!(
        stderr(&inside).contains("inside a git repository"),
        "{}",
        stderr(&inside)
    );

    let no_list = run_str(&[
        "export",
        "--dataset",
        "salt",
        "--dataset-dir",
        s(&setup.dataset()),
        "--splits",
        s(&setup.inside.path().join("empty-splits")),
        "--out",
        s(&out),
        "--split",
        "holdout",
        "--release",
        RELEASE,
    ]);
    assert_code(&no_list, 1);
    assert!(
        stderr(&no_list).contains("no dev list"),
        "{}",
        stderr(&no_list)
    );
}

#[test]
fn a_holdout_export_commits_its_world_list_and_refuses_a_changed_one() {
    let Some(setup) = setup() else { return };
    let dev = setup.inside.path().join("dev");
    assert_code(&export(&setup, &dev, &[]), 0);
    assert_eq!(manifest(&dev)["worlds"].as_array().unwrap().len(), 1);
    assert_eq!(manifest(&dev)["split"], "dev");

    let first = setup.outside.path().join("h1");
    let exported = holdout(&setup, &first);
    assert_code(&exported, 0);
    let m = manifest(&first);
    assert_eq!(m["split"], "holdout");
    assert_eq!(m["selection"]["release"], RELEASE);
    assert_eq!(m["worlds"].as_array().unwrap().len(), 2);
    let commit = setup.splits().join("salt@1.holdout.commit");
    let committed = std::fs::read_to_string(&commit).unwrap();
    assert_eq!(committed.trim().len(), 64);
    assert!(common::stdout(&exported).contains("recorded"));

    let second = setup.outside.path().join("h2");
    let again = holdout(&setup, &second);
    assert_code(&again, 0);
    assert!(common::stdout(&again).contains("matches"));

    std::fs::write(&commit, format!("{}\n", "0".repeat(64))).unwrap();
    let changed = holdout(&setup, &setup.outside.path().join("h3"));
    assert_code(&changed, 1);
    assert!(
        stderr(&changed).contains("the holdout changed"),
        "{}",
        stderr(&changed)
    );
}

#[test]
fn a_holdout_run_needs_the_release_and_a_tagged_detector_and_reports_aggregates() {
    let Some(setup) = setup() else { return };
    let export_dir = setup.outside.path().join("holdout");
    assert_code(&holdout(&setup, &export_dir), 0);
    let gates = common::gates(setup.inside.path(), "salt", 0.0);
    let cmd = format!("'{}'", reference().display());
    let run = |out: &Path, release: Option<&str>| {
        let mut args = vec![
            "run",
            "--export",
            s(&export_dir),
            "--detector-cmd",
            cmd.as_str(),
            "--out",
            s(out),
            "--gates",
            s(&gates),
        ];
        if let Some(release) = release {
            args.extend(["--holdout-release", release]);
        }
        run_str(&args)
    };
    let outside = |name: &str| setup.outside.path().join(name);

    let no_release = run(&outside("r0"), None);
    assert_code(&no_release, 1);
    assert!(stderr(&no_release).contains("--holdout-release"));

    let other = run(&outside("r1"), Some("crosstalk-live@v1"));
    assert_code(&other, 1);
    assert!(
        stderr(&other).contains("is for release"),
        "{}",
        stderr(&other)
    );

    let inside = run(&setup.inside.path().join("r2"), Some(RELEASE));
    assert_code(&inside, 1);
    assert!(stderr(&inside).contains("inside a git repository"));

    let released = run(&outside("r3"), Some(RELEASE));
    assert_code(&released, 0);
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(outside("r3").join("report.json")).unwrap()).unwrap();
    assert_eq!(report["disclosure"], "holdout");
    for field in ["misses", "false_positives", "failures"] {
        assert!(report.get(field).is_none(), "{field}");
    }
    assert!(report["notes"][0]["labels"].as_u64().unwrap() > 0);

    // A detector whose version is not the release's tag is refused.
    let untagged_export = setup.outside.path().join("holdout-v9");
    // The commitment is the same holdout: the release is only a setting.
    assert_code(
        &export(
            &setup,
            &untagged_export,
            &["--split", "holdout", "--release", "reference@v9"],
        ),
        0,
    );
    let mut args = vec!["score", "--export", s(&untagged_export), "--predictions"];
    let predictions = outside("r3").join("predictions.jsonl");
    let out = outside("s9");
    args.extend([
        s(&predictions),
        "--out",
        s(&out),
        "--holdout-release",
        "reference@v9",
    ]);
    let untagged = run_str(&args);
    assert_code(&untagged, 1);
    assert!(
        stderr(&untagged).contains("needs a detector at tag"),
        "{}",
        stderr(&untagged)
    );
}

#[test]
fn a_dev_export_refuses_a_holdout_release() {
    let scratch = scratch();
    let out = scratch.path().join("dev");
    assert_code(
        &run_str(&[
            "export",
            "--dataset",
            "salt",
            "--dataset-dir",
            s(&fixture("salt/tests/fixtures/salt")),
            "--out",
            s(&out),
        ]),
        0,
    );
    let refused = run_str(&[
        "score",
        "--export",
        s(&out),
        "--predictions",
        s(&out.join("missing.jsonl")),
        "--out",
        s(&scratch.path().join("s")),
        "--holdout-release",
        RELEASE,
    ]);
    assert_code(&refused, 1);
    assert!(stderr(&refused).contains("not a holdout export"));
}
