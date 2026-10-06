//! `manifest.json`: what an export holds and everything that fixes its bytes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{DatasetId, Digest, WorldKey};
use crate::version::Format;

/// Which split of a dataset version an export holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Dev,
    Holdout,
}

/// The dataset as read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The dataset's directory relative to the data root.
    pub path: String,
    /// The dataset's own revision: an HF snapshot hash or a git HEAD.
    pub revision: String,
    /// BLAKE3 over the files read, in path order.
    pub digest: Digest,
}

/// The converter that wrote the export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Converter {
    pub version: String,
    /// The bench commit.
    pub git: String,
}

/// One value of a selection or pace setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Setting {
    Bool(bool),
    Int(i64),
    Text(String),
    /// A repeatable setting, in the order given.
    List(Vec<Setting>),
}

/// A world the export holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldEntry {
    pub key: WorldKey,
    pub exchanges: u64,
    /// The world's label rows. Truth: absent in the input view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<u64>,
    /// Converter counts worth reporting (labels it could not place, groups
    /// it did not label), by name. Truth-derived: absent in the input view.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, u64>,
}

/// The digests of an export's files: their trailers' digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDigests {
    pub messages: Digest,
    pub exchanges: Digest,
    /// Absent in the input view handed to a detector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: Format,
    pub dataset: DatasetId,
    /// `<dataset>@<n>`: bumped whenever the exported bytes change for the
    /// same source revision and selection.
    pub dataset_version: u32,
    pub split: Split,
    pub source: Source,
    pub converter: Converter,
    pub selection: BTreeMap<String, Setting>,
    pub pace: BTreeMap<String, Setting>,
    pub worlds: Vec<WorldEntry>,
    pub files: FileDigests,
}

impl Manifest {
    /// The manifest a detector sees: no labels digest, label counts or notes.
    pub fn input_view(&self) -> Self {
        let mut view = self.clone();
        view.files.labels = None;
        for world in &mut view.worlds {
            world.labels = None;
            world.notes.clear();
        }
        view
    }

    /// The digest predictions name: the keyed BLAKE3 of the input view's
    /// canonical JSON, so the full manifest and the input view have the same
    /// one.
    pub fn digest(&self) -> Result<Digest, serde_json::Error> {
        let text = serde_json::to_string(&self.input_view())?;
        let canonical =
            crate::json::CanonicalJson::canonicalize(&text).map_err(serde::de::Error::custom)?;
        Ok(Digest::keyed(
            MANIFEST_DIGEST_CONTEXT,
            canonical.as_str().as_bytes(),
        ))
    }
}

/// The BLAKE3 derive-key context of a manifest digest.
pub const MANIFEST_DIGEST_CONTEXT: &str = "a2a-bench/1 manifest";
