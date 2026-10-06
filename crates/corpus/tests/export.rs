//! The export writer: files in lockstep, the manifest, determinism,
//! failures, refusals, and the detector's input view.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use a2a_bench_corpus::export::{
    EXCHANGES_FILE, ExportError, LABELS_FILE, MANIFEST_FILE, MESSAGES_FILE, export, input_view,
    read_manifest,
};
use a2a_bench_corpus::source::InMemory;
use a2a_bench_corpus::split::Selection;
use a2a_bench_format::check::{WorldInputs, check_labels};
use a2a_bench_format::files::{ExchangeRow, Exchanges, Labels, MessageRow, Messages};
use a2a_bench_format::ids::DatasetId;
use a2a_bench_format::jsonl::{FileKind, FileReader};
use a2a_bench_format::manifest::{Setting, Split};
use a2a_bench_format::version::FORMAT;
use common::{dataset, info, ok, sample_world, three_worlds};

fn reader<K: FileKind>(dir: &Path, name: &str) -> FileReader<K, BufReader<File>> {
    ok(FileReader::<K, _>::open(BufReader::new(ok(File::open(
        dir.join(name),
    )))))
}

fn bytes(dir: &Path, name: &str) -> Vec<u8> {
    ok(std::fs::read(dir.join(name)))
}

#[derive(Debug)]
struct Failed(&'static str);

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Failed {}

#[test]
fn an_export_reads_back_world_by_world_and_passes_the_checks() {
    let worlds = three_worlds();
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("x");
    let mut source = InMemory::new(dataset(), worlds.clone());
    let exported = ok(export(&mut source, &out, info("r1"), &Selection::Unsplit));
    assert!(exported.failures.is_empty());

    let mut messages = reader::<Messages>(&out, MESSAGES_FILE);
    let mut exchanges = reader::<Exchanges>(&out, EXCHANGES_FILE);
    let mut labels = reader::<Labels>(&out, LABELS_FILE);
    for world in &worlds {
        let (Some(m), Some(e), Some(l)) = (
            ok(messages.next_world()),
            ok(exchanges.next_world()),
            ok(labels.next_world()),
        ) else {
            panic!("a file ended early")
        };
        assert_eq!(&m.world.key, world.key());
        assert_eq!(&e.world, world.decl());
        assert_eq!(l.world.key, *world.key());
        let msgs: Vec<_> = m.rows.into_iter().map(|MessageRow::Message(x)| x).collect();
        // First-use order, each once.
        assert_eq!(
            msgs.iter().map(|x| x.id()).collect::<Vec<_>>(),
            world
                .messages_in_order()
                .iter()
                .map(|x| x.id())
                .collect::<Vec<_>>()
        );
        let exs: Vec<_> = e
            .rows
            .into_iter()
            .map(|ExchangeRow::Exchange(x)| x)
            .collect();
        assert_eq!(exs, world.exchanges());
        let inputs = ok(WorldInputs::new(&m.world.key, msgs, e.world, exs));
        ok(check_labels(&inputs, &l.rows));
        assert_eq!(l.rows, world.labels());
    }
    assert!(ok(messages.next_world()).is_none());
    assert!(ok(exchanges.next_world()).is_none());
    assert!(ok(labels.next_world()).is_none());
}

#[test]
fn the_manifest_records_digests_counts_and_settings() {
    let worlds = three_worlds();
    let counts: Vec<u64> = worlds.iter().map(|w| w.exchanges().len() as u64).collect();
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("x");
    let mut source = InMemory::new(dataset(), worlds);
    let exported = ok(export(&mut source, &out, info("r1"), &Selection::Unsplit));
    let manifest = ok(read_manifest(&out));
    assert_eq!(manifest, exported.manifest);
    assert_eq!(manifest.format, FORMAT);
    assert_eq!(manifest.dataset, dataset());
    assert_eq!(manifest.dataset_version, 1);
    assert_eq!(manifest.split, Split::Dev);
    assert_eq!(manifest.source, info("r1").source);
    assert_eq!(manifest.converter, info("r1").converter);
    assert_eq!(manifest.pace, info("r1").pace);
    assert_eq!(manifest.selection.get("limit"), Some(&Setting::Int(3)));
    assert_eq!(
        manifest.selection.get("split_list"),
        Some(&Setting::Text("none".into()))
    );
    assert_eq!(
        manifest
            .worlds
            .iter()
            .map(|w| w.exchanges)
            .collect::<Vec<_>>(),
        counts
    );
    // The digests are the trailers'.
    let trailer = |name: &str| {
        let text = String::from_utf8(bytes(&out, name)).unwrap_or_default();
        let last = text.lines().last().unwrap_or_default().to_owned();
        let value: serde_json::Value = ok(serde_json::from_str(&last));
        value["digest"].as_str().unwrap_or_default().to_owned()
    };
    assert_eq!(manifest.files.messages.to_hex(), trailer(MESSAGES_FILE));
    assert_eq!(manifest.files.exchanges.to_hex(), trailer(EXCHANGES_FILE));
    assert_eq!(
        manifest.files.labels.map(|d| d.to_hex()),
        Some(trailer(LABELS_FILE))
    );
    // Pretty JSON with a final newline.
    assert!(bytes(&out, MANIFEST_FILE).ends_with(b"}\n"));
}

#[test]
fn two_exports_of_one_source_are_byte_identical() {
    let dir = ok(tempfile::tempdir());
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    ok(export(
        &mut InMemory::new(dataset(), three_worlds()),
        &a,
        info("r1"),
        &Selection::Unsplit,
    ));
    ok(export(
        &mut InMemory::new(dataset(), three_worlds()),
        &b,
        info("r1"),
        &Selection::Unsplit,
    ));
    for name in [MANIFEST_FILE, MESSAGES_FILE, EXCHANGES_FILE, LABELS_FILE] {
        assert_eq!(bytes(&a, name), bytes(&b, name), "{name} differs");
    }
    // A different world changes the bytes and the digests.
    let c = dir.path().join("c");
    let mut changed = three_worlds();
    changed[1] = sample_world("w2", "another secret");
    ok(export(
        &mut InMemory::new(dataset(), changed),
        &c,
        info("r1"),
        &Selection::Unsplit,
    ));
    assert_ne!(bytes(&a, MESSAGES_FILE), bytes(&c, MESSAGES_FILE));
    assert_ne!(
        ok(read_manifest(&a)).files.messages,
        ok(read_manifest(&c)).files.messages
    );
}

#[test]
fn a_failed_world_is_recorded_and_the_export_goes_on() {
    let mut worlds = three_worlds().into_iter();
    let items: Vec<Result<_, Failed>> = vec![
        worlds.next().ok_or(Failed("w1")),
        Err(Failed("broken record")),
        worlds.next().ok_or(Failed("w2")),
    ];
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("x");
    let mut source = InMemory::with_failures(dataset(), items);
    let exported = ok(export(&mut source, &out, info("r1"), &Selection::Unsplit));
    assert_eq!(exported.failures.len(), 1);
    assert_eq!(exported.failures[0].position, 1);
    assert_eq!(exported.failures[0].error.0, "broken record");
    let keys: Vec<_> = exported
        .manifest
        .worlds
        .iter()
        .map(|w| w.key.as_str().to_owned())
        .collect();
    assert_eq!(keys, vec!["w1", "w2"]);
}

#[test]
fn exports_refuse_a_used_directory_another_dataset_and_reserved_keys() {
    let dir = ok(tempfile::tempdir());
    let used = dir.path().join("used");
    ok(std::fs::create_dir_all(&used));
    ok(std::fs::write(used.join("stray"), b"x"));
    assert!(matches!(
        export(
            &mut InMemory::new(dataset(), three_worlds()),
            &used,
            info("r1"),
            &Selection::Unsplit
        ),
        Err(ExportError::NotEmpty(_))
    ));

    let other = ok(DatasetId::new("other"));
    assert!(matches!(
        export(
            &mut InMemory::new(other, vec![]),
            &dir.path().join("o"),
            info("r1"),
            &Selection::Unsplit
        ),
        Err(ExportError::DatasetMismatch { .. })
    ));

    let mut reserved = info("r1");
    reserved
        .selection
        .insert("split_list".into(), Setting::Text("x".into()));
    assert!(matches!(
        export(
            &mut InMemory::new(dataset(), vec![]),
            &dir.path().join("r"),
            reserved,
            &Selection::Unsplit
        ),
        Err(ExportError::ReservedSelectionKey(_))
    ));
}

#[test]
fn the_input_view_never_holds_labels() {
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("export");
    let view = dir.path().join("view");
    let exported = ok(export(
        &mut InMemory::new(dataset(), three_worlds()),
        &out,
        info("r1"),
        &Selection::Unsplit,
    ));
    let manifest = ok(input_view(&out, &view));
    assert_eq!(manifest, exported.manifest.input_view());
    assert!(manifest.files.labels.is_none());
    assert_eq!(ok(read_manifest(&view)), manifest);
    let mut names: Vec<_> = ok(std::fs::read_dir(&view))
        .map(|entry| ok(entry).file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec![EXCHANGES_FILE, MANIFEST_FILE, MESSAGES_FILE]);
    assert!(!view.join(LABELS_FILE).exists());
    let manifest_text = String::from_utf8(bytes(&view, MANIFEST_FILE)).unwrap_or_default();
    assert!(!manifest_text.contains("labels"));
    for name in [MESSAGES_FILE, EXCHANGES_FILE] {
        assert_eq!(bytes(&out, name), bytes(&view, name));
    }
    // The view's manifest digest is the export's: predictions name either.
    assert_eq!(ok(manifest.digest()), ok(exported.manifest.digest()));
    // A used destination and a directory without an export are refused.
    assert!(matches!(
        input_view(&out, &view),
        Err(ExportError::NotEmpty(_))
    ));
    assert!(matches!(
        input_view(&dir.path().join("nothing"), &dir.path().join("v2")),
        Err(ExportError::NotAnExport(_))
    ));
}
