//! A dataset directory's own revision, and the pin in `datasets.toml`
//! (design §8).
//!
//! - **HF snapshot**: the directory's resolved path (symlinks followed)
//!   holds `snapshots/<hash>`; the revision is `<hash>`.
//! - **git clone**: the directory is in a git work tree whose `HEAD`
//!   resolves; the revision is that commit. A work tree at or above the data
//!   root is not a dataset's clone (the root may sit in an unrelated
//!   repository) and is ignored.
//! - otherwise [`UNVERSIONED`].
//!
//! A pinned revision that differs from the actual one refuses the export
//! unless the caller allows it; the actual revision is what the manifest
//! records either way.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// The revision of a directory that is neither an HF snapshot nor a clone.
pub const UNVERSIONED: &str = "unversioned";

/// Where a dataset directory's revision came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    HfSnapshot(String),
    Git(String),
    Unversioned,
}

impl Revision {
    pub fn as_str(&self) -> &str {
        match self {
            Self::HfSnapshot(hash) | Self::Git(hash) => hash,
            Self::Unversioned => UNVERSIONED,
        }
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RevisionError {
    #[error("resolving the dataset directory {path}: {source}")]
    Resolve {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(
        "datasets.toml pins revision {pinned:?}, the dataset is at {actual:?}; pass --allow-revision to export it anyway"
    )]
    Mismatch { pinned: String, actual: String },
}

/// How the actual revision compares with the pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pin {
    /// No revision is pinned.
    Unpinned,
    /// The pin is the actual revision.
    Matches,
    /// The pin differs and the caller allowed it.
    Overridden { pinned: String },
}

/// The revision of `dataset_dir`; `data_root` (when known) bounds which git
/// work trees count as the dataset's.
pub fn source_revision(
    dataset_dir: &Path,
    data_root: Option<&Path>,
) -> Result<Revision, RevisionError> {
    let resolved = dataset_dir
        .canonicalize()
        .map_err(|source| RevisionError::Resolve {
            path: dataset_dir.to_path_buf(),
            source,
        })?;
    if let Some(hash) = hf_snapshot(&resolved) {
        return Ok(Revision::HfSnapshot(hash));
    }
    let root = data_root.and_then(|root| root.canonicalize().ok());
    Ok(git_head(&resolved, root.as_deref()).map_or(Revision::Unversioned, Revision::Git))
}

/// The `<hash>` of the last `snapshots/<hash>` in `path`.
fn hf_snapshot(path: &Path) -> Option<String> {
    let names: Vec<&str> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    names
        .windows(2)
        .rev()
        .find(|pair| pair[0] == "snapshots" && !pair[1].is_empty())
        .map(|pair| pair[1].to_owned())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// `HEAD` of the work tree holding `dir`, unless that work tree is at or
/// above `data_root`.
fn git_head(dir: &Path, data_root: Option<&Path>) -> Option<String> {
    let top = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?);
    let top = top.canonicalize().unwrap_or(top);
    if let Some(root) = data_root
        && root.starts_with(&top)
    {
        tracing::debug!(dir = %dir.display(), work_tree = %top.display(), "git work tree encloses the data root; not the dataset's clone");
        return None;
    }
    git(dir, &["rev-parse", "HEAD"])
}

/// Checks `actual` against the `pinned` revision: equal or unpinned is
/// fine; a difference is refused unless `allow`.
pub fn check_pin(pinned: Option<&str>, actual: &str, allow: bool) -> Result<Pin, RevisionError> {
    match pinned {
        None => Ok(Pin::Unpinned),
        Some(pinned) if pinned == actual => Ok(Pin::Matches),
        Some(pinned) if allow => Ok(Pin::Overridden {
            pinned: pinned.to_owned(),
        }),
        Some(pinned) => Err(RevisionError::Mismatch {
            pinned: pinned.to_owned(),
            actual: actual.to_owned(),
        }),
    }
}
