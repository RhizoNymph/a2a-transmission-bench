//! The source digest: pinned bytes, path order, refusals, and agreement
//! with the format's `SourceDigest`.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

use a2a_bench_corpus::export::{DigestError, FilesRead, SOURCE_DIGEST_CONTEXT, source_digest};
use a2a_bench_format::source::SourceDigest;
use common::ok;

/// The digest of `pinned_tree`, computed by corpus's own implementation
/// before it moved onto the format's `SourceDigest`.
const PINNED: &str = "da05001faed5c294a06fafb525c667e8184b178eb6dc47ef58a0d691e8271d02";

fn pinned_tree(root: &Path) -> Vec<PathBuf> {
    let files: [(&str, &[u8]); 4] = [
        ("b.jsonl", b"{\"x\":1}\n"),
        ("a/z.txt", b"zzz"),
        ("a/empty", b""),
        ("a b/\u{e9}.json", b"[]"),
    ];
    let mut paths = Vec::new();
    for (name, contents) in files {
        let path = root.join(name);
        ok(std::fs::create_dir_all(path.parent().unwrap()));
        ok(std::fs::write(&path, contents));
        paths.push(PathBuf::from(name));
    }
    paths
}

#[test]
fn the_digest_is_pinned() {
    let dir = ok(tempfile::tempdir());
    let files = pinned_tree(dir.path());
    let digest = ok(source_digest(dir.path(), files));
    assert_eq!(digest.to_hex(), PINNED);
}

#[test]
fn the_digest_is_the_definition() {
    let dir = ok(tempfile::tempdir());
    let files = pinned_tree(dir.path());
    let mut sorted: Vec<String> = files
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    sorted.sort();
    let mut hasher = blake3::Hasher::new_derive_key(SOURCE_DIGEST_CONTEXT);
    for name in &sorted {
        let contents = ok(std::fs::read(dir.path().join(name)));
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        hasher.update(&(contents.len() as u64).to_le_bytes());
        hasher.update(&contents);
    }
    let digest = ok(source_digest(dir.path(), files));
    assert_eq!(digest.as_bytes(), hasher.finalize().as_bytes());
}

#[test]
fn the_digest_is_the_formats() {
    let dir = ok(tempfile::tempdir());
    let files = pinned_tree(dir.path());
    let mut sorted: Vec<String> = files
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    sorted.sort();
    let mut format = SourceDigest::new();
    for name in &sorted {
        let contents = ok(std::fs::read(dir.path().join(name)));
        let mut file = ok(format.file(name, contents.len() as u64));
        file.update(&contents);
        ok(file.end());
    }
    let digest = ok(source_digest(dir.path(), files));
    assert_eq!(digest, format.finish());
}

#[test]
fn files_read_dedups_and_takes_absolute_paths() {
    let dir = ok(tempfile::tempdir());
    let files = pinned_tree(dir.path());
    let mut read = FilesRead::new();
    for path in files.iter().rev() {
        read.record(dir.path().join(path));
        read.record(path.clone());
    }
    read.record(Path::new("./b.jsonl"));
    // An absolute and a relative spelling are two records, one file.
    assert_eq!(read.len(), files.len() * 2 + 1);
    assert_eq!(ok(read.digest(dir.path())).to_hex(), PINNED);
}

#[test]
fn bad_paths_are_refused() {
    let dir = ok(tempfile::tempdir());
    pinned_tree(dir.path());
    let outside = source_digest(dir.path(), [PathBuf::from("/elsewhere/x")]);
    assert!(matches!(outside, Err(DigestError::OutsideRoot { .. })));
    let up = source_digest(dir.path(), [PathBuf::from("a/../b.jsonl")]);
    assert!(matches!(up, Err(DigestError::BadPath(_))));
    let missing = source_digest(dir.path(), [PathBuf::from("nope")]);
    assert!(matches!(missing, Err(DigestError::Io { .. })));
}
