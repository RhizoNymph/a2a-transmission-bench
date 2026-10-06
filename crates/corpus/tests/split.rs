//! Splits: dev lists, the selection logic, and dev and holdout exports.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use a2a_bench_corpus::export::{ExportError, export};
use a2a_bench_corpus::source::{InMemory, WorldFilter};
use a2a_bench_corpus::split::{
    DevList, Release, Selection, SplitError, dev_list_path, enclosing_repository,
};
use a2a_bench_format::manifest::{Setting, Split};
use common::{dataset, info, ok, three_worlds, world_key};

fn write_list(splits: &Path, revision: &str, worlds: &[&str]) {
    let names: Vec<String> = worlds.iter().map(|w| format!("{w:?}")).collect();
    let text = format!(
        "dataset = \"synthetic\"\nversion = 1\nrevision = \"{revision}\"\nworlds = [{}]\n",
        names.join(", ")
    );
    ok(std::fs::create_dir_all(splits));
    ok(std::fs::write(dev_list_path(splits, &dataset(), 1), text));
}

fn keys(names: &[&str]) -> BTreeSet<a2a_bench_format::ids::WorldKey> {
    names.iter().map(|name| world_key(name)).collect()
}

fn exported_keys(manifest: &a2a_bench_format::manifest::Manifest) -> Vec<String> {
    manifest
        .worlds
        .iter()
        .map(|w| w.key.as_str().to_owned())
        .collect()
}

#[test]
fn dev_lists_parse_and_check_their_name() {
    let path = Path::new("splits/synthetic@1.dev.toml");
    let good = br#"dataset = "synthetic"
version = 1
revision = "abc"
worlds = ["w1", "w3"]
"#;
    let list = ok(DevList::parse(good, path, &dataset(), 1));
    assert_eq!(list.revision(), "abc");
    assert_eq!(list.worlds(), &keys(&["w1", "w3"]));
    assert_eq!(list.file_name(), "synthetic@1.dev.toml");
    assert_eq!(list.digest().as_bytes(), blake3::hash(good).as_bytes());

    assert!(matches!(
        DevList::parse(good, path, &dataset(), 2),
        Err(SplitError::Mismatch { .. })
    ));
    let twice = br#"dataset = "synthetic"
version = 1
revision = "abc"
worlds = ["w1", "w1"]
"#;
    assert!(matches!(
        DevList::parse(twice, path, &dataset(), 1),
        Err(SplitError::DuplicateWorld { .. })
    ));
    let extra = br#"dataset = "synthetic"
version = 1
revision = "abc"
worlds = []
holdout = ["w2"]
"#;
    assert!(matches!(
        DevList::parse(extra, path, &dataset(), 1),
        Err(SplitError::Parse { .. })
    ));
    let empty_key = br#"dataset = "synthetic"
version = 1
revision = "abc"
worlds = [""]
"#;
    assert!(matches!(
        DevList::parse(empty_key, path, &dataset(), 1),
        Err(SplitError::Key { .. })
    ));
}

#[test]
fn a_dataset_without_a_list_is_unsplit_and_has_no_holdout() {
    let dir = ok(tempfile::tempdir());
    let splits = dir.path().join("splits");
    let selection = ok(Selection::dev(&splits, &dataset(), 1));
    assert_eq!(selection, Selection::Unsplit);
    assert_eq!(selection.split(), Split::Dev);
    assert_eq!(selection.filter(), WorldFilter::All);
    assert!(matches!(
        Selection::holdout(
            &splits,
            &dataset(),
            1,
            ok(Release::new("crosstalk-live", "v1"))
        ),
        Err(SplitError::NoHoldout { .. })
    ));
}

#[test]
fn dev_keeps_the_list_and_holdout_its_complement() {
    let dir = ok(tempfile::tempdir());
    let splits = dir.path().join("splits");
    write_list(&splits, "r1", &["w1", "w3"]);
    let dev = ok(Selection::dev(&splits, &dataset(), 1));
    let release = ok("crosstalk-live@v1.2".parse::<Release>());
    let holdout = ok(Selection::holdout(&splits, &dataset(), 1, release));
    assert_eq!(dev.split(), Split::Dev);
    assert_eq!(holdout.split(), Split::Holdout);
    for (name, dev_keeps) in [("w1", true), ("w2", false), ("w3", true), ("w9", false)] {
        let key = world_key(name);
        assert_eq!(dev.filter().keeps(&key), dev_keeps, "{name}");
        assert_eq!(holdout.filter().keeps(&key), !dev_keeps, "{name}");
    }
    let settings = holdout.settings();
    assert_eq!(
        settings.get("split_list"),
        Some(&Setting::Text("synthetic@1.dev.toml".into()))
    );
    assert!(settings.contains_key("split_digest"));
    assert_eq!(
        settings.get("release"),
        Some(&Setting::Text("crosstalk-live@v1.2".into()))
    );
    assert!(!dev.settings().contains_key("release"));
}

#[test]
fn releases_are_detector_at_tag() {
    let release = ok("reference@2026-10-05".parse::<Release>());
    assert_eq!(release.detector(), "reference");
    assert_eq!(release.tag(), "2026-10-05");
    assert_eq!(release.to_string(), "reference@2026-10-05");
    assert_eq!(
        release.default_dir(Path::new("/home/u")),
        Path::new("/home/u/.local/share/a2a-bench/releases/reference/2026-10-05")
    );
    for bad in [
        "reference",
        "@v1",
        "reference@",
        "a@b@c",
        "a/b@v1",
        "a@..",
        "a@v\n1",
    ] {
        assert!(bad.parse::<Release>().is_err(), "{bad:?}");
    }
}

#[test]
fn a_dev_export_holds_only_listed_worlds() {
    let dir = ok(tempfile::tempdir());
    let splits = dir.path().join("splits");
    write_list(&splits, "r1", &["w1", "w3", "w7"]);
    let selection = ok(Selection::dev(&splits, &dataset(), 1));
    let out = dir.path().join("dev");
    let exported = ok(export(
        &mut InMemory::new(dataset(), three_worlds()),
        &out,
        info("r1"),
        &selection,
    ));
    assert_eq!(exported_keys(&exported.manifest), vec!["w1", "w3"]);
    assert_eq!(exported.left_out, 1);
    assert_eq!(exported.missing, vec![world_key("w7")]);
    assert_eq!(exported.manifest.split, Split::Dev);
    assert_eq!(
        exported.manifest.selection.get("split_list"),
        Some(&Setting::Text("synthetic@1.dev.toml".into()))
    );
}

#[test]
fn an_unsplit_dev_export_holds_every_world_and_says_so() {
    let dir = ok(tempfile::tempdir());
    let out = dir.path().join("dev");
    let exported = ok(export(
        &mut InMemory::new(dataset(), three_worlds()),
        &out,
        info("r1"),
        &Selection::Unsplit,
    ));
    assert_eq!(exported_keys(&exported.manifest), vec!["w1", "w2", "w3"]);
    assert_eq!(
        exported.manifest.selection.get("split_list"),
        Some(&Setting::Text("none".into()))
    );
    assert!(!exported.manifest.selection.contains_key("split_digest"));
}

#[test]
fn a_holdout_export_holds_the_complement_outside_any_repository() {
    // TMPDIR may itself sit inside a repository (a worktree's target/),
    // and a sandbox may refuse writes elsewhere.
    let candidates = [
        std::env::temp_dir(),
        std::path::PathBuf::from("/tmp/claude-1000"),
        std::path::PathBuf::from("/tmp"),
    ];
    let Some(dir) = candidates.iter().find_map(|base| {
        tempfile::tempdir_in(base)
            .ok()
            .filter(|dir| matches!(enclosing_repository(dir.path()), Ok(None)))
    }) else {
        eprintln!("no writable temporary directory outside a repository; skipped");
        return;
    };
    assert_eq!(ok(enclosing_repository(dir.path())), None);
    let splits = dir.path().join("splits");
    write_list(&splits, "r1", &["w1", "w3"]);
    let release = ok(Release::new("reference", "v1"));
    let selection = ok(Selection::holdout(&splits, &dataset(), 1, release));
    let out = dir.path().join("releases/reference/v1");
    let exported = ok(export(
        &mut InMemory::new(dataset(), three_worlds()),
        &out,
        info("r1"),
        &selection,
    ));
    assert_eq!(exported_keys(&exported.manifest), vec!["w2"]);
    assert_eq!(exported.manifest.split, Split::Holdout);
    assert!(exported.missing.is_empty());
}

#[test]
fn a_holdout_export_inside_a_repository_is_refused() {
    let dir = ok(tempfile::tempdir());
    let repo = dir.path().join("repo");
    ok(std::fs::create_dir_all(repo.join(".git")));
    // A worktree's .git is a file; either counts.
    let worktree = dir.path().join("worktree");
    ok(std::fs::create_dir_all(&worktree));
    ok(std::fs::write(
        worktree.join(".git"),
        b"gitdir: elsewhere\n",
    ));
    let splits = dir.path().join("splits");
    write_list(&splits, "r1", &["w1"]);
    for root in [&repo, &worktree] {
        let out = root.join("deep/not/yet/made");
        assert_eq!(
            ok(enclosing_repository(&out)).as_deref(),
            Some(root.as_path())
        );
        let release = ok(Release::new("reference", "v1"));
        let selection = ok(Selection::holdout(&splits, &dataset(), 1, release));
        assert!(matches!(
            export(
                &mut InMemory::new(dataset(), three_worlds()),
                &out,
                info("r1"),
                &selection
            ),
            Err(ExportError::Split(SplitError::InsideRepository { .. }))
        ));
        assert!(!out.exists(), "nothing is written");
        // Dev exports may go anywhere.
        let dev = ok(Selection::dev(&splits, &dataset(), 1));
        assert!(
            export(
                &mut InMemory::new(dataset(), three_worlds()),
                &root.join("dev"),
                info("r1"),
                &dev
            )
            .is_ok()
        );
    }
}

#[test]
fn a_list_applies_only_to_its_source_revision() {
    let dir = ok(tempfile::tempdir());
    let splits = dir.path().join("splits");
    write_list(&splits, "r1", &["w1"]);
    let selection = ok(Selection::dev(&splits, &dataset(), 1));
    assert!(matches!(
        export(
            &mut InMemory::new(dataset(), three_worlds()),
            &dir.path().join("x"),
            info("r2"),
            &selection
        ),
        Err(ExportError::Split(SplitError::RevisionMismatch { .. }))
    ));
}
