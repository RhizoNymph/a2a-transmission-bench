//! Revision detection and pins, the holdout commitment, dataset flags.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;

use a2a_bench_cli::datasets::{DatasetFlags, DatasetName};
use a2a_bench_cli::holdout::{self, Committed, HoldoutError};
use a2a_bench_cli::revision::{Pin, Revision, RevisionError, check_pin, source_revision};
use a2a_bench_format::ids::{DatasetId, Digest, WorldKey};
use a2a_bench_format::manifest::{
    Converter, FileDigests, Manifest, Setting, Source, Split, WorldEntry,
};
use a2a_bench_format::version::FORMAT;

#[test]
fn an_hf_snapshot_is_named_by_its_hash_through_the_symlink() {
    let dir = common::scratch();
    let snapshot = dir
        .path()
        .join("hub/datasets--org--name/snapshots/838b4150303ca8228e8edb432d8b8ccae353d258");
    std::fs::create_dir_all(&snapshot).unwrap();
    let link = dir.path().join("dataset");
    std::os::unix::fs::symlink(&snapshot, &link).unwrap();
    assert_eq!(
        source_revision(&link, None).unwrap(),
        Revision::HfSnapshot("838b4150303ca8228e8edb432d8b8ccae353d258".into())
    );
}

#[test]
fn a_git_clone_is_named_by_its_head_unless_it_encloses_the_data_root() {
    let fixture = common::fixture("salt/tests/fixtures/salt");
    match source_revision(&fixture, None).unwrap() {
        Revision::Git(head) => assert_eq!(head.len(), 40),
        other => panic!("{other:?}"),
    }
    // The bench checkout holds the data root here: not the dataset's clone.
    let root = common::fixture("");
    assert_eq!(
        source_revision(&fixture, Some(&root)).unwrap(),
        Revision::Unversioned
    );
}

#[test]
fn a_plain_directory_is_unversioned() {
    let Some(dir) = common::outside_repository() else {
        return;
    };
    assert_eq!(
        source_revision(dir.path(), None).unwrap().as_str(),
        "unversioned"
    );
    assert!(matches!(
        source_revision(&dir.path().join("missing"), None),
        Err(RevisionError::Resolve { .. })
    ));
}

#[test]
fn pins_refuse_a_different_revision_unless_allowed() {
    assert_eq!(check_pin(None, "abc", false).unwrap(), Pin::Unpinned);
    assert_eq!(check_pin(Some("abc"), "abc", false).unwrap(), Pin::Matches);
    assert!(matches!(
        check_pin(Some("abc"), "def", false),
        Err(RevisionError::Mismatch { .. })
    ));
    assert_eq!(
        check_pin(Some("abc"), "def", true).unwrap(),
        Pin::Overridden {
            pinned: "abc".into()
        }
    );
}

fn holdout_manifest(worlds: &[&str], labels: u8) -> Manifest {
    Manifest {
        format: FORMAT,
        dataset: DatasetId::new("salt").unwrap(),
        dataset_version: 1,
        split: Split::Holdout,
        source: Source {
            path: "salt-nlp".into(),
            revision: "r".into(),
            digest: Digest::from_bytes([0; 32]),
        },
        converter: Converter {
            version: "0".into(),
            git: "0".into(),
        },
        selection: BTreeMap::from([(
            "release".to_owned(),
            Setting::Text("reference@0.1.0".into()),
        )]),
        pace: BTreeMap::new(),
        worlds: worlds
            .iter()
            .map(|key| WorldEntry {
                key: WorldKey::new(*key).unwrap(),
                exchanges: 1,
                labels: Some(1),
                notes: BTreeMap::new(),
            })
            .collect(),
        files: FileDigests {
            messages: Digest::from_bytes([1; 32]),
            exchanges: Digest::from_bytes([2; 32]),
            labels: Some(Digest::from_bytes([labels; 32])),
        },
    }
}

#[test]
fn the_commitment_covers_the_world_list_and_the_labels_digest() {
    let base = holdout::commitment(&holdout_manifest(&["a", "b"], 3)).unwrap();
    assert_eq!(
        base,
        holdout::commitment(&holdout_manifest(&["a", "b"], 3)).unwrap()
    );
    assert_ne!(
        base,
        holdout::commitment(&holdout_manifest(&["a", "c"], 3)).unwrap()
    );
    assert_ne!(
        base,
        holdout::commitment(&holdout_manifest(&["a", "b"], 4)).unwrap()
    );
    // The key boundary is part of it.
    assert_ne!(
        holdout::commitment(&holdout_manifest(&["ab"], 3)).unwrap(),
        holdout::commitment(&holdout_manifest(&["a", "b"], 3)).unwrap()
    );
    let mut dev = holdout_manifest(&["a"], 3);
    dev.split = Split::Dev;
    assert!(matches!(
        holdout::commitment(&dev),
        Err(HoldoutError::NotHoldout)
    ));
}

#[test]
fn the_commit_file_is_recorded_once_then_checked() {
    let dir = common::scratch();
    let path = holdout::commit_path(dir.path(), &DatasetId::new("salt").unwrap(), 2);
    assert!(path.ends_with("salt@2.holdout.commit"));
    let commitment = holdout::commitment(&holdout_manifest(&["a"], 3)).unwrap();
    assert!(matches!(
        holdout::record_or_check(&path, commitment).unwrap(),
        Committed::Recorded { .. }
    ));
    assert!(matches!(
        holdout::record_or_check(&path, commitment).unwrap(),
        Committed::Matched { .. }
    ));
    let other = holdout::commitment(&holdout_manifest(&["b"], 3)).unwrap();
    assert!(matches!(
        holdout::record_or_check(&path, other),
        Err(HoldoutError::Mismatch { .. })
    ));
    std::fs::write(&path, "not a digest\n").unwrap();
    assert!(matches!(
        holdout::record_or_check(&path, commitment),
        Err(HoldoutError::Malformed { .. })
    ));
}

#[test]
fn flags_default_to_ct_evals_and_refuse_what_a_dataset_does_not_read() {
    let flags = DatasetFlags::default();
    let pace = flags.pace().unwrap();
    assert_eq!(pace.settings()["min_ms"], Setting::Int(1000));
    assert_eq!(pace.settings()["max_ms"], Setting::Int(5000));
    assert_eq!(pace.settings()["seed"], Setting::Int(0));
    let village = flags.ai_village().unwrap();
    assert_eq!(village.from.to_string(), "2026-07-13");
    assert_eq!(village.to.to_string(), "2026-07-17");

    let seeded = DatasetFlags {
        corpus_seed: Some(3),
        ..DatasetFlags::default()
    };
    assert!(seeded.check(DatasetName::Cipher).is_ok());
    assert!(seeded.check(DatasetName::Salt).is_ok());
    assert!(seeded.check(DatasetName::Tau2).is_err());
    assert!(seeded.check(DatasetName::Lmcache).is_err());
    let demo = DatasetFlags {
        demo: true,
        ..DatasetFlags::default()
    };
    assert!(demo.check(DatasetName::CollusionWiki).is_ok());
    assert!(demo.check(DatasetName::Salt).is_err());
    let bad_pace = DatasetFlags {
        pace_min_ms: Some(10),
        ..DatasetFlags::default()
    };
    assert!(bad_pace.pace().is_err());
}

#[test]
fn dataset_names_are_the_bench_ids() {
    for (name, id, paced) in [
        (DatasetName::Salt, "salt", true),
        (DatasetName::CollusionWiki, "collusion-wiki", true),
        (DatasetName::SwarmTraces, "swarm-traces", true),
        (DatasetName::OpenSwe, "open_swe", true),
        (DatasetName::SweSplice, "swe_splice", true),
        (DatasetName::Tau2, "tau2", false),
        (DatasetName::Lmcache, "lmcache", false),
        (DatasetName::AiVillage, "ai-village", false),
    ] {
        assert_eq!(name.id(), id);
        assert_eq!(name.paced(), paced);
        assert_eq!(name.version(), 1);
    }
}
