//! Holdout rules the CLI owns (design §9.2): the commitment file a holdout
//! export leaves in the bench's `splits/`, the release a holdout run must
//! name, and "never inside a git repository".
//!
//! The commitment is BLAKE3 (derive-key [`COMMIT_CONTEXT`]) over each
//! holdout world key in export order, each followed by `\n`, then `0x00`,
//! then the hex of the labels file's trailer digest. It is written as one
//! hex line to `splits/<dataset>@<n>.holdout.commit`; once the file exists,
//! a holdout export of the same version must reproduce it.

use std::path::{Path, PathBuf};

use a2a_bench_corpus::split::{RELEASE_KEY, Release, SplitError, enclosing_repository};
use a2a_bench_format::files::DetectorInfo;
use a2a_bench_format::ids::{DatasetId, Digest};
use a2a_bench_format::manifest::{Manifest, Setting, Split};

/// The BLAKE3 derive-key context of a holdout commitment.
pub const COMMIT_CONTEXT: &str = "a2a-bench/1 holdout commit";

#[derive(Debug, thiserror::Error)]
pub enum HoldoutError {
    #[error("the export is not a holdout export")]
    NotHoldout,
    #[error("the export is a holdout export: it needs --holdout-release <detector>@<tag>")]
    ReleaseRequired,
    #[error("the manifest has no labels digest")]
    NoLabels,
    #[error(transparent)]
    Release(#[from] SplitError),
    #[error("the holdout export is for release {export}, not {given}")]
    ReleaseMismatch { export: String, given: String },
    #[error(
        "the detector reports version {version:?}, release {release} needs a detector at tag {tag:?}"
    )]
    Untagged {
        version: String,
        release: String,
        tag: String,
    },
    #[error("holdout output may not be written inside a git repository ({repository})")]
    InsideRepository { repository: PathBuf },
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("writing {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} does not hold a commitment")]
    Malformed { path: PathBuf },
    #[error(
        "{path} commits holdout {committed}, this export's is {actual}: the holdout changed; refused"
    )]
    Mismatch {
        path: PathBuf,
        committed: Digest,
        actual: Digest,
    },
}

/// What happened to the commitment file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Committed {
    /// No file existed; it was written.
    Recorded { path: PathBuf, commitment: Digest },
    /// The file existed and holds this commitment.
    Matched { path: PathBuf, commitment: Digest },
}

/// The commitment of a holdout export's manifest.
pub fn commitment(manifest: &Manifest) -> Result<Digest, HoldoutError> {
    if manifest.split != Split::Holdout {
        return Err(HoldoutError::NotHoldout);
    }
    let labels = manifest.files.labels.ok_or(HoldoutError::NoLabels)?;
    let mut hasher = blake3::Hasher::new_derive_key(COMMIT_CONTEXT);
    for world in &manifest.worlds {
        hasher.update(world.key.as_str().as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(&[0]);
    hasher.update(labels.to_hex().as_bytes());
    Ok(Digest::from_bytes(*hasher.finalize().as_bytes()))
}

/// `<splits>/<dataset>@<version>.holdout.commit`.
pub fn commit_path(splits_dir: &Path, dataset: &DatasetId, version: u32) -> PathBuf {
    splits_dir.join(format!("{dataset}@{version}.holdout.commit"))
}

/// Writes `commitment` to `path`, or checks it against the file there.
pub fn record_or_check(path: &Path, commitment: Digest) -> Result<Committed, HoldoutError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let committed: Digest =
                text.trim()
                    .parse()
                    .map_err(
                        |_: <Digest as std::str::FromStr>::Err| HoldoutError::Malformed {
                            path: path.to_path_buf(),
                        },
                    )?;
            if committed == commitment {
                Ok(Committed::Matched {
                    path: path.to_path_buf(),
                    commitment,
                })
            } else {
                Err(HoldoutError::Mismatch {
                    path: path.to_path_buf(),
                    committed,
                    actual: commitment,
                })
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let write = |source| HoldoutError::Write {
                path: path.to_path_buf(),
                source,
            };
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(write)?;
            }
            std::fs::write(path, format!("{}\n", commitment.to_hex())).map_err(write)?;
            Ok(Committed::Recorded {
                path: path.to_path_buf(),
                commitment,
            })
        }
        Err(source) => Err(HoldoutError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Refuses `dir` when it is (or would be created) inside a git repository.
pub fn outside_repository(dir: &Path) -> Result<(), HoldoutError> {
    match enclosing_repository(dir)? {
        Some(repository) => Err(HoldoutError::InsideRepository { repository }),
        None => Ok(()),
    }
}

/// The release a holdout run names: required for a holdout export, which
/// must be for that release, refused for any other.
pub fn run_release(
    manifest: &Manifest,
    release: Option<&str>,
) -> Result<Option<Release>, HoldoutError> {
    match (manifest.split, release) {
        (Split::Dev, None) => Ok(None),
        (Split::Dev, Some(_)) => Err(HoldoutError::NotHoldout),
        (Split::Holdout, None) => Err(HoldoutError::ReleaseRequired),
        (Split::Holdout, Some(text)) => {
            let release: Release = text.parse()?;
            let export = match manifest.selection.get(RELEASE_KEY) {
                Some(Setting::Text(export)) => export.clone(),
                _ => String::new(),
            };
            if export != release.to_string() {
                return Err(HoldoutError::ReleaseMismatch {
                    export,
                    given: release.to_string(),
                });
            }
            Ok(Some(release))
        }
    }
}

/// A holdout run's detector must be at the release's tag: its header's
/// `version` names the tag.
pub fn tagged_detector(release: &Release, detector: &DetectorInfo) -> Result<(), HoldoutError> {
    if detector.version == release.tag() {
        Ok(())
    } else {
        Err(HoldoutError::Untagged {
            version: detector.version.clone(),
            release: release.to_string(),
            tag: release.tag().to_owned(),
        })
    }
}
