//! The source digest: order, paths and lengths are part of it.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use a2a_bench_format::source::{SourceDigest, SourceDigestError};

fn digest(files: &[(&str, &[u8])]) -> Result<String, SourceDigestError> {
    let mut digest = SourceDigest::new();
    for (path, bytes) in files {
        let mut file = digest.file(path, u64::try_from(bytes.len()).unwrap())?;
        for chunk in bytes.chunks(3) {
            file.update(chunk);
        }
        file.end()?;
    }
    Ok(digest.finish().to_hex())
}

#[test]
fn same_files_same_digest_any_chunking() {
    let a = digest(&[("a/1.json", b"hello"), ("b.json", b"world")]).unwrap();
    assert_eq!(
        a,
        digest(&[("a/1.json", b"hello"), ("b.json", b"world")]).unwrap()
    );
    let mut whole = SourceDigest::new();
    let mut file = whole.file("a/1.json", 5).unwrap();
    file.update(b"hello");
    file.end().unwrap();
    let mut file = whole.file("b.json", 5).unwrap();
    file.update(b"world");
    file.end().unwrap();
    assert_eq!(whole.finish().to_hex(), a);
}

#[test]
fn renames_moves_and_edits_change_it() {
    let base = digest(&[("a.json", b"xy"), ("b.json", b"z")]).unwrap();
    assert_ne!(
        base,
        digest(&[("a.json", b"x"), ("b.json", b"yz")]).unwrap()
    );
    assert_ne!(
        base,
        digest(&[("a2.json", b"xy"), ("b.json", b"z")]).unwrap()
    );
    assert_ne!(
        base,
        digest(&[("a.json", b"xy"), ("b.json", b"Z")]).unwrap()
    );
}

#[test]
fn order_paths_and_lengths_are_checked() {
    assert!(matches!(
        digest(&[("b", b""), ("a", b"")]),
        Err(SourceDigestError::OutOfOrder { .. })
    ));
    assert!(matches!(
        digest(&[("a", b""), ("a", b"")]),
        Err(SourceDigestError::OutOfOrder { .. })
    ));
    for bad in ["", "/abs", "a//b", "./a", "a/../b"] {
        assert!(
            matches!(digest(&[(bad, b"")]), Err(SourceDigestError::BadPath(_))),
            "{bad}"
        );
    }
    let mut digest = SourceDigest::new();
    let mut file = digest.file("a", 4).unwrap();
    file.update(b"abc");
    assert!(matches!(
        file.end(),
        Err(SourceDigestError::Length {
            declared: 4,
            given: 3,
            ..
        })
    ));
}
