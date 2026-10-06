//! `diff --normalize-ids` on predictions: detector agents are named by the
//! exchanges they hold, transmissions by their content, and the detector's
//! header fields are reported apart from row differences.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use a2a_bench_format::files::Predictions;
use a2a_bench_format::jsonl::{FileReader, FileWriter};
use a2a_bench_format::predictions::Prediction;
use common::{assert_code, fixture, gates, reference, run, scratch, stdout};
use serde_json::Value;

/// The reference's predictions on the SALT fixture.
fn predictions(dir: &Path) -> PathBuf {
    let out = dir.join("export");
    assert_code(
        &run(&[
            OsStr::new("export"),
            OsStr::new("--dataset"),
            OsStr::new("salt"),
            OsStr::new("--dataset-dir"),
            fixture("salt/tests/fixtures/salt").as_os_str(),
            OsStr::new("--out"),
            out.as_os_str(),
        ]),
        0,
    );
    let run_dir = dir.join("run");
    assert_code(
        &run(&[
            OsStr::new("run"),
            OsStr::new("--export"),
            out.as_os_str(),
            OsStr::new("--detector-cmd"),
            OsStr::new(&format!("'{}'", reference().display())),
            OsStr::new("--out"),
            run_dir.as_os_str(),
            OsStr::new("--gates"),
            gates(dir, "salt", 0.0).as_os_str(),
        ]),
        0,
    );
    run_dir.join("predictions.jsonl")
}

/// Rewrites `from` into `to` (fresh trailer): `header` edits the header,
/// `rows` maps each world's rows as JSON.
fn rewrite(
    from: &Path,
    to: &Path,
    header: impl Fn(&mut Value),
    rows: impl Fn(Vec<Value>) -> Vec<Value>,
) {
    let reader = std::io::BufReader::new(std::fs::File::open(from).unwrap());
    let mut reader = FileReader::<Predictions, _>::open(reader).unwrap();
    let mut head = serde_json::to_value(reader.header()).unwrap();
    header(&mut head);
    let head = serde_json::from_value(head).unwrap();
    let mut writer =
        FileWriter::<Predictions, _>::new(std::fs::File::create(to).unwrap(), &head).unwrap();
    while let Some(section) = reader.next_world().unwrap() {
        writer.world(&section.world).unwrap();
        let values = section
            .rows
            .iter()
            .map(|row| serde_json::to_value(row).unwrap())
            .collect();
        for value in rows(values) {
            let row: Prediction = serde_json::from_value(value).unwrap();
            writer.row(&row).unwrap();
        }
    }
    writer.finish().unwrap();
}

/// Renames every detector agent (`n` → `other-n`) wherever it appears.
fn rename_agent(value: &mut Value) {
    let rename = |field: &mut Value| {
        let name = field.as_str().unwrap().to_owned();
        *field = Value::String(format!("other-{name}"));
    };
    if let Some(agent) = value.get_mut("agent") {
        rename(agent);
    }
    for list in ["matches", "co_access"] {
        if let Some(Value::Array(items)) = value.get_mut(list) {
            for item in items {
                rename(&mut item["from"]);
                rename(&mut item["to"]);
            }
        }
    }
}

/// Other agent names, other transmission ids, attribution exchanges
/// reversed, rows of each world reversed.
fn renamed(rows: Vec<Value>) -> Vec<Value> {
    let mut rows: Vec<Value> = rows
        .into_iter()
        .map(|mut row| {
            rename_agent(&mut row);
            if row["kind"] == "transmission" {
                let id = row["id"].as_str().unwrap().to_owned();
                row["id"] = Value::String(format!("other-{id}"));
            }
            if let Some(Value::Array(exchanges)) = row.get_mut("exchanges") {
                exchanges.reverse();
            }
            row
        })
        .collect();
    rows.reverse();
    rows
}

fn diff(a: &Path, b: &Path, normalize: bool) -> std::process::Output {
    let mut args = vec![OsStr::new("diff"), a.as_os_str(), b.as_os_str()];
    if normalize {
        args.push(OsStr::new("--normalize-ids"));
    }
    run(&args)
}

#[test]
fn identical_splits_with_other_names_and_ids_diff_empty() {
    let scratch = scratch();
    let original = predictions(scratch.path());
    let other = scratch.path().join("other.jsonl");
    rewrite(&original, &other, |_| {}, renamed);
    assert_code(&diff(&original, &other, false), 1);
    let normalized = diff(&original, &other, true);
    assert_code(&normalized, 0);
    let shown = stdout(&normalized);
    assert!(shown.contains(": 0 differences"), "{shown}");
    assert!(!shown.contains("header differences"), "{shown}");
}

#[test]
fn a_different_split_differs() {
    let scratch = scratch();
    let original = predictions(scratch.path());
    let split = scratch.path().join("split.jsonl");
    // The first attribution holding two or more exchanges gives its first
    // exchange to a new agent.
    rewrite(
        &original,
        &split,
        |_| {},
        |rows| {
            let mut out = Vec::new();
            let mut done = false;
            for mut row in rows {
                if !done
                    && row["kind"] == "attribution"
                    && row["exchanges"].as_array().unwrap().len() > 1
                {
                    let first = row["exchanges"].as_array_mut().unwrap().remove(0);
                    out.push(serde_json::json!({
                        "kind": "attribution",
                        "agent": "split-off",
                        "exchanges": [first],
                    }));
                    done = true;
                }
                out.push(row);
            }
            out
        },
    );
    let normalized = diff(&original, &split, true);
    assert_code(&normalized, 1);
    let shown = stdout(&normalized);
    assert!(shown.contains("attribution"), "{shown}");
}

#[test]
fn header_only_differences_are_reported_apart() {
    let scratch = scratch();
    let original = predictions(scratch.path());
    let other = scratch.path().join("other.jsonl");
    rewrite(
        &original,
        &other,
        |header| {
            let detector = &mut header["detector"];
            detector["name"] = Value::String("another-detector".into());
            detector["version"] = Value::String("9.9.9".into());
            detector["variant"] = Value::String("other-variant".into());
            detector["config_digest"] = Value::String("ab".repeat(32));
        },
        |rows| rows,
    );
    for normalize in [false, true] {
        let shown = diff(&original, &other, normalize);
        assert_code(&shown, 0);
        let text = stdout(&shown);
        assert!(text.contains(": 0 differences"), "{text}");
        assert!(text.contains("header differences"), "{text}");
        for field in ["name", "version", "variant", "config_digest"] {
            assert!(text.contains(&format!("/detector/{field}")), "{text}");
        }
        assert!(!text.contains("another-detector"), "values are not shown");
    }
}
