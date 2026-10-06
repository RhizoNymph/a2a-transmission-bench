//! Where datasets live: `datasets.toml` names a root directory and each
//! dataset's path under it, and the source revision its exports are pinned
//! to. No dataset bytes are in the repository.
//!
//! ```toml
//! root = "~/Data/ai/agents"
//!
//! [datasets.salt]
//! path = "salt-nlp"
//! revision = ""
//! ```
//!
//! A leading `~` expands to `$HOME`. A dataset path may be absolute. An
//! empty (or absent) revision means none is pinned yet.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetConfig {
    pub path: String,
    /// The pinned source revision (an HF snapshot hash or a git HEAD);
    /// empty when none is pinned.
    #[serde(default)]
    pub revision: String,
}

impl DatasetConfig {
    /// The pinned revision, if any.
    pub fn pinned_revision(&self) -> Option<&str> {
        Some(self.revision.as_str()).filter(|revision| !revision.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetsConfig {
    pub root: String,
    #[serde(default)]
    pub datasets: BTreeMap<String, DatasetConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("config {path} is not valid: {source}")]
    Parse {
        path: String,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("dataset {0:?} is not configured")]
    UnknownDataset(String),
}

/// The default data root.
pub const DEFAULT_ROOT: &str = "~/Data/ai/agents";

/// Each dataset's directory under the default root (the shipped
/// `datasets.toml` says the same).
pub const DEFAULT_DATASETS: &[(&str, &str)] = &[
    ("salt", "salt-nlp"),
    ("agentdojo", "agentdojo"),
    ("tau2", "tau2-bench/data/tau2/results/final"),
    ("ai-village", "ai-village"),
    ("collusion-wiki", "collusion-wiki"),
    ("swarm-traces", "swarm-traces"),
    ("open_swe", "open-swe-traces"),
    ("lmcache", "lmcache"),
    ("swe_splice", "open-swe-traces"),
    ("cipher", "steganographic-evals/datasets/message_data"),
];

impl Default for DatasetsConfig {
    /// [`DEFAULT_ROOT`], with each dataset at its [`DEFAULT_DATASETS`] path
    /// and no revision pinned.
    fn default() -> Self {
        let datasets = DEFAULT_DATASETS
            .iter()
            .map(|(name, path)| {
                (
                    (*name).to_owned(),
                    DatasetConfig {
                        path: (*path).to_owned(),
                        revision: String::new(),
                    },
                )
            })
            .collect();
        Self {
            root: DEFAULT_ROOT.to_owned(),
            datasets,
        }
    }
}

impl DatasetsConfig {
    pub fn parse(text: &str, path: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(source),
        })
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: shown.clone(),
            source,
        })?;
        Self::parse(&text, &shown)
    }

    pub fn dataset(&self, dataset: &str) -> Result<&DatasetConfig, ConfigError> {
        self.datasets
            .get(dataset)
            .ok_or_else(|| ConfigError::UnknownDataset(dataset.to_owned()))
    }

    /// The data root, `~` expanded.
    pub fn root(&self, home: Option<&Path>) -> PathBuf {
        expand(&self.root, home)
    }

    /// The directory of `dataset`.
    pub fn dataset_root(&self, dataset: &str, home: Option<&Path>) -> Result<PathBuf, ConfigError> {
        let path = expand(&self.dataset(dataset)?.path, home);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(self.root(home).join(path))
        }
    }
}

/// `text` with a leading `~` replaced by `home`.
pub fn expand(text: &str, home: Option<&Path>) -> PathBuf {
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(text),
    }
}

/// `$HOME`, when set.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
