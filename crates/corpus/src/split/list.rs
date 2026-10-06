//! A dev list: `splits/<dataset>@<n>.dev.toml`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use a2a_bench_format::ids::{DatasetId, Digest, WorldKey};
use serde::Deserialize;

use super::SplitError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawList {
    dataset: String,
    version: u32,
    revision: String,
    worlds: Vec<String>,
}

/// The dev worlds of one dataset version and the source revision they were
/// listed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevList {
    dataset: DatasetId,
    version: u32,
    revision: String,
    worlds: BTreeSet<WorldKey>,
    file_name: String,
    digest: Digest,
}

/// The path of `dataset@version`'s dev list under `splits_dir`.
pub fn dev_list_path(splits_dir: &Path, dataset: &DatasetId, version: u32) -> PathBuf {
    splits_dir.join(file_name(dataset, version))
}

fn file_name(dataset: &DatasetId, version: u32) -> String {
    format!("{dataset}@{version}.dev.toml")
}

impl DevList {
    /// The list of `dataset@version` under `splits_dir`, or `None` when the
    /// dataset has none yet.
    pub fn load(
        splits_dir: &Path,
        dataset: &DatasetId,
        version: u32,
    ) -> Result<Option<Self>, SplitError> {
        let path = dev_list_path(splits_dir, dataset, version);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(SplitError::Read { path, source }),
        };
        Self::parse(&bytes, &path, dataset, version).map(Some)
    }

    /// A list from its file's bytes; `path` names it in errors, and its
    /// contents must be for `dataset@version`.
    pub fn parse(
        bytes: &[u8],
        path: &Path,
        dataset: &DatasetId,
        version: u32,
    ) -> Result<Self, SplitError> {
        let text = std::str::from_utf8(bytes).map_err(|error| SplitError::Read {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        })?;
        let raw: RawList = toml::from_str(text).map_err(|source| SplitError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        if raw.dataset != dataset.as_str() || raw.version != version {
            return Err(SplitError::Mismatch {
                path: path.to_path_buf(),
                expected: format!("{dataset}@{version}"),
                found: format!("{}@{}", raw.dataset, raw.version),
            });
        }
        let mut worlds = BTreeSet::new();
        for world in raw.worlds {
            let key = WorldKey::new(world).map_err(|source| SplitError::Key {
                path: path.to_path_buf(),
                source,
            })?;
            if let Some(duplicate) = worlds.replace(key) {
                return Err(SplitError::DuplicateWorld {
                    path: path.to_path_buf(),
                    world: duplicate,
                });
            }
        }
        Ok(Self {
            dataset: dataset.clone(),
            version,
            revision: raw.revision,
            worlds,
            file_name: file_name(dataset, version),
            digest: Digest::from_bytes(*blake3::hash(bytes).as_bytes()),
        })
    }

    pub fn dataset(&self) -> &DatasetId {
        &self.dataset
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// The source revision the list was cut from.
    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn worlds(&self) -> &BTreeSet<WorldKey> {
        &self.worlds
    }

    /// `<dataset>@<n>.dev.toml`.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// BLAKE3 of the list file's bytes.
    pub fn digest(&self) -> &Digest {
        &self.digest
    }
}
