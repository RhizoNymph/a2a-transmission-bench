//! The manifest: its digest is the same for the full manifest and the
//! input view a detector gets.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use a2a_bench_format::ids::{Digest, WorldKey};
use a2a_bench_format::manifest::{
    Converter, FileDigests, Manifest, Setting, Source, Split, WorldEntry,
};
use a2a_bench_format::version::FORMAT;

fn manifest() -> Manifest {
    Manifest {
        format: FORMAT,
        dataset: common::dataset(),
        dataset_version: 1,
        split: Split::Dev,
        source: Source {
            path: "fixture".into(),
            revision: "abc".into(),
            digest: Digest::keyed("t", b"s"),
        },
        converter: Converter {
            version: "0.1.0".into(),
            git: "deadbeef".into(),
        },
        selection: BTreeMap::from([
            (
                "include".to_owned(),
                Setting::List(vec![Setting::Text("a".into()), Setting::Text("b".into())]),
            ),
            ("limit".to_owned(), Setting::Int(53)),
            ("demo".to_owned(), Setting::Bool(false)),
        ]),
        pace: BTreeMap::from([("seed".to_owned(), Setting::Int(0))]),
        worlds: vec![WorldEntry {
            key: WorldKey::new("w1").unwrap(),
            exchanges: 3,
            labels: Some(4),
            notes: BTreeMap::from([("uncarried_control".to_owned(), 1)]),
        }],
        files: FileDigests {
            messages: Digest::keyed("t", b"m"),
            exchanges: Digest::keyed("t", b"e"),
            labels: Some(Digest::keyed("t", b"l")),
        },
    }
}

#[test]
fn input_view_hides_labels_and_keeps_the_digest() {
    let full = manifest();
    let view = full.input_view();
    assert!(view.files.labels.is_none());
    assert!(!serde_json::to_string(&view).unwrap().contains("labels"));
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("uncarried_control")
    );
    assert!(
        view.worlds
            .iter()
            .all(|w| w.labels.is_none() && w.notes.is_empty())
    );
    assert_eq!(full.digest().unwrap(), view.digest().unwrap());
    let mut other = manifest();
    other.dataset_version = 2;
    assert_ne!(other.digest().unwrap(), full.digest().unwrap());
}

#[test]
fn round_trips_and_refuses_unknown_fields() {
    let json = serde_json::to_string(&manifest()).unwrap();
    assert_eq!(serde_json::from_str::<Manifest>(&json).unwrap(), manifest());
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Manifest>(value).is_err());
}
