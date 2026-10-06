//! A dataset directory's own revision, and the pin in `datasets.toml`
//! (design §8).
//!
//! - **HF snapshot**: the directory's resolved path (symlinks followed)
//!   holds `snapshots/<hash>`; the revision is `<hash>`.
//! - **HF local dir**: the directory was written by `hf download
//!   --local-dir`, which leaves `.cache/huggingface/download/**/*.metadata`
//!   (first line the commit, then the etag and a timestamp). The revision is
//!   the commit they share; when they disagree it is [`MIXED`] (and a
//!   warning). A metadata file whose first line is not a commit is skipped.
//! - **git clone**: the directory is in a git work tree whose `HEAD`
//!   resolves; the revision is that commit. A work tree at or above the data
//!   root is not a dataset's clone (the root may sit in an unrelated
//!   repository) and is ignored.
//! - otherwise [`UNKNOWN`] (the spelling crosstalk's golden export uses).
//!
//! A pinned revision that differs from the actual one refuses the export
//! unless the caller allows it; the actual revision is what the manifest
//! records either way.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// The revision of a directory that is neither an HF download nor a clone.
pub const UNKNOWN: &str = "unknown";

/// The revision of an HF local dir whose files come from several commits.
pub const MIXED: &str = "mixed";

/// Where `hf download --local-dir` keeps its per-file metadata.
pub const HF_DOWNLOAD_CACHE: &str = ".cache/huggingface/download";

/// Where a dataset directory's revision came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revision {
    HfSnapshot(String),
    /// The commit every metadata file of an HF local dir names.
    HfLocalDir(String),
    /// An HF local dir whose metadata files name these commits (sorted,
    /// two or more).
    Mixed {
        commits: Vec<String>,
    },
    Git(String),
    Unknown,
}

impl Revision {
    pub fn as_str(&self) -> &str {
        match self {
            Self::HfSnapshot(hash) | Self::HfLocalDir(hash) | Self::Git(hash) => hash,
            Self::Mixed { .. } => MIXED,
            Self::Unknown => UNKNOWN,
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
    #[error("reading the HF download metadata {path}: {source}")]
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
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
    if let Some(revision) = hf_local_dir(&resolved)? {
        if let Revision::Mixed { commits } = &revision {
            tracing::warn!(dir = %resolved.display(), commits = ?commits, "HF download metadata names several commits; revision recorded as mixed");
        }
        return Ok(revision);
    }
    let root = data_root.and_then(|root| root.canonicalize().ok());
    Ok(git_head(&resolved, root.as_deref()).map_or(Revision::Unknown, Revision::Git))
}

/// The commits named by an HF local dir's metadata files: `None` when
/// there is no download cache or no file in it names a commit.
fn hf_local_dir(dir: &Path) -> Result<Option<Revision>, RevisionError> {
    let cache = dir.join(HF_DOWNLOAD_CACHE);
    if !cache.is_dir() {
        return Ok(None);
    }
    let mut commits = BTreeSet::new();
    let mut pending = vec![cache];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|source| RevisionError::Metadata {
            path: dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| RevisionError::Metadata {
                path: dir.clone(),
                source,
            })?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|source| RevisionError::Metadata {
                    path: path.clone(),
                    source,
                })?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "metadata") {
                match metadata_commit(&path)? {
                    Some(commit) => {
                        commits.insert(commit);
                    }
                    None => {
                        tracing::debug!(path = %path.display(), "HF download metadata names no commit; skipped");
                    }
                }
            }
        }
    }
    let mut commits: Vec<String> = commits.into_iter().collect();
    Ok(match commits.len() {
        0 => None,
        1 => commits.pop().map(Revision::HfLocalDir),
        _ => Some(Revision::Mixed { commits }),
    })
}

/// The commit on the first line of a metadata file, if it is one (40
/// lowercase hex digits).
fn metadata_commit(path: &Path) -> Result<Option<String>, RevisionError> {
    let file = File::open(path).map_err(|source| RevisionError::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    let mut line = String::new();
    BufReader::new(file)
        .take(256)
        .read_line(&mut line)
        .map_err(|source| RevisionError::Metadata {
            path: path.to_path_buf(),
            source,
        })?;
    let line = line.trim_end();
    let is_commit = line.len() == 40
        && line
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    Ok(is_commit.then(|| line.to_owned()))
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
