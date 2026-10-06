//! The `a2a-reference` binary against the detector contract: input view in,
//! a complete predictions file out, byte-identical across runs.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use a2a_bench_format::check::{WorldInputs, check_predictions};
use a2a_bench_format::exchange::{Exchange, WorldDecl};
use a2a_bench_format::files::{
    ExchangeRow, Exchanges, MessageRow, Messages, Predictions, WorldOnly,
};
use a2a_bench_format::ids::Digest;
use a2a_bench_format::jsonl::{BasicHeader, FileReader, FileWriter};
use a2a_bench_format::manifest::{Converter, FileDigests, Manifest, Source, Split, WorldEntry};
use a2a_bench_format::message::Message;
use a2a_bench_format::predictions::{Prediction, WorldStatus};
use a2a_bench_format::version::FORMAT;
use common::{WorldBuilder, dataset, says, system, user};

const SENTENCE: &str = "The vendor table has eleven overdue approvals in March";

type World = (WorldDecl, Vec<Message>, Vec<Exchange>);

fn relay(key: &str) -> World {
    let mut builder = WorldBuilder::new(&["alice", "bob"]).named(key);
    builder.exchange(
        "alice",
        1,
        vec![system("You are Alice."), user("Report.")],
        vec![says(SENTENCE)],
    );
    builder.exchange(
        "bob",
        2,
        vec![
            system("You are Bob."),
            user(&format!("alice says: {SENTENCE}")),
        ],
        vec![says("Noted.")],
    );
    builder.parts()
}

fn quiet(key: &str) -> World {
    let mut builder = WorldBuilder::new(&["carol"]).named(key);
    builder.exchange("carol", 1, vec![user("hello")], vec![says("hi")]);
    builder.parts()
}

fn digest(text: &str) -> Digest {
    Digest::keyed("a2a-bench test", text.as_bytes())
}

/// Writes an input view of `worlds` into `dir`; the messages file holds
/// `message_order` (default: the same order).
fn write_export(dir: &Path, worlds: &[World], message_order: Option<&[World]>) -> Manifest {
    let messages_file = BufWriter::new(File::create(dir.join("messages.jsonl")).unwrap());
    let mut messages =
        FileWriter::<Messages, _>::new(messages_file, &BasicHeader::new::<Messages>(dataset()))
            .unwrap();
    for (decl, rows, _) in message_order.unwrap_or(worlds) {
        messages
            .world(&WorldOnly {
                key: decl.key.clone(),
            })
            .unwrap();
        for row in rows {
            messages.row(&MessageRow::Message(row.clone())).unwrap();
        }
    }
    let (_, messages_trailer) = messages.finish().unwrap();
    let exchanges_file = BufWriter::new(File::create(dir.join("exchanges.jsonl")).unwrap());
    let mut exchanges =
        FileWriter::<Exchanges, _>::new(exchanges_file, &BasicHeader::new::<Exchanges>(dataset()))
            .unwrap();
    for (decl, _, rows) in worlds {
        exchanges.world(decl).unwrap();
        for row in rows {
            exchanges.row(&ExchangeRow::Exchange(row.clone())).unwrap();
        }
    }
    let (_, exchanges_trailer) = exchanges.finish().unwrap();
    let manifest = Manifest {
        format: FORMAT,
        dataset: dataset(),
        dataset_version: 1,
        split: Split::Dev,
        source: Source {
            path: "synthetic".into(),
            revision: "0".into(),
            digest: digest("source"),
        },
        converter: Converter {
            version: "0.1.0".into(),
            git: "0".into(),
        },
        selection: BTreeMap::new(),
        pace: BTreeMap::new(),
        worlds: worlds
            .iter()
            .map(|(decl, _, exchanges)| WorldEntry {
                key: decl.key.clone(),
                exchanges: exchanges.len() as u64,
            })
            .collect(),
        files: FileDigests {
            messages: messages_trailer.digest,
            exchanges: exchanges_trailer.digest,
            labels: None,
        },
    };
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    manifest
}

fn reference(input: &Path, output: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_a2a-reference"))
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output)
        .args(extra)
        .output()
        .unwrap()
}

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("a2a-reference-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

/// The predictions file's header and its worlds' sections.
fn read_predictions(
    path: &PathBuf,
) -> (
    a2a_bench_format::files::PredictionsHeader,
    Vec<(String, WorldStatus, Vec<Prediction>)>,
) {
    let mut reader =
        FileReader::<Predictions, _>::open(BufReader::new(File::open(path).unwrap())).unwrap();
    let header = reader.header().clone();
    let mut worlds = Vec::new();
    while let Some(section) = reader.next_world().unwrap() {
        worlds.push((
            section.world.key.to_string(),
            section.world.status,
            section.rows,
        ));
    }
    (header, worlds)
}

fn inputs((decl, messages, exchanges): &World) -> WorldInputs {
    WorldInputs::new(&decl.key, messages.clone(), decl.clone(), exchanges.clone()).unwrap()
}

#[test]
fn a_run_writes_a_complete_predictions_file() {
    let dir = tempdir();
    let worlds = [relay("w1"), quiet("w2")];
    let manifest = write_export(dir.path(), &worlds, None);
    let out = dir.path().join("predictions.jsonl");
    let run = reference(dir.path(), &out, &[]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let (header, sections) = read_predictions(&out);
    assert_eq!(header.detector.name, "reference");
    assert_eq!(header.detector.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(header.detector.variant, "max-postings-50");
    assert!(header.detector.config_digest.is_some());
    assert_eq!(header.dataset, dataset());
    assert_eq!(header.manifest_digest, manifest.digest().unwrap());
    let keys: Vec<&str> = sections.iter().map(|(key, _, _)| key.as_str()).collect();
    assert_eq!(keys, ["w1", "w2"]);
    for ((_, status, rows), world) in sections.iter().zip(&worlds) {
        assert_eq!(status, &WorldStatus::Scored);
        check_predictions(&inputs(world), rows).unwrap();
    }
    let transmissions = |rows: &[Prediction]| {
        rows.iter()
            .filter(|row| matches!(row, Prediction::Transmission(_)))
            .count()
    };
    assert_eq!(transmissions(&sections[0].2), 1);
    assert_eq!(transmissions(&sections[1].2), 0);
}

#[test]
fn two_runs_are_byte_identical() {
    let dir = tempdir();
    write_export(dir.path(), &[relay("w1"), quiet("w2"), relay("w3")], None);
    let (first, second) = (dir.path().join("a.jsonl"), dir.path().join("b.jsonl"));
    assert!(reference(dir.path(), &first, &[]).status.success());
    assert!(reference(dir.path(), &second, &[]).status.success());
    let (first, second) = (
        std::fs::read(first).unwrap(),
        std::fs::read(second).unwrap(),
    );
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

#[test]
fn the_cutoff_names_the_variant() {
    let dir = tempdir();
    write_export(dir.path(), &[relay("w1")], None);
    let out = dir.path().join("predictions.jsonl");
    let default = dir.path().join("default.jsonl");
    assert!(
        reference(dir.path(), &out, &["--max-postings", "7"])
            .status
            .success()
    );
    assert!(reference(dir.path(), &default, &[]).status.success());
    let (header, _) = read_predictions(&out);
    let (default, _) = read_predictions(&default);
    assert_eq!(header.detector.variant, "max-postings-7");
    assert_ne!(
        header.detector.config_digest,
        default.detector.config_digest
    );
}

#[test]
fn a_world_that_cannot_be_processed_fails_alone() {
    let dir = tempdir();
    let (decl, mut messages, exchanges) = relay("broken");
    messages.pop();
    let worlds = [quiet("w1"), (decl, messages, exchanges), relay("w3")];
    write_export(dir.path(), &worlds, None);
    let out = dir.path().join("predictions.jsonl");
    let run = reference(dir.path(), &out, &[]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let (_, sections) = read_predictions(&out);
    let statuses: Vec<_> = sections
        .iter()
        .map(|(key, status, rows)| (key.as_str(), status, rows.len()))
        .collect();
    assert_eq!(statuses[0].1, &WorldStatus::Scored);
    assert!(
        matches!(statuses[1], ("broken", WorldStatus::Failed { reason }, 0) if !reason.is_empty()),
        "{statuses:?}"
    );
    assert_eq!(statuses[2].1, &WorldStatus::Scored);
}

#[test]
fn a_credential_the_reference_cannot_name_fails_the_world() {
    let dir = tempdir();
    let mut builder = WorldBuilder::new(&["carol"]).named("w1");
    builder.exchange_with("", "carol", 1, vec![user("hello")], vec![says("hi")]);
    write_export(dir.path(), &[builder.parts()], None);
    let out = dir.path().join("predictions.jsonl");
    assert!(reference(dir.path(), &out, &[]).status.success());
    let (_, sections) = read_predictions(&out);
    assert!(matches!(&sections[0].1, WorldStatus::Failed { .. }));
}

#[test]
fn worlds_out_of_order_across_files_are_a_run_failure() {
    let dir = tempdir();
    let worlds = [relay("w1"), quiet("w2")];
    let swapped = [quiet("w2"), relay("w1")];
    write_export(dir.path(), &worlds, Some(&swapped));
    let out = dir.path().join("predictions.jsonl");
    let run = reference(dir.path(), &out, &[]);
    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("world order differs"), "{stderr}");
}

#[test]
fn worlds_the_manifest_does_not_list_are_a_run_failure() {
    let dir = tempdir();
    let manifest = write_export(dir.path(), &[relay("w1")], None);
    let mut other = manifest;
    other.worlds[0].key = a2a_bench_format::ids::WorldKey::new("elsewhere").unwrap();
    std::fs::write(
        dir.path().join("manifest.json"),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let out = dir.path().join("predictions.jsonl");
    let run = reference(dir.path(), &out, &[]);
    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("the manifest lists elsewhere"), "{stderr}");
}

#[test]
fn missing_inputs_are_a_run_failure() {
    let dir = tempdir();
    let out = dir.path().join("predictions.jsonl");
    assert!(!reference(dir.path(), &out, &[]).status.success());
}
