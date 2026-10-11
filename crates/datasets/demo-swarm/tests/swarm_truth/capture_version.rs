//! Captures at an older dataset version: a label fix bumps only the
//! converter's version, so a capture written for an older one is exported
//! at the converter's, recording the capture's digest and version so the
//! capture's predictions still score.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{read_manifest, write_manifest};
use a2a_bench_corpus::split::Release;
use a2a_bench_dataset_demo_swarm::capture_view::{self, CaptureViewError};
use a2a_bench_dataset_demo_swarm::{
    CAPTURE_DIGEST_KEY, CAPTURE_VERSION_KEY, CaptureManifestError, Error, Options, VERSION,
    write_export, write_holdout_export,
};
use a2a_bench_format::manifest::{Converter, Manifest, Setting, Split};

use super::fixture::{self, Written};

fn converter() -> Converter {
    Converter {
        version: "0.1.0".to_owned(),
        git: "test".to_owned(),
    }
}

/// The fixture's run, its capture manifest at dataset version `version`.
fn run_at(name: &str, version: u32) -> (Written, PathBuf) {
    let dir = fixture::dir(name);
    let run = dir.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let written = fixture::write(&run, &fixture::truth_rows());
    set_capture(&written.dir, |m| m.dataset_version = version);
    (written, dir)
}

fn set_capture(dir: &Path, change: impl FnOnce(&mut Manifest)) {
    let mut manifest = read_manifest(dir).unwrap();
    change(&mut manifest);
    write_manifest(dir, &manifest).unwrap();
}

fn capture(written: &Written) -> Manifest {
    read_manifest(&written.dir).unwrap()
}

fn export(written: &Written, out: &Path) -> Result<Manifest, Error> {
    write_export(&written.inputs, out, &Options::default(), converter())?;
    Ok(read_manifest(out).unwrap())
}

#[test]
fn a_v1_capture_exports_at_the_converters_version_and_records_the_capture() {
    let (old, dir) = run_at("capture-v1", 1);
    let manifest = export(&old, &dir.join("export")).expect("a v1 capture exports");
    let captured = capture(&old);
    assert_eq!(manifest.dataset_version, VERSION);
    assert_eq!(manifest.split, Split::Dev);
    assert_eq!(
        manifest.selection.get(CAPTURE_DIGEST_KEY),
        Some(&Setting::Text(captured.digest().unwrap().to_hex()))
    );
    assert_eq!(
        manifest.selection.get(CAPTURE_VERSION_KEY),
        Some(&Setting::Int(1))
    );
    // The capture view rebuilt from the export is the capture's manifest.
    assert_eq!(capture_view::capture_view(&manifest).unwrap(), captured);
    assert_eq!(
        capture_view::capture_digest(&manifest).unwrap(),
        Some(captured.digest().unwrap())
    );
    // Its own input view has another digest, which a detector run on it
    // names.
    assert_ne!(manifest.digest().unwrap(), captured.digest().unwrap());

    // The labels, messages and exchanges are those of the same run captured
    // at the converter's version.
    let (current, dir2) = run_at("capture-current", VERSION);
    export(&current, &dir2.join("export")).unwrap();
    for name in ["labels.jsonl", "messages.jsonl", "exchanges.jsonl"] {
        assert_eq!(
            std::fs::read(dir.join("export").join(name)).unwrap(),
            std::fs::read(dir2.join("export").join(name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn a_capture_at_the_converters_version_records_no_capture_view() {
    let (current, dir) = run_at("capture-same", VERSION);
    let manifest = export(&current, &dir.join("export")).unwrap();
    assert!(!manifest.selection.contains_key(CAPTURE_DIGEST_KEY));
    assert!(!manifest.selection.contains_key(CAPTURE_VERSION_KEY));
    assert_eq!(manifest.input_view(), capture(&current));
    assert_eq!(capture_view::capture_digest(&manifest).unwrap(), None);
}

#[test]
fn a_capture_newer_than_the_converter_is_refused() {
    for (name, version) in [("capture-newer", VERSION + 1), ("capture-zero", 0)] {
        let (written, dir) = run_at(name, version);
        let out = dir.join("export");
        let refused = export(&written, &out).expect_err("refused");
        assert!(
            matches!(
                refused,
                Error::CaptureManifest {
                    problem: CaptureManifestError::Version { found },
                    ..
                } if found == version
            ),
            "{refused:?}"
        );
        assert!(!out.join("manifest.json").exists());
    }
}

#[test]
fn a_capture_holding_the_capture_keys_is_refused() {
    for key in [CAPTURE_DIGEST_KEY, CAPTURE_VERSION_KEY] {
        let (written, dir) = run_at(&format!("capture-reserved-{key}"), 1);
        set_capture(&written.dir, |m| {
            m.selection.insert(key.to_owned(), Setting::Int(1));
        });
        let refused = export(&written, &dir.join("export")).expect_err("refused");
        assert!(
            matches!(
                refused,
                Error::CaptureManifest {
                    problem: CaptureManifestError::Reserved(_),
                    ..
                }
            ),
            "{refused:?}"
        );
    }
}

#[test]
fn a_tampered_capture_digest_or_version_is_refused() {
    let (old, dir) = run_at("capture-tampered", 1);
    let manifest = export(&old, &dir.join("export")).unwrap();
    let tampered = |change: &dyn Fn(&mut Manifest)| {
        let mut m = manifest.clone();
        change(&mut m);
        capture_view::capture_digest(&m)
    };
    assert!(matches!(
        tampered(&|m| {
            m.selection
                .insert(CAPTURE_DIGEST_KEY.to_owned(), Setting::Text("0".repeat(64)));
        }),
        Err(CaptureViewError::Mismatch { .. })
    ));
    assert!(matches!(
        tampered(&|m| {
            m.selection.insert(
                CAPTURE_DIGEST_KEY.to_owned(),
                Setting::Text("not a digest".to_owned()),
            );
        }),
        Err(CaptureViewError::Malformed(_))
    ));
    // Another version: it digests to another manifest, or is no older
    // version at all.
    let current = i64::from(VERSION);
    for version in [0, current, current + 1, -1] {
        let result = tampered(&|m| {
            m.selection
                .insert(CAPTURE_VERSION_KEY.to_owned(), Setting::Int(version));
        });
        assert!(
            matches!(result, Err(CaptureViewError::Version { .. })),
            "{version}: {result:?}"
        );
    }
    assert!(matches!(
        tampered(&|m| {
            m.selection.insert(
                CAPTURE_VERSION_KEY.to_owned(),
                Setting::Text("1".to_owned()),
            );
        }),
        Err(CaptureViewError::Malformed(_))
    ));
    // The version removed: the view is at the export's version, not the
    // capture's.
    assert!(matches!(
        tampered(&|m| {
            m.selection.remove(CAPTURE_VERSION_KEY);
        }),
        Err(CaptureViewError::Mismatch { .. })
    ));
    // The digest removed, the version kept.
    assert!(matches!(
        tampered(&|m| {
            m.selection.remove(CAPTURE_DIGEST_KEY);
        }),
        Err(CaptureViewError::Missing)
    ));
    // The export's own version made the capture's: no longer older.
    assert!(matches!(
        tampered(&|m| m.dataset_version = 1),
        Err(CaptureViewError::Version { .. })
    ));
}

#[test]
fn a_v1_holdout_capture_exports_as_a_holdout_at_the_converters_version() {
    let (old, dir) = run_at("capture-v1-holdout", 1);
    let release: Release = "reference@0.1.0".parse().unwrap();
    let out = dir.join("export");
    write_holdout_export(
        &old.inputs,
        &out,
        &Options::default(),
        converter(),
        &release,
    )
    .expect("a v1 holdout exports");
    let manifest = read_manifest(&out).unwrap();
    let captured = capture(&old);
    assert_eq!(manifest.dataset_version, VERSION);
    assert_eq!(manifest.split, Split::Holdout);
    assert_eq!(
        manifest.selection.get("release"),
        Some(&Setting::Text("reference@0.1.0".to_owned()))
    );
    assert_eq!(
        manifest.selection.get(CAPTURE_VERSION_KEY),
        Some(&Setting::Int(1))
    );
    assert_eq!(capture_view::capture_view(&manifest).unwrap(), captured);
    assert_eq!(
        capture_view::capture_digest(&manifest).unwrap(),
        Some(captured.digest().unwrap())
    );
}
