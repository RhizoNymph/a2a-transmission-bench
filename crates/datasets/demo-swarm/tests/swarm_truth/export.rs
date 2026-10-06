//! The completed export: the capture checked, the four files and the
//! diagnostics report written, and everything read back through the
//! format's checks.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use a2a_bench_corpus::export::read_manifest;
use a2a_bench_dataset_demo_swarm::{
    CaptureError, DIAGNOSTICS_FILE, DemoSwarmSource, Error, HEADLINE, Options, VERSION,
    write_export,
};
use a2a_bench_format::check::{WorldInputs, check_labels};
use a2a_bench_format::exchange::Exchange;
use a2a_bench_format::files::{ExchangeRow, Exchanges, Labels, MessageRow, Messages};
use a2a_bench_format::jsonl::{FileKind, FileReader, WorldSection};
use a2a_bench_format::manifest::{Converter, Setting};
use a2a_bench_format::message::Message;

use super::fixture::{self, Shape};

fn converter() -> Converter {
    Converter {
        version: "0.1.0".to_owned(),
        git: "test".to_owned(),
    }
}

fn sections<K: FileKind>(path: &Path) -> Vec<WorldSection<K>> {
    let file = File::open(path).unwrap();
    let mut reader = FileReader::<K, _>::open(BufReader::new(file)).unwrap();
    let mut out = Vec::new();
    while let Some(section) = reader.next_world().unwrap() {
        out.push(section);
    }
    out
}

fn exported(name: &str, truth: &[serde_json::Value]) -> (fixture::Written, std::path::PathBuf) {
    let dir = fixture::dir(name);
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let written = fixture::write(&run, truth);
    let out = dir.join("export");
    write_export(
        &written.inputs,
        &out,
        &Options::default(),
        converter(),
        "bench-runs/fixture",
    )
    .expect("the export is written");
    (written, out)
}

#[test]
fn the_export_reads_back_through_every_check() {
    let (_, out) = exported("export-roundtrip", &fixture::truth_rows());
    let messages = sections::<Messages>(&out.join("messages.jsonl"));
    let exchanges = sections::<Exchanges>(&out.join("exchanges.jsonl"));
    let labels = sections::<Labels>(&out.join("labels.jsonl"));
    assert_eq!((messages.len(), exchanges.len(), labels.len()), (1, 1, 1));
    let held: Vec<Message> = messages[0]
        .rows
        .iter()
        .map(|MessageRow::Message(message)| message.clone())
        .collect();
    let rows: Vec<Exchange> = exchanges[0]
        .rows
        .iter()
        .map(|ExchangeRow::Exchange(exchange)| exchange.clone())
        .collect();
    let inputs = WorldInputs::new(
        &messages[0].world.key,
        held,
        exchanges[0].world.clone(),
        rows,
    )
    .expect("the inputs check");
    check_labels(&inputs, &labels[0].rows).expect("the labels check");
    let agents: Vec<&str> = exchanges[0]
        .world
        .agents
        .iter()
        .map(|agent| agent.key.as_str())
        .collect();
    assert_eq!(agents, ["a001", "a002", "a003"]);

    let manifest = read_manifest(&out).unwrap();
    assert_eq!(manifest.dataset.as_str(), HEADLINE);
    assert_eq!(manifest.dataset_version, VERSION);
    assert_eq!(manifest.source.revision, "01J00000000000000000000000");
    assert_eq!(manifest.source.path, "bench-runs/fixture");
    assert_eq!(
        manifest.selection.get("run_lead_ms"),
        Some(&Setting::Int(5_000))
    );
    assert_eq!(
        manifest.selection.get("run_slack_ms"),
        Some(&Setting::Int(60_000))
    );
    assert!(manifest.pace.is_empty());
    assert_eq!(manifest.worlds.len(), 1);
    assert_eq!(manifest.worlds[0].exchanges, 11);
}

#[test]
fn the_diagnostics_report_is_written_beside_the_labels() {
    let (_, out) = exported("export-diagnostics", &fixture::truth_rows());
    let text = std::fs::read_to_string(out.join(DIAGNOSTICS_FILE)).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["resolved"]["transmissions"], 3);
    assert_eq!(report["resolved"]["dropped"], 1);
    assert_eq!(report["capture"]["exchanges"], 11);
    assert_eq!(report["window"]["start_unix_ms"], fixture::START_MS - 5_000);
    let table = report["table"].as_array().unwrap();
    assert!(table.iter().any(|row| row["failure"] == "hash_mismatch"
        && row["row"] == "transmission"
        && row["side"] == "reader"
        && row["effect"] == "dropped"
        && row["count"] == 1));
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 3);
    // The truth's excerpts are never copied anywhere.
    for name in ["labels.jsonl", "manifest.json", DIAGNOSTICS_FILE] {
        let text = std::fs::read_to_string(out.join(name)).unwrap();
        assert!(!text.contains("excerpt"), "{name}");
    }
}

#[test]
fn a_rerun_is_byte_identical() {
    let (_, first) = exported("export-rerun-a", &fixture::truth_rows());
    let (_, second) = exported("export-rerun-b", &fixture::truth_rows());
    for name in [
        "messages.jsonl",
        "exchanges.jsonl",
        "labels.jsonl",
        DIAGNOSTICS_FILE,
    ] {
        assert_eq!(
            std::fs::read(first.join(name)).unwrap(),
            std::fs::read(second.join(name)).unwrap(),
            "{name}"
        );
    }
    let (a, b) = (
        read_manifest(&first).unwrap(),
        read_manifest(&second).unwrap(),
    );
    assert_eq!(a.files, b.files);
    assert_eq!(a.source.digest, b.source.digest);
}

#[test]
fn a_capture_of_only_the_run_keeps_its_messages_as_they_are() {
    let (written, out) = exported("export-messages", &fixture::truth_rows());
    assert_eq!(
        std::fs::read(written.dir.join("messages.jsonl")).unwrap(),
        std::fs::read(out.join("messages.jsonl")).unwrap()
    );
    let exchanges = sections::<Exchanges>(&out.join("exchanges.jsonl"));
    let ids: Vec<_> = exchanges[0]
        .rows
        .iter()
        .map(|ExchangeRow::Exchange(exchange)| exchange.clone())
        .collect();
    assert_eq!(ids, written.exchanges, "exchanges carried verbatim");
}

#[test]
fn a_capture_of_another_dataset_or_world_is_refused() {
    let dir = fixture::dir("capture-dataset");
    let written = fixture::write_as(
        &dir,
        &fixture::truth_rows(),
        Shape::default(),
        "demo-swarm/boilerplate",
        fixture::WORLD,
    );
    assert!(matches!(
        DemoSwarmSource::open(&written.inputs, &Options::default()),
        Err(Error::Capture(CaptureError::Dataset { .. }))
    ));
    let dir = fixture::dir("capture-world");
    let written = fixture::write_as(
        &dir,
        &fixture::truth_rows(),
        Shape::default(),
        HEADLINE,
        "swarm-other",
    );
    assert!(matches!(
        DemoSwarmSource::open(&written.inputs, &Options::default()),
        Err(Error::Capture(CaptureError::World { .. }))
    ));
}

#[test]
fn a_truncated_capture_is_refused() {
    let dir = fixture::dir("capture-truncated");
    let written = fixture::write(&dir, &fixture::truth_rows());
    let path = dir.join("exchanges.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let cut: String = text
        .lines()
        .filter(|line| !line.starts_with(r#"{"kind":"trailer""#))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&path, cut).unwrap();
    assert!(matches!(
        DemoSwarmSource::open(&written.inputs, &Options::default()),
        Err(Error::Capture(CaptureError::Read { .. }))
    ));
}

#[test]
fn the_source_digest_covers_the_truth_and_the_capture() {
    let dir = fixture::dir("capture-digest");
    let written = fixture::write(&dir, &fixture::truth_rows());
    let source = DemoSwarmSource::open(&written.inputs, &Options::default()).unwrap();
    assert_eq!(source.files_read().len(), 3);
    let before = source.files_read().digest(&dir).unwrap();
    let mut rows = fixture::truth_rows();
    rows.pop();
    fixture::write_jsonl(&dir.join("truth.jsonl"), &rows);
    let after = source.files_read().digest(&dir).unwrap();
    assert_ne!(before, after);
}
