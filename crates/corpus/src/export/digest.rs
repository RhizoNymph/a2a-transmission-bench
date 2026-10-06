//! The source digest: BLAKE3 over the dataset files a converter read,
//! sorted by path, recorded in the manifest's `source.digest`.
//!
//! The hasher is BLAKE3 in derive-key mode under [`SOURCE_DIGEST_CONTEXT`].
//! For each file, in byte order of its path relative to the dataset root
//! (`/`-separated), it absorbs
//!
//! ```text
//! path 0x00 length(u64, little-endian) contents
//! ```
//!
//! so a renamed, moved, split or edited file changes the digest.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Component, Path, PathBuf};

use a2a_bench_format::ids::Digest;

/// The BLAKE3 derive-key context of a source digest.
pub const SOURCE_DIGEST_CONTEXT: &str = "a2a-bench/1 source";

#[derive(Debug, thiserror::Error)]
pub enum DigestError {
    #[error("{path} is not under the dataset root {root}")]
    OutsideRoot { path: PathBuf, root: PathBuf },
    #[error("{0} is not a plain relative path in UTF-8")]
    BadPath(PathBuf),
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The files a converter read, as it reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilesRead {
    files: std::collections::BTreeSet<PathBuf>,
}

impl FilesRead {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `path` (absolute, or relative to the dataset root). Reading
    /// a file twice records it once.
    pub fn record(&mut self, path: impl Into<PathBuf>) {
        self.files.insert(path.into());
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The digest of the recorded files under `root`.
    pub fn digest(&self, root: &Path) -> Result<Digest, DigestError> {
        source_digest(root, self.files.iter().cloned())
    }
}

/// `path` relative to `root`, `/`-separated.
fn relative_text(root: &Path, path: &Path) -> Result<String, DigestError> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root)
            .map_err(|_| DigestError::OutsideRoot {
                path: path.to_path_buf(),
                root: root.to_path_buf(),
            })?
    } else {
        path
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| DigestError::BadPath(path.to_path_buf()))?,
            ),
            Component::CurDir => {}
            _ => return Err(DigestError::BadPath(path.to_path_buf())),
        }
    }
    if parts.is_empty() {
        return Err(DigestError::BadPath(path.to_path_buf()));
    }
    Ok(parts.join("/"))
}

/// The source digest of `files` (absolute, or relative to `root`) under
/// `root` (see the module docs). A file named twice counts once.
pub fn source_digest(
    root: &Path,
    files: impl IntoIterator<Item = PathBuf>,
) -> Result<Digest, DigestError> {
    let mut sorted = BTreeMap::new();
    for path in files {
        let text = relative_text(root, &path)?;
        sorted.insert(text, path);
    }
    let mut hasher = blake3::Hasher::new_derive_key(SOURCE_DIGEST_CONTEXT);
    for (text, path) in sorted {
        let full = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let io = |source| DigestError::Io {
            path: full.clone(),
            source,
        };
        let file = File::open(&full).map_err(io)?;
        let length = file.metadata().map_err(io)?.len();
        hasher.update(text.as_bytes());
        hasher.update(&[0]);
        hasher.update(&length.to_le_bytes());
        let mut reader = std::io::Read::take(file, length);
        let copied = std::io::copy(&mut reader, &mut hasher).map_err(io)?;
        if copied != length {
            return Err(io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the file changed while it was hashed",
            )));
        }
    }
    Ok(Digest::from_bytes(*hasher.finalize().as_bytes()))
}
