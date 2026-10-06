//! The source digest: BLAKE3 over the dataset files a converter read,
//! recorded in the manifest's `source.digest`.
//!
//! BLAKE3 in derive-key mode under [`SOURCE_DIGEST_CONTEXT`]. For each file,
//! in byte order of its path relative to the dataset root (`/`-separated,
//! UTF-8), it absorbs
//!
//! ```text
//! path 0x00 length(u64, little-endian) contents
//! ```
//!
//! so a renamed, moved, split or edited file changes the digest. This is the
//! definition every exporter (the bench's converters, crosstalk's golden
//! export) uses; changing it is a format major.

use crate::ids::Digest;

/// The BLAKE3 derive-key context of a source digest.
pub const SOURCE_DIGEST_CONTEXT: &str = "a2a-bench/1 source";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceDigestError {
    #[error("{path:?} is not after {previous:?}: files must come in path byte order, once each")]
    OutOfOrder { previous: String, path: String },
    #[error("{0:?} is not a plain relative path")]
    BadPath(String),
    #[error("{path:?} declared {declared} bytes and gave {given}")]
    Length {
        path: String,
        declared: u64,
        given: u64,
    },
}

/// Builds a source digest one file at a time; the caller reads the files.
#[derive(Debug, Clone)]
pub struct SourceDigest {
    hasher: blake3::Hasher,
    previous: Option<String>,
}

impl Default for SourceDigest {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceDigest {
    pub fn new() -> Self {
        Self {
            hasher: blake3::Hasher::new_derive_key(SOURCE_DIGEST_CONTEXT),
            previous: None,
        }
    }

    /// Starts the next file: `path` relative to the dataset root and its length.
    pub fn file(&mut self, path: &str, len: u64) -> Result<FileDigest<'_>, SourceDigestError> {
        let plain = !path.is_empty()
            && !path.starts_with('/')
            && path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        if !plain {
            return Err(SourceDigestError::BadPath(path.to_owned()));
        }
        if let Some(previous) = &self.previous
            && previous.as_bytes() >= path.as_bytes()
        {
            return Err(SourceDigestError::OutOfOrder {
                previous: previous.clone(),
                path: path.to_owned(),
            });
        }
        self.previous = Some(path.to_owned());
        self.hasher.update(path.as_bytes());
        self.hasher.update(&[0]);
        self.hasher.update(&len.to_le_bytes());
        Ok(FileDigest {
            owner: self,
            path: path.to_owned(),
            declared: len,
            given: 0,
        })
    }

    pub fn finish(&self) -> Digest {
        Digest::from_bytes(*self.hasher.finalize().as_bytes())
    }
}

/// One file's contents, fed in any number of chunks.
#[derive(Debug)]
pub struct FileDigest<'a> {
    owner: &'a mut SourceDigest,
    path: String,
    declared: u64,
    given: u64,
}

impl FileDigest<'_> {
    pub fn update(&mut self, bytes: &[u8]) {
        self.given = self
            .given
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        self.owner.hasher.update(bytes);
    }

    /// Ends the file; its contents must have been exactly the declared length.
    pub fn end(self) -> Result<(), SourceDigestError> {
        if self.given == self.declared {
            Ok(())
        } else {
            Err(SourceDigestError::Length {
                path: self.path,
                declared: self.declared,
                given: self.given,
            })
        }
    }
}
