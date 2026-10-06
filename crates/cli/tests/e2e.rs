//! export → validate → run (a2a-reference) → score → gates, over the
//! converters' synthetic fixtures; byte-identical reruns; the input view;
//! diff.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use common::{
    assert_code, bench, fixture, gates, manifest, reference, run, run_str, scratch, stdout, tree,
};

fn salt() -> PathBuf {
    fixture("salt/tests/fixtures/salt")
}

fn export(dataset: &str, dir: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    let mut args: Vec<&OsStr> = vec![
        OsStr::new("export"),
        OsStr::new("--dataset"),
        OsStr::new(dataset),
        OsStr::new("--dataset-dir"),
        dir.as_os_str(),
        OsStr::new("--out"),
        out.as_os_str(),
    ];
    args.extend(extra.iter().map(OsStr::new));
    run(&args)
}

fn reference_cmd() -> String {
    format!("'{}'", reference().display())
}

#[test]
fn salt_export_validate_run_score_and_gates() {
    let scratch = scratch();
    let out = scratch.path().join("export");
    let exported = export("salt", &salt(), &out, &[]);
    assert_code(&exported, 0);
    assert!(
        stdout(&exported).contains("3 worlds"),
        "{}",
        stdout(&exported)
    );
    let m = manifest(&out);
    assert_eq!(m["dataset"], "salt");
    assert_eq!(m["converter"]["git"].as_str().map(str::len), Some(40));
    assert_ne!(
        m["source"]["digest"],
        serde_json::json!("0".repeat(64)),
        "the source digest is recorded"
    );
    assert_eq!(m["pace"]["min_ms"], 1000);
    assert_eq!(m["selection"]["split_list"], "none");

    let validated = run(&[OsStr::new("validate"), out.as_os_str()]);
    assert_code(&validated, 0);
    assert!(stdout(&validated).contains("valid"));

    // A gate the reference passes, then one it fails (exit 2).
    let pass = gates(scratch.path(), "salt", 0.5);
    let fail = gates(scratch.path(), "salt", 0.99);
    let run_dir = scratch.path().join("run");
    let ran = run(&[
        OsStr::new("run"),
        OsStr::new("--export"),
        out.as_os_str(),
        OsStr::new("--detector-cmd"),
        OsStr::new(&reference_cmd()),
        OsStr::new("--out"),
        run_dir.as_os_str(),
        OsStr::new("--gates"),
        pass.as_os_str(),
        OsStr::new("--twice"),
    ]);
    assert_code(&ran, 0);
    let table = stdout(&ran);
    assert!(table.contains("overall: recall"), "{table}");
    assert!(table.contains("notes: salt"), "{table}");
    assert!(table.contains("pass"), "{table}");
    let predictions = run_dir.join("predictions.jsonl");
    for file in [
        "report.json",
        "report.txt",
        "predictions.jsonl",
        "predictions.second.jsonl",
    ] {
        assert!(run_dir.join(file).is_file(), "{file}");
    }
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run_dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["notes"][0]["dataset"], "salt");
    assert!(report["overall"]["found"].as_u64().unwrap() > 0);

    let with_predictions = run(&[
        OsStr::new("validate"),
        out.as_os_str(),
        OsStr::new("--predictions"),
        predictions.as_os_str(),
    ]);
    assert_code(&with_predictions, 0);
    assert!(stdout(&with_predictions).contains("predictions:"));

    let score = |dir: &Path, gates: &Path| {
        run(&[
            OsStr::new("score"),
            OsStr::new("--export"),
            out.as_os_str(),
            OsStr::new("--predictions"),
            predictions.as_os_str(),
            OsStr::new("--out"),
            dir.as_os_str(),
            OsStr::new("--gates"),
            gates.as_os_str(),
        ])
    };
    let failed = score(&scratch.path().join("score-fail"), &fail);
    assert_code(&failed, 2);
    assert!(stdout(&failed).contains("FAIL"));

    // Byte-identical reruns of score.
    let (one, two) = (scratch.path().join("s1"), scratch.path().join("s2"));
    assert_code(&score(&one, &pass), 0);
    assert_code(&score(&two, &pass), 0);
    assert_eq!(tree(&one), tree(&two));
    assert_eq!(
        std::fs::read(one.join("report.json")).unwrap(),
        std::fs::read(run_dir.join("report.json")).unwrap()
    );
}

#[test]
fn exports_are_byte_identical_across_reruns() {
    let scratch = scratch();
    let (one, two) = (scratch.path().join("one"), scratch.path().join("two"));
    assert_code(&export("salt", &salt(), &one, &["--limit", "2"]), 0);
    assert_code(&export("salt", &salt(), &two, &["--limit", "2"]), 0);
    assert_eq!(tree(&one), tree(&two));
    // An export directory must be empty.
    assert_code(&export("salt", &salt(), &one, &["--limit", "2"]), 1);
}

#[test]
fn the_input_view_never_holds_labels() {
    let scratch = scratch();
    let out = scratch.path().join("export");
    assert_code(&export("salt", &salt(), &out, &[]), 0);
    let view = scratch.path().join("view");
    let built = run(&[OsStr::new("input-view"), out.as_os_str(), view.as_os_str()]);
    assert_code(&built, 0);
    let files: Vec<PathBuf> = tree(&view).into_iter().map(|(path, _)| path).collect();
    assert_eq!(
        files,
        ["exchanges.jsonl", "manifest.json", "messages.jsonl"].map(PathBuf::from)
    );
    let m = manifest(&view);
    assert!(m["files"].get("labels").is_none());
    for world in m["worlds"].as_array().unwrap() {
        assert!(world.get("labels").is_none() && world.get("notes").is_none());
    }
    // The view validates as inputs only.
    let validated = run(&[OsStr::new("validate"), view.as_os_str()]);
    assert_code(&validated, 0);
    assert!(stdout(&validated).contains("labels: none"));
}

#[test]
fn validate_finds_a_tampered_export_and_prints_no_text() {
    let scratch = scratch();
    let out = scratch.path().join("export");
    assert_code(&export("salt", &salt(), &out, &[]), 0);
    let labels = out.join("labels.jsonl");
    let text = std::fs::read_to_string(&labels).unwrap();
    // Drop one transmission label: the trailer no longer matches.
    let mut lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("\"kind\":\"transmission\""))
        .unwrap();
    let dropped = lines.remove(at).to_owned();
    std::fs::write(&labels, lines.join("\n") + "\n").unwrap();
    let validated = run(&[OsStr::new("validate"), out.as_os_str()]);
    assert_code(&validated, 1);
    let shown = stdout(&validated) + &common::stderr(&validated);
    assert!(shown.contains("problems"), "{shown}");
    let label: serde_json::Value = serde_json::from_str(&dropped).unwrap();
    let content = label["content"]["text"].as_str().unwrap();
    assert!(!shown.contains(content), "label text leaked: {shown}");
}

#[test]
fn diff_on_identical_and_differing_exports() {
    let scratch = scratch();
    let (one, two, small) = (
        scratch.path().join("one"),
        scratch.path().join("two"),
        scratch.path().join("small"),
    );
    assert_code(&export("salt", &salt(), &one, &[]), 0);
    assert_code(&export("salt", &salt(), &two, &[]), 0);
    assert_code(&export("salt", &salt(), &small, &["--limit", "1"]), 0);
    let same = run(&[OsStr::new("diff"), one.as_os_str(), two.as_os_str()]);
    assert_code(&same, 0);
    assert!(
        stdout(&same).contains(": 0 differences"),
        "{}",
        stdout(&same)
    );

    let differ = run(&[OsStr::new("diff"), one.as_os_str(), small.as_os_str()]);
    assert_code(&differ, 1);
    let shown = stdout(&differ);
    assert!(shown.contains("only in a"), "{shown}");
    assert!(shown.contains("manifest"), "{shown}");

    // Mixed kinds are an error (exit 2).
    let mixed = run(&[
        OsStr::new("diff"),
        one.as_os_str(),
        one.join("labels.jsonl").as_os_str(),
    ]);
    assert_code(&mixed, 2);
}

#[test]
fn diff_of_predictions_can_ignore_transmission_ids() {
    let scratch = scratch();
    let out = scratch.path().join("export");
    assert_code(&export("salt", &salt(), &out, &[]), 0);
    let run_dir = scratch.path().join("run");
    assert_code(
        &run(&[
            OsStr::new("run"),
            OsStr::new("--export"),
            out.as_os_str(),
            OsStr::new("--detector-cmd"),
            OsStr::new(&reference_cmd()),
            OsStr::new("--out"),
            run_dir.as_os_str(),
            OsStr::new("--gates"),
            gates(scratch.path(), "salt", 0.0).as_os_str(),
        ]),
        0,
    );
    let original = run_dir.join("predictions.jsonl");
    // The same predictions with renamed transmission ids (and the trailer
    // rewritten so the file stays valid).
    let renamed = scratch.path().join("renamed.jsonl");
    rename_transmissions(&original, &renamed);
    let strict = run(&[
        OsStr::new("diff"),
        original.as_os_str(),
        renamed.as_os_str(),
    ]);
    assert_code(&strict, 1);
    let normalized = run(&[
        OsStr::new("diff"),
        original.as_os_str(),
        renamed.as_os_str(),
        OsStr::new("--normalize-ids"),
    ]);
    assert_code(&normalized, 0);
}

/// Rewrites `from`'s transmission ids (`t:…` → `t:renamed-…`) into `to`,
/// with a fresh trailer.
fn rename_transmissions(from: &Path, to: &Path) {
    use a2a_bench_format::files::Predictions;
    use a2a_bench_format::jsonl::{FileReader, FileWriter};
    use a2a_bench_format::predictions::Prediction;
    let reader = std::io::BufReader::new(std::fs::File::open(from).unwrap());
    let mut reader = FileReader::<Predictions, _>::open(reader).unwrap();
    let mut writer =
        FileWriter::<Predictions, _>::new(std::fs::File::create(to).unwrap(), reader.header())
            .unwrap();
    while let Some(section) = reader.next_world().unwrap() {
        writer.world(&section.world).unwrap();
        for row in section.rows {
            let row = match row {
                Prediction::Transmission(t) => {
                    let mut value = serde_json::to_value(Prediction::Transmission(t)).unwrap();
                    let id = value["id"].as_str().unwrap().to_owned();
                    value["id"] = serde_json::Value::String(format!("t:renamed-{id}"));
                    serde_json::from_value(value).unwrap()
                }
                other => other,
            };
            writer.row(&row).unwrap();
        }
    }
    writer.finish().unwrap();
}

#[test]
fn every_fixture_converter_exports_and_validates() {
    let scratch = scratch();
    let cases: &[(&str, &str, &[&str])] = &[
        ("agentdojo", "agentdojo/tests/fixtures", &[]),
        ("tau2", "tau2/tests/fixtures", &[]),
        ("collusion-wiki", "wiki/tests/fixtures/collusion-wiki", &[]),
        ("swarm-traces", "swarm/tests/fixtures/swarm-traces", &[]),
        ("cipher", "cipher/tests/fixtures/cipher", &["--count", "2"]),
        (
            "open_swe",
            "open-swe/tests/fixtures/open_swe",
            &["--agents-per-world", "4"],
        ),
        ("lmcache", "lmcache/tests/fixtures/lmcache", &[]),
        (
            "swe_splice",
            "open-swe/tests/fixtures/open_swe",
            &["--count", "2"],
        ),
    ];
    for (dataset, dir, extra) in cases {
        let out = scratch.path().join(dataset);
        let exported = export(dataset, &fixture(dir), &out, extra);
        assert_code(&exported, 0);
        let validated = run(&[OsStr::new("validate"), out.as_os_str()]);
        assert_code(&validated, 0);
        let m = manifest(&out);
        assert_eq!(m["dataset"], *dataset);
        assert!(
            !m["worlds"].as_array().unwrap().is_empty(),
            "{dataset}: no worlds"
        );
    }
}

#[test]
fn a_flag_the_dataset_does_not_read_is_refused() {
    let scratch = scratch();
    let out = scratch.path().join("x");
    let refused = export(
        "tau2",
        &fixture("tau2/tests/fixtures"),
        &out,
        &["--corpus-seed", "3"],
    );
    assert_code(&refused, 1);
    assert!(common::stderr(&refused).contains("--corpus-seed does not apply to tau2"));
    let wrong_version = export("salt", &salt(), &out, &["--version", "7"]);
    assert_code(&wrong_version, 1);
    let no_inputs = run_str(&[
        "export",
        "--dataset",
        "demo-swarm",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_code(&no_inputs, 1);
    assert!(common::stderr(&no_inputs).contains("needs --inputs"));
}

#[test]
fn a_pinned_revision_is_enforced() {
    let scratch = scratch();
    let config = scratch.path().join("datasets.toml");
    std::fs::write(
        &config,
        format!(
            "root = \"{}\"\n\n[datasets.salt]\npath = \"salt/tests/fixtures/salt\"\nrevision = \"not-the-revision\"\n",
            common::workspace().join("crates/datasets").display()
        ),
    )
    .unwrap();
    let out = scratch.path().join("pinned");
    let args = |extra: &[&str]| {
        let mut args = vec![
            "export",
            "--dataset",
            "salt",
            "--config",
            config.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ];
        args.extend(extra);
        args.into_iter().map(str::to_owned).collect::<Vec<_>>()
    };
    let refused = run_str(&args(&[]).iter().map(String::as_str).collect::<Vec<_>>());
    assert_code(&refused, 1);
    assert!(common::stderr(&refused).contains("--allow-revision"));
    let allowed = run_str(
        &args(&["--allow-revision"])
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert_code(&allowed, 0);
    // The actual revision is recorded, not the pin.
    assert_ne!(manifest(&out)["source"]["revision"], "not-the-revision");
    assert_eq!(manifest(&out)["source"]["path"], "salt/tests/fixtures/salt");
}

#[test]
fn the_binary_has_every_command() {
    let help = std::process::Command::new(bench())
        .arg("--help")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&help.stdout).into_owned();
    for command in ["export", "validate", "input-view", "run", "score", "diff"] {
        assert!(text.contains(command), "{command}: {text}");
    }
}
