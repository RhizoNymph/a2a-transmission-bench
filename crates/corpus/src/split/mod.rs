//! Dev and holdout splits (design §9.2).
//!
//! A dataset version's dev split is an explicit, committed list,
//! `splits/<dataset>@<n>.dev.toml`:
//!
//! ```toml
//! dataset = "salt"
//! version = 2
//! revision = "<the source revision the list was cut from>"
//! worlds = ["trace-0001", "trace-0002"]
//! ```
//!
//! The holdout is its complement in that source revision. Dev is what the
//! normal commands export; a holdout export needs a [`Release`] and refuses
//! to write inside any git repository. A dataset with no list yet is
//! [`Selection::Unsplit`]: a dev export of every world, recorded as such in
//! the manifest.

mod list;
mod repo;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use a2a_bench_format::ids::{DatasetId, WorldKey};
use a2a_bench_format::manifest::{Setting, Split};

pub use list::{DevList, dev_list_path};
pub use repo::enclosing_repository;

use crate::source::WorldFilter;

#[derive(Debug, thiserror::Error)]
pub enum SplitError {
    #[error("reading split list {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("split list {path} is not valid: {source}")]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("split list {path} is for {found}, its name says {expected}")]
    Mismatch {
        path: PathBuf,
        expected: String,
        found: String,
    },
    #[error("split list {path} names world {world} twice")]
    DuplicateWorld { path: PathBuf, world: WorldKey },
    #[error("split list {path}: {source}")]
    Key {
        path: PathBuf,
        source: a2a_bench_format::ids::InvalidKey,
    },
    #[error("{dataset}@{version} has no dev list, so it has no holdout")]
    NoHoldout { dataset: DatasetId, version: u32 },
    #[error("a release is <detector>@<tag>, got {0:?}")]
    Release(String),
    #[error(
        "the split list was cut from source revision {listed:?}, the source is at {source_revision:?}"
    )]
    RevisionMismatch {
        listed: String,
        source_revision: String,
    },
    #[error("a holdout export may not be written inside a git repository ({repository})")]
    InsideRepository { repository: PathBuf },
    #[error("locating {path}: {source}")]
    Locate {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The release a holdout export is for: `<detector>@<tag>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Release {
    detector: String,
    tag: String,
}

impl Release {
    pub fn new(detector: &str, tag: &str) -> Result<Self, SplitError> {
        let valid = |text: &str| {
            !text.is_empty()
                && !text.contains(['@', '/', '\\'])
                && !text.chars().any(char::is_control)
                && text != "."
                && text != ".."
        };
        if valid(detector) && valid(tag) {
            Ok(Self {
                detector: detector.to_owned(),
                tag: tag.to_owned(),
            })
        } else {
            Err(SplitError::Release(format!("{detector}@{tag}")))
        }
    }

    pub fn detector(&self) -> &str {
        &self.detector
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Where release exports go by default:
    /// `<home>/.local/share/a2a-bench/releases/<detector>/<tag>/`.
    pub fn default_dir(&self, home: &Path) -> PathBuf {
        home.join(".local/share/a2a-bench/releases")
            .join(&self.detector)
            .join(&self.tag)
    }
}

impl FromStr for Release {
    type Err = SplitError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (detector, tag) = text
            .split_once('@')
            .ok_or_else(|| SplitError::Release(text.to_owned()))?;
        Self::new(detector, tag)
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.detector, self.tag)
    }
}

/// The manifest `selection` keys a split writes; a converter's selection
/// may not use them.
pub const SPLIT_LIST_KEY: &str = "split_list";
pub const SPLIT_DIGEST_KEY: &str = "split_digest";
pub const RELEASE_KEY: &str = "release";

/// Which worlds of a dataset version an export holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// No dev list exists yet: a dev export of every world.
    Unsplit,
    /// The listed worlds.
    Dev(DevList),
    /// Every world the list does not name, for one release.
    Holdout { dev: DevList, release: Release },
}

impl Selection {
    /// The dev selection of `dataset@version`: its list under `splits_dir`,
    /// or [`Selection::Unsplit`] when it has none.
    pub fn dev(splits_dir: &Path, dataset: &DatasetId, version: u32) -> Result<Self, SplitError> {
        Ok(match DevList::load(splits_dir, dataset, version)? {
            Some(list) => Self::Dev(list),
            None => Self::Unsplit,
        })
    }

    /// The holdout of `dataset@version` for `release`: refused when the
    /// dataset has no dev list.
    pub fn holdout(
        splits_dir: &Path,
        dataset: &DatasetId,
        version: u32,
        release: Release,
    ) -> Result<Self, SplitError> {
        match DevList::load(splits_dir, dataset, version)? {
            Some(dev) => Ok(Self::Holdout { dev, release }),
            None => Err(SplitError::NoHoldout {
                dataset: dataset.clone(),
                version,
            }),
        }
    }

    /// The manifest's `split`.
    pub fn split(&self) -> Split {
        match self {
            Self::Unsplit | Self::Dev(_) => Split::Dev,
            Self::Holdout { .. } => Split::Holdout,
        }
    }

    /// The worlds kept.
    pub fn filter(&self) -> WorldFilter {
        match self {
            Self::Unsplit => WorldFilter::All,
            Self::Dev(list) => WorldFilter::Only(list.worlds().clone()),
            Self::Holdout { dev, .. } => WorldFilter::Except(dev.worlds().clone()),
        }
    }

    /// The list, when there is one.
    pub fn list(&self) -> Option<&DevList> {
        match self {
            Self::Unsplit => None,
            Self::Dev(list) | Self::Holdout { dev: list, .. } => Some(list),
        }
    }

    /// What the manifest's `selection` records of the split: the list's
    /// file name and digest (`none` when unsplit) and, for a holdout, the
    /// release.
    pub fn settings(&self) -> BTreeMap<String, Setting> {
        let mut out = BTreeMap::new();
        match self.list() {
            None => {
                out.insert(SPLIT_LIST_KEY.to_owned(), Setting::Text("none".to_owned()));
            }
            Some(list) => {
                out.insert(
                    SPLIT_LIST_KEY.to_owned(),
                    Setting::Text(list.file_name().to_owned()),
                );
                out.insert(
                    SPLIT_DIGEST_KEY.to_owned(),
                    Setting::Text(list.digest().to_hex()),
                );
            }
        }
        if let Self::Holdout { release, .. } = self {
            out.insert(RELEASE_KEY.to_owned(), Setting::Text(release.to_string()));
        }
        out
    }

    /// Checks the selection against the source revision and, for a
    /// holdout, the output directory: a list applies only to the revision
    /// it was cut from, and a holdout is never written inside a git
    /// repository.
    pub fn check(&self, source_revision: &str, out_dir: &Path) -> Result<(), SplitError> {
        if let Some(list) = self.list()
            && list.revision() != source_revision
        {
            return Err(SplitError::RevisionMismatch {
                listed: list.revision().to_owned(),
                source_revision: source_revision.to_owned(),
            });
        }
        if let Self::Holdout { .. } = self
            && let Some(repository) = enclosing_repository(out_dir)?
        {
            return Err(SplitError::InsideRepository { repository });
        }
        Ok(())
    }
}
